//! The digging statuses of the player-action packet: start, cancel and finish.

#[allow(clippy::wildcard_imports)]
use super::super::*;
use pumpkin_data::BlockState;

impl JavaClient {
    /// `StartedDigging`: checks reach, plugin events and whether the player may mine, then
    /// breaks the block at once (creative, or fast enough) or starts mining it.
    pub(super) fn start_digging(
        &self,
        player: &Arc<Player>,
        player_action: &SPlayerAction,
        server: &Server,
    ) {
        if !player.can_interact_with_block_at(&player_action.position, 1.0) {
            warn!(
                "Player {0} tried to interact with block out of reach at {1}",
                player.gameprofile.name, player_action.position
            );
            self.update_sequence(player_action.sequence.0);
            return;
        }
        let position = player_action.position;
        let entity = &player.get_entity();
        let world = entity.world.load_full();
        let (block, state) = world.get_block_and_state(&position);

        if !self.fire_dig_start_events(player, player_action, &world, block, state) {
            return;
        }

        if player.cannot_mine(server, block) {
            player.try_send_client_packet(&CBlockUpdate::new(
                position,
                VarInt(i32::from(state.id.as_u16())),
            ));
            self.update_sequence(player_action.sequence.0);
            return;
        }

        // TODO: do validation
        // TODO: Config
        if player.gamemode.load() == GameMode::Creative {
            self.break_in_creative(player, player_action, server, &world, block, state);
            return;
        }
        player.start_mining_time.store(
            player.tick_counter.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        if !state.is_air() {
            server
                .block_registry
                .attack(&world, block, state, &position, player);
            let speed = block::calc_block_breaking(player, state, block);
            // Instant break
            if speed >= 1.0 {
                self.break_instantly(player, server, &world, position, block);
            } else {
                start_mining(player, entity, &world, position, speed);
            }
        }
        self.update_sequence(player_action.sequence.0);
    }

    /// Fires the left-click interact and block-damage events. False when a plugin cancelled
    /// one: the client then got its sequence (and, for the interact, its block back).
    fn fire_dig_start_events(
        &self,
        player: &Arc<Player>,
        player_action: &SPlayerAction,
        world: &World,
        block: &'static Block,
        state: &BlockState,
    ) -> bool {
        let position = player_action.position;
        if let Some(server_arc) = world.server.upgrade() {
            let mut interact_event = PlayerInteractEvent::new(
                player,
                InteractAction::LeftClickBlock,
                block,
                Some(position),
            );
            server_arc
                .plugin_manager
                .fire_blocking(&server_arc, &mut interact_event);
            if interact_event.cancelled {
                player.try_send_client_packet(&CBlockUpdate::new(
                    position,
                    VarInt(i32::from(state.id.as_u16())),
                ));
                self.update_sequence(player_action.sequence.0);
                return false;
            }

            let mut event = crate::plugin::api::events::block::block_damage::BlockDamageEvent::new(
                player.clone(),
                block,
                position,
                false,
            );
            server_arc
                .plugin_manager
                .fire_blocking(&server_arc, &mut event);
            if event.cancelled {
                self.update_sequence(player_action.sequence.0);
                return false;
            }
        }
        true
    }

    /// A creative player breaks the block at once, without drops.
    fn break_in_creative(
        &self,
        player: &Arc<Player>,
        player_action: &SPlayerAction,
        server: &Server,
        world: &Arc<World>,
        block: &'static Block,
        state: &BlockState,
    ) {
        let position = player_action.position;
        // Block break & play sound
        let new_state = world.break_block(
            &position,
            Some(player),
            BlockFlags::NOTIFY_ALL | BlockFlags::SKIP_DROPS,
        );
        if new_state.is_some() {
            server
                .block_registry
                .broken(world, block, player, &position, server, state);
        }
        self.sync_block_state_to_client(world, position);
        self.update_sequence(player_action.sequence.0);
    }

    /// A survival player mines the block in one tick: break it, damage the tool, count it.
    fn break_instantly(
        &self,
        player: &Arc<Player>,
        server: &Server,
        world: &Arc<World>,
        position: BlockPos,
        block: &'static Block,
    ) {
        let broken_state = world.get_block_state(&position);
        let can_harvest = player.can_harvest(broken_state, block);
        let flags = if can_harvest {
            BlockFlags::NOTIFY_ALL
        } else {
            BlockFlags::SKIP_DROPS | BlockFlags::NOTIFY_ALL
        };
        let new_state = world.break_block(&position, Some(player), flags);
        if new_state.is_some() {
            server
                .block_registry
                .broken(world, block, player, &position, server, broken_state);
            let tool = player.held_tool();
            player.apply_tool_damage_for_block_break(broken_state);
            if can_harvest {
                player.add_exhaustion(MINE_BLOCK_EXHAUSTION);
            }
            player.award_block_break_stats(tool, block, can_harvest);
        }
        self.sync_block_state_to_client(world, position);
    }

    /// `CancelledDigging`: stops mining and the breaking animation.
    pub(super) fn cancel_digging(&self, player: &Arc<Player>, player_action: &SPlayerAction) {
        if !player.can_interact_with_block_at(&player_action.position, 1.0) {
            warn!(
                "Player {0} tried to interact with block out of reach at {1}",
                player.gameprofile.name, player_action.position
            );
            self.update_sequence(player_action.sequence.0);
            return;
        }
        let entity = &player.get_entity();
        let world = entity.world.load_full();
        if let Some(server_arc) = world.server.upgrade() {
            let mut abort_event =
                crate::plugin::api::events::block::block_damage_abort::BlockDamageAbortEvent::new(
                    player.clone(),
                    player_action.position,
                    world.clone(),
                    player.inventory().held_item(),
                );
            server_arc
                .plugin_manager
                .fire_blocking(&server_arc, &mut abort_event);
        }

        player.mining.store(false, Ordering::Relaxed);
        world.set_block_breaking(entity, player_action.position, BlockBreakingProgress::Stop);
        self.update_sequence(player_action.sequence.0);
    }

    /// `FinishedDigging`: breaks the block the player mined (drops unless creative or not
    /// harvestable), damages the tool and counts it.
    pub(super) fn finish_digging(
        &self,
        player: &Arc<Player>,
        player_action: &SPlayerAction,
        server: &Server,
    ) {
        // TODO: do validation
        let location = player_action.position;
        if !player.can_interact_with_block_at(&location, 1.0) {
            warn!(
                "Player {0} tried to interact with block out of reach at {1}",
                player.gameprofile.name, player_action.position
            );
            self.update_sequence(player_action.sequence.0);
            return;
        }

        // Vanilla `STOP_DESTROY_BLOCK` acts only on the block whose digging started (a creative
        // break, or a finish with no start, leaves it alone).
        let started_at = *player
            .mining_pos
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if started_at != location {
            self.update_sequence(player_action.sequence.0);
            return;
        }

        // Block break & play sound
        let entity = &player.get_entity();
        let world = entity.world.load_full();

        player.mining.store(false, Ordering::Relaxed);
        world.set_block_breaking(entity, location, BlockBreakingProgress::Stop);

        let (block, state) = world.get_block_and_state(&location);
        let block_drop =
            player.gamemode.load() != GameMode::Creative && player.can_harvest(state, block);

        let new_state = world.break_block(
            &location,
            Some(player),
            if block_drop {
                BlockFlags::NOTIFY_ALL
            } else {
                BlockFlags::SKIP_DROPS | BlockFlags::NOTIFY_ALL
            },
        );
        if new_state.is_some() {
            server
                .block_registry
                .broken(&world, block, player, &location, server, state);

            let tool = player.held_tool();
            player.apply_tool_damage_for_block_break(state);
            if block_drop {
                player.add_exhaustion(MINE_BLOCK_EXHAUSTION);
            }
            player.award_block_break_stats(tool, block, block_drop);
        }

        self.sync_block_state_to_client(&world, location);

        self.update_sequence(player_action.sequence.0);
    }
}

/// Starts mining a block that takes more than one tick: the stage and speed the tick uses.
fn start_mining(player: &Player, entity: &Entity, world: &World, position: BlockPos, speed: f32) {
    player.mining.store(true, Ordering::Relaxed);
    *player
        .mining_pos
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = position;
    let progress = (speed * 10.0) as i32;
    player
        .current_block_breaking_speed
        .store(speed.to_bits(), Ordering::Relaxed);
    world.set_block_breaking(
        entity,
        position,
        BlockBreakingProgress::Start {
            stage: progress,
            speed,
        },
    );
    player
        .current_block_destroy_stage
        .store(progress, Ordering::Relaxed);
}
