//! The per-tick work of a player, one method per phase.
//!
//! `Player::tick` runs the phases in a fixed order; the statements are the ones the single long
//! method had, in the same order.

use super::super::EntityBase;
use super::{MINE_BLOCK_EXHAUSTION, Player, statistics};
use crate::net::ClientPlatform;
use crate::net::DisconnectReason;
use crate::net::chunk_sender::PreparedBatch;
use crate::server::Server;
use crate::world::World;
use pumpkin_data::statistic::StatisticCategory;
use pumpkin_data::{Block, translation};
use pumpkin_inventory::merchant::merchant_screen_handler::MerchantScreenHandler;
use pumpkin_protocol::bedrock::client::play_status::CPlayStatus;
use pumpkin_protocol::bedrock::server::respawn::RespawnState;
use pumpkin_protocol::java::client::play::CSetCamera;
use pumpkin_util::Difficulty;
use pumpkin_util::text::TextComponent;
use pumpkin_util::version::JavaMinecraftVersion;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

impl Player {
    /// Follows the camera target, or resets the camera when the target is gone.
    pub(super) fn tick_camera(&self) {
        if let Some(camera_id) = self.camera_target_id.load() {
            if camera_id == self.entity_id() {
                self.camera_target_id.store(None);
            } else {
                let world = self.world();
                let target = world
                    .get_player_by_id(camera_id)
                    .map(|p| Arc::clone(&p) as Arc<dyn EntityBase>)
                    .or_else(|| world.get_entity_by_id(camera_id));
                if let Some(target) = target {
                    let target_pos = target.get_entity().pos.load();
                    let player_pos = self.living_entity.entity.pos.load();
                    if player_pos != target_pos {
                        self.living_entity.entity.set_pos(target_pos);
                        if let Some(p) = self.world().get_player_by_uuid(self.gameprofile.id) {
                            crate::world::chunker::update_position(&p);
                        }
                    }
                } else {
                    // Target no longer exists, reset camera back to player
                    self.camera_target_id.store(None);
                    self.try_send_client_packet(&CSetCamera::new(self.entity_id().into()));
                }
            }
        }
    }

    /// Closes a merchant screen that is out of range, or sends the screen's content updates.
    pub(super) fn tick_screen_handler(&self) {
        if let Ok(current_screen_handler_guard) = self.current_screen_handler.try_lock() {
            let current_screen_handler = current_screen_handler_guard.clone();
            drop(current_screen_handler_guard);
            let is_invalid = current_screen_handler
                .try_lock()
                .is_ok_and(|screen_handler| {
                    screen_handler.as_any().is::<MerchantScreenHandler>()
                        && !screen_handler.can_use(self)
                });

            if is_invalid {
                if let Some(p) = self.world().get_player_by_uuid(self.gameprofile.id) {
                    p.close_handled_screen();
                }
            } else if let Ok(mut screen_handler) = current_screen_handler.try_lock() {
                screen_handler.send_content_updates();
            }
        }
    }

    /// Counts the tick in the statistics.
    pub(super) fn tick_statistics(&self) {
        if let Ok(mut stats) = self.stats.try_lock() {
            stats.increment_custom(statistics::CustomStatistic::PlayTime, 1);
            stats.increment_custom(statistics::CustomStatistic::TotalWorldTime, 1);
            if !self.living_entity.dead.load(Ordering::Relaxed)
                && self.living_entity.health.load() > 0.0
            {
                stats.increment_custom(statistics::CustomStatistic::TimeSinceDeath, 1);
            }
            if !self.is_sleeping() {
                stats.increment_custom(statistics::CustomStatistic::TimeSinceRest, 1);
            }
            if self.living_entity.entity.sneaking.load(Ordering::Relaxed) {
                stats.increment_custom(statistics::CustomStatistic::SneakTime, 1);
            }
        }
    }

    /// The experience pick-up delay and the warden spawn tracker.
    pub(super) fn tick_timers(&self) {
        if let Ok(mut xp) = self.experience_pick_up_delay.try_lock()
            && *xp > 0
        {
            *xp -= 1;
        }
        self.warden_spawn_tracker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .tick();
    }

    /// Queues the chunks the world announced, sends a batch and returns how many chunks were
    /// sent in total.
    pub(super) fn send_chunks(&self) -> usize {
        if let Ok(listener) = self.chunk_listener.try_lock()
            && let Ok(mut sender) = self.chunk_sender.try_lock()
        {
            let watched = self.watched_section.load();
            while let Ok((pos, _)) = listener.try_recv() {
                if watched.is_within_distance(pos.x, pos.y) {
                    sender.enqueue_chunk(pos);
                }
            }
        }
        let world = self.world();
        let player_chunk = self.get_entity().chunk_pos.load();
        let epoch = self.chunk_send_epoch.load(Ordering::Relaxed);
        let version = match self.client.as_ref() {
            ClientPlatform::Java(java_client) => java_client.version.load(),
            ClientPlatform::Bedrock(_) => JavaMinecraftVersion::V_1_20_2,
        };

        let view_distance = self.watched_section.load().view_distance;
        let prepared_batch = self.chunk_sender.try_lock().ok().and_then(|mut sender| {
            sender.prepare_batch(&world.level, player_chunk, view_distance, epoch, version)
        });

        prepared_batch.map_or_else(
            || {
                self.chunk_sender
                    .try_lock()
                    .map_or(0, |s| s.sent_chunks_count())
            },
            |batch| match self.client.as_ref() {
                ClientPlatform::Java(_) => self.commit_java_batch(&world, &batch),
                ClientPlatform::Bedrock(_) => self.commit_bedrock_batch(&world, &batch),
            },
        )
    }

    /// Encodes and sends a prepared batch to a Java client.
    fn commit_java_batch(&self, world: &Arc<World>, batch: &PreparedBatch) -> usize {
        let mut per_player_cache = rustc_hash::FxHashMap::default();
        let encoded = crate::net::ChunkSender::encode_batch(batch, &mut per_player_cache);
        let current_epoch = self.chunk_send_epoch.load(Ordering::Relaxed);
        let (sent, total_sent_chunks) = self.chunk_sender.try_lock().map_or_else(
            |_| (Vec::new(), 0),
            |mut sender| {
                let sent = sender.commit_batch(batch, &encoded, &self.client, current_epoch);
                (sent, sender.sent_chunks_count())
            },
        );
        self.pair_entities_in_chunks(world, &sent);
        total_sent_chunks
    }

    /// Sends a prepared batch to a Bedrock client from a task and returns the sent count.
    fn commit_bedrock_batch(&self, world: &Arc<World>, batch: &PreparedBatch) -> usize {
        let current_epoch = self.chunk_send_epoch.load(Ordering::Relaxed);
        let (chunks, total_sent_chunks) = self.chunk_sender.try_lock().map_or_else(
            |_| (Vec::new(), 0),
            |mut sender| {
                let chunks = sender.commit_bedrock_batch(batch, current_epoch);
                let total_sent_chunks = sender.sent_chunks_count();
                (chunks, total_sent_chunks)
            },
        );
        if !chunks.is_empty() {
            let client = self.client.clone();
            let world = Arc::clone(world);
            let uuid = self.gameprofile.id;
            self.spawn_task(async move {
                let (deliveries, chunks): (Vec<_>, Vec<_>) = chunks
                    .into_iter()
                    .map(|c| ((c.position, c.delivery_token), c.chunk))
                    .unzip();
                client.send_chunks(&chunks).await;
                if let Some(player) = world.get_player_by_uuid(uuid) {
                    // dispatcher sets a reset or a re-enqueue since then holds a newer token.
                    let delivered = player
                        .chunk_sender
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .mark_delivered(&deliveries);
                    player.pair_entities_in_chunks(&world, &delivered);
                }
            });
        }
        total_sent_chunks
    }

    /// Tells a Bedrock client to spawn once enough chunks reached it.
    pub(super) fn finish_bedrock_spawn(&self, total_sent_chunks: usize) {
        if let ClientPlatform::Bedrock(bedrock_client) = self.client.as_ref()
            && !self.bedrock_spawned.load(Ordering::Relaxed)
            && total_sent_chunks > 4
        {
            if let Ok(data) = bedrock_client.serialize_packet(&CPlayStatus::PlayerSpawn) {
                bedrock_client.try_enqueue_packet(data);
            }
            self.bedrock_spawned.store(true, Ordering::Relaxed);
            self.set_client_loaded(true);
            self.send_health();
            if self.living_entity.health.load() <= 0.0 {
                self.send_bedrock_respawn_state(RespawnState::SearchingForSpawn);
            }
        }
    }

    /// Advances the tick counters and the sleeping timer.
    pub(super) fn advance_tick_counters(&self) {
        self.tick_counter.fetch_add(1, Ordering::Relaxed);
        self.living_entity
            .entity
            .tick_count
            .fetch_add(1, Ordering::Relaxed);
        if let Some(sleeping_since) = self.sleeping_since.load()
            && sleeping_since < 101
        {
            self.sleeping_since.store(Some(sleeping_since + 1));
        }
    }

    /// Continues a block break in progress.
    pub(super) fn tick_mining(&self) {
        if self.mining.load(Ordering::Relaxed)
            && let Some(p) = self.world().get_player_by_uuid(self.gameprofile.id)
        {
            let world_clone = p.world();
            let server_clone = world_clone.server.upgrade();
            let pos = *p
                .mining_pos
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let world = p.world();
            let state = world.get_block_state(&pos);
            // Is the block broken?
            if state.is_air() {
                p.stop_mining();
            } else {
                let finished = p.continue_mining(
                    pos,
                    &world,
                    state,
                    p.start_mining_time.load(Ordering::Relaxed),
                );
                if finished && matches!(p.client.as_ref(), ClientPlatform::Bedrock(_)) {
                    p.stop_mining();

                    let block = Block::from_state_id(state.id);
                    let can_harvest = p.can_harvest(state, block);
                    let flags = if can_harvest {
                        pumpkin_world::world::BlockFlags::NOTIFY_ALL
                    } else {
                        pumpkin_world::world::BlockFlags::SKIP_DROPS
                            | pumpkin_world::world::BlockFlags::NOTIFY_ALL
                    };
                    if world.break_block(&pos, Some(&p), flags).is_some() {
                        if let Some(server) = server_clone {
                            server
                                .block_registry
                                .broken(&world, block, &p, &pos, &server, state);
                        }
                        p.apply_tool_damage_for_block_break(state);
                        if can_harvest {
                            p.add_exhaustion(MINE_BLOCK_EXHAUSTION);
                        }
                        let item_id = p.inventory().held_item().item.id;
                        p.increment_stat(StatisticCategory::Used, item_id as i32, 1);
                        p.increment_stat(StatisticCategory::Mined, state.id.as_u16() as i32, 1);
                    }
                }
            }
        }
    }

    /// Slow health and hunger regeneration in peaceful mode.
    pub(super) fn tick_peaceful_regeneration(&self) {
        let level_info = self.world().level_info.load();
        if level_info.difficulty == Difficulty::Peaceful
            && level_info.game_rules.natural_health_regeneration
        {
            let tick_count = self.tick_counter.load(Ordering::Relaxed);
            if tick_count % 20 == 0 {
                if self.can_food_heal() {
                    self.heal(1.0);
                }

                let saturation = self.hunger_manager.saturation.load();
                if saturation < 20.0 {
                    self.hunger_manager.set_saturation(saturation + 1.0);
                }
            }

            if tick_count % 10 == 0 && self.hunger_manager.level.load() < 20 {
                self.hunger_manager.add_hunger(1);
            }
        }
    }

    /// Updates the pose, then checks and sends advancements.
    pub(super) fn tick_advancements(&self) {
        self.update_player_pose();
        self.check_inventory_advancements();
        if self.tick_counter.load(Ordering::Relaxed) % 20 == 0 {
            self.trigger_advancement(
                crate::entity::player::advancement::trigger::AdvancementTrigger::Location,
            );
            self.trigger_advancement(
                crate::entity::player::advancement::trigger::AdvancementTrigger::Tick,
            );
        }
        if let Ok(mut adv) = self.advancements.try_lock() {
            adv.flush_dirty(self, true);
        }
    }

    /// Lowers the anti-spam counter of the chat.
    pub(super) fn decay_chat_spam(&self, server: &Server) {
        let anti_spam = &server.advanced_config.chat.anti_spam;
        if anti_spam.enabled && anti_spam.decay_per_tick > 0 {
            let _ = self.chat_spam_tick_count.fetch_update(
                Ordering::Relaxed,
                Ordering::Relaxed,
                |count| Some(count.saturating_sub(anti_spam.decay_per_tick)),
            );
        }
    }

    /// Kicks the player after the idle time the server allows.
    pub(super) fn kick_when_idle(&self, server: &Server) {
        let now = Instant::now();
        let idle_timeout_minutes = server.player_idle_timeout.load(Ordering::Relaxed);
        if idle_timeout_minutes > 0 {
            let idle_duration = now.duration_since(self.last_action_time.load());
            if idle_duration >= Duration::from_secs(idle_timeout_minutes as u64 * 60) {
                self.kick(
                    DisconnectReason::KickedForIdle,
                    &TextComponent::translate_cross(
                        translation::java::MULTIPLAYER_DISCONNECT_IDLING,
                        translation::java::MULTIPLAYER_DISCONNECT_IDLING,
                        [],
                    ),
                );
            }
        }
    }
}
