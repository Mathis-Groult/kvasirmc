#[allow(clippy::wildcard_imports)]
use super::*;

mod digging;

impl BedrockClient {
    #[expect(clippy::match_same_arms)]
    pub fn handle_player_action(
        &self,
        player: &Arc<Player>,
        server: &Arc<Server>,
        packet: &SPlayerAction,
    ) {
        if !player.has_client_loaded()
            || ((player.living_entity.dead.load(Ordering::Relaxed)
                || player.living_entity.health.load() <= 0.0)
                && !matches!(packet.action, PlayerAction::Respawn))
        {
            return;
        }
        player.update_last_action_time();

        match packet.action {
            PlayerAction::StartDestroyBlock
            | PlayerAction::CreativeDestroyBlock
            | PlayerAction::ContinueDestroyBlock => {
                Self::destroy_block(player, server, packet.block_position);
            }
            action @ (PlayerAction::PredictDestroyBlock | PlayerAction::StopDestroyBlock) => {
                self.predict_destroy_block(player, server, packet.block_position, action);
            }
            PlayerAction::CrackBlock => {
                // Don't do anything for this action. It is no longer used. Block
                // cracking is done fully server-side.
            }
            PlayerAction::AbortDestroyBlock => {
                player.stop_mining();
            }
            PlayerAction::DropItem => {
                player.drop_held_item(false);
            }
            PlayerAction::StartGliding => {
                player.try_start_fall_flying(server);
            }
            PlayerAction::StopGliding => {
                player.stop_fall_flying(server);
            }
            PlayerAction::StartSwimming => {
                player.get_entity().set_swimming(true);
                player.update_player_pose();
            }
            PlayerAction::StopSwimming => {
                player.get_entity().set_swimming(false);
                player.update_player_pose();
            }
            PlayerAction::StartSpinAttack => {
                player
                    .get_entity()
                    .set_pose(pumpkin_data::entity::EntityPose::SpinAttack);
            }
            PlayerAction::StopSpinAttack => {
                player.update_player_pose();
            }
            PlayerAction::StartSprinting => {
                player.set_sprinting(true);
            }
            PlayerAction::StopSprinting => {
                player.set_sprinting(false);
            }
            PlayerAction::StartSneaking => {
                player.get_entity().set_sneaking(true);
                player.update_bedrock_shield_sneaking();
            }
            PlayerAction::StopSneaking => {
                player.get_entity().set_sneaking(false);
                player.update_bedrock_shield_sneaking();
            }
            PlayerAction::StartCrawling => {
                player
                    .get_entity()
                    .set_pose(pumpkin_data::entity::EntityPose::Swimming);
            }
            PlayerAction::StopCrawling => {
                player.update_player_pose();
            }
            PlayerAction::StopSleeping => {
                player.wake_up();
            }
            PlayerAction::Respawn
                if player.living_entity.dead.load(Ordering::Relaxed)
                    || player.living_entity.health.load() <= 0.0 =>
            {
                let player_c = player.clone();
                player.spawn_task(async move {
                    player_c.world().respawn_player(&player_c, false).await;
                });
            }
            // TODO
            _ => {}
        }
    }
}
