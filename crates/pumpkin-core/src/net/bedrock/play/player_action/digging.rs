//! The digging actions of a Bedrock player: start, creative and continue destroy (progress or
//! an instant break), and the client's predicted or stopped break.

#[allow(clippy::wildcard_imports)]
use super::*;
use crate::entity::Entity;
use crate::world::World;
use pumpkin_data::{Block, BlockState};

impl BedrockClient {
    /// Start, creative and continue destroy at `location`.
    pub(super) fn destroy_block(player: &Arc<Player>, server: &Arc<Server>, location: BlockPos) {
        if !player.can_interact_with_block_at(&location, 1.0) {
            return;
        }

        let entity = &player.get_entity();
        let world = entity.world.load_full();
        let (block, state) = world.get_block_and_state(&location);

        if player.mining.load(Ordering::Relaxed)
            && *player
                .mining_pos
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                != location
        {
            player.stop_mining();
        }

        if player.gamemode.load() == GameMode::Creative {
            Self::creative_destroy(player, server, &world, block, state, location);
        } else if !state.is_air() {
            Self::survival_dig(player, server, &world, entity, block, state, location);
        }
    }

    fn creative_destroy(
        player: &Arc<Player>,
        server: &Arc<Server>,
        world: &Arc<World>,
        block: &'static Block,
        state: &'static BlockState,
        location: BlockPos,
    ) {
        let new_state = world.break_block(
            &location,
            Some(player),
            BlockFlags::NOTIFY_ALL | BlockFlags::SKIP_DROPS,
        );
        if new_state.is_some() {
            server
                .block_registry
                .broken(world, block, player, &location, server, state);
        }
    }

    fn survival_dig(
        player: &Arc<Player>,
        server: &Arc<Server>,
        world: &Arc<World>,
        entity: &Entity,
        block: &'static Block,
        state: &'static BlockState,
        location: BlockPos,
    ) {
        let starts_breaking = !player.mining.load(Ordering::Relaxed)
            || *player
                .mining_pos
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                != location;
        if starts_breaking {
            server
                .block_registry
                .attack(world, block, state, &location, player);
        }
        let speed = crate::block::calc_block_breaking(player, state, block);
        if speed >= 1.0 {
            Self::instant_break(player, server, world, block, location);
        } else {
            Self::progress_dig(player, world, entity, location, speed, starts_breaking);
        }
    }

    fn instant_break(
        player: &Arc<Player>,
        server: &Arc<Server>,
        world: &Arc<World>,
        block: &'static Block,
        location: BlockPos,
    ) {
        player.stop_mining();
        let broken_state = world.get_block_state(&location);
        let can_harvest = player.can_harvest(broken_state, block);
        let flags = if can_harvest {
            BlockFlags::NOTIFY_ALL
        } else {
            BlockFlags::SKIP_DROPS | BlockFlags::NOTIFY_ALL
        };
        let new_state = world.break_block(&location, Some(player), flags);
        if new_state.is_some() {
            server
                .block_registry
                .broken(world, block, player, &location, server, broken_state);
            player.apply_tool_damage_for_block_break(broken_state);
            if can_harvest {
                player.add_exhaustion(MINE_BLOCK_EXHAUSTION);
            }
            let item_id = player.inventory().held_item().item.id;
            player.increment_stat(
                pumpkin_data::statistic::StatisticCategory::Used,
                item_id as i32,
                1,
            );
            player.increment_stat(
                pumpkin_data::statistic::StatisticCategory::Mined,
                block.id.as_u16() as i32,
                1,
            );
        }
    }

    fn progress_dig(
        player: &Arc<Player>,
        world: &Arc<World>,
        entity: &Entity,
        location: BlockPos,
        speed: f32,
        starts_breaking: bool,
    ) {
        let mut mining_pos = player
            .mining_pos
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let progress = if starts_breaking {
            player.start_mining_time.store(
                player.tick_counter.load(Ordering::Relaxed),
                Ordering::Relaxed,
            );
            player.mining.store(true, Ordering::Relaxed);
            *mining_pos = location;
            (speed * 10.0) as i32
        } else {
            player.current_block_destroy_stage.load(Ordering::Relaxed)
        };
        drop(mining_pos);
        let old_speed = player
            .current_block_breaking_speed
            .swap(speed.to_bits(), Ordering::Relaxed);
        if starts_breaking {
            world.set_block_breaking(
                entity,
                location,
                BlockBreakingProgress::Start {
                    stage: progress,
                    speed,
                },
            );
            player
                .current_block_destroy_stage
                .store(progress, Ordering::Relaxed);
        } else if old_speed != speed.to_bits() {
            world.set_block_breaking(
                entity,
                location,
                BlockBreakingProgress::Update {
                    stage: progress,
                    speed: Some(speed),
                },
            );
        }
    }

    /// Predict and stop destroy at `location`.
    pub(super) fn predict_destroy_block(
        &self,
        player: &Arc<Player>,
        server: &Arc<Server>,
        location: BlockPos,
        action: PlayerAction,
    ) {
        if !player.can_interact_with_block_at(&location, 1.0) {
            return;
        }

        let entity = &player.get_entity();
        let world = entity.world.load_full();

        let (block, state) = world.get_block_and_state(&location);
        if player.gamemode.load() != GameMode::Creative && !state.is_air() {
            let speed = crate::block::calc_block_breaking(player, state, block);
            let elapsed = player.tick_counter.load(Ordering::Relaxed)
                - player.start_mining_time.load(Ordering::Relaxed)
                + 1;
            let same_block = *player
                .mining_pos
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                == location;
            if player.mining.load(Ordering::Relaxed)
                && same_block
                && speed * elapsed as f32 >= MIN_PREDICTED_BREAK_PROGRESS
            {
                player.stop_mining();

                let can_harvest = player.can_harvest(state, block);
                let flags = if can_harvest {
                    BlockFlags::NOTIFY_ALL
                } else {
                    BlockFlags::SKIP_DROPS | BlockFlags::NOTIFY_ALL
                };
                if world.break_block(&location, Some(player), flags).is_some() {
                    server
                        .block_registry
                        .broken(&world, block, player, &location, server, state);
                    player.apply_tool_damage_for_block_break(state);
                    if can_harvest {
                        player.add_exhaustion(MINE_BLOCK_EXHAUSTION);
                    }
                    let item_id = player.inventory().held_item().item.id;
                    player.increment_stat(
                        pumpkin_data::statistic::StatisticCategory::Used,
                        item_id as i32,
                        1,
                    );
                    player.increment_stat(
                        pumpkin_data::statistic::StatisticCategory::Mined,
                        block.id.as_u16() as i32,
                        1,
                    );
                }
            } else {
                let runtime_id = pumpkin_data::BlockState::to_be_network_id(state.id);
                self.try_enqueue_client_packet(&CUpdateBlock::new(location, runtime_id));
                if matches!(action, PlayerAction::StopDestroyBlock) {
                    player.stop_mining();
                } else {
                    world.set_block_breaking(
                        entity,
                        location,
                        BlockBreakingProgress::Update {
                            stage: player.current_block_destroy_stage.load(Ordering::Relaxed),
                            speed: Some(speed),
                        },
                    );
                }
            }
        } else if matches!(action, PlayerAction::StopDestroyBlock) {
            player.stop_mining();
        }
    }
}
