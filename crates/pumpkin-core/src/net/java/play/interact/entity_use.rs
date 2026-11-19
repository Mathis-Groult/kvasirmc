//! The attack and interact actions of the interact packet, once the target was found, is in
//! reach and the plugin event passed.

#[allow(clippy::wildcard_imports)]
use super::super::*;

impl JavaClient {
    /// `Attack`: refuses with `PvP` off, kicks for attacking oneself, spares dead and (with
    /// `protect_creative`) creative players, otherwise attacks.
    pub(super) fn attack_entity(
        &self,
        player: &Arc<Player>,
        server: &Arc<Server>,
        world: &Arc<World>,
        entity_id: VarInt,
        player_target: Option<&Arc<Player>>,
        target: &Arc<dyn EntityBase>,
    ) {
        let config = &server.advanced_config.pvp;
        if !config.enabled {
            return;
        }

        if entity_id.0 == player.entity_id() {
            self.try_kick(&TextComponent::translate_cross(
                translation::java::MULTIPLAYER_DISCONNECT_INVALID_ENTITY_ATTACKED,
                translation::java::MULTIPLAYER_DISCONNECT_INVALID_ENTITY_ATTACKED,
                [],
            ));
            return;
        }

        if let Some(player_victim) = player_target {
            if player_victim.living_entity.health.load() <= 0.0 {
                return;
            }
            if config.protect_creative && player_victim.gamemode.load() == GameMode::Creative {
                world.play_sound(
                    Sound::EntityPlayerAttackNodamage,
                    SoundCategory::Players,
                    &player_victim.position(),
                );
                return;
            }
        }
        player.attack(target);
    }

    /// `Interact` / `InteractAt`: the target's own use of the hand's stack (or, when it has
    /// none, of the held stack), else the item's use on the entity, then the advancement
    /// trigger, statistics and the hand's new stack.
    pub(super) fn interact_entity(
        player: &Arc<Player>,
        server: &Arc<Server>,
        interact: &SInteract,
        action: ActionType,
        target: &Arc<dyn EntityBase>,
    ) {
        let entity_id = interact.entity_id;
        if action == ActionType::InteractAt
            && let Some(pos) = interact.target_position
        {
            let mut at_event = crate::plugin::api::events::player::player_interact_at_entity::PlayerInteractAtEntityEvent::new(
                player.clone(),
                entity_id.0,
                pos.x,
                pos.y,
                pos.z,
                u8::from(interact.hand.map_or(0, |h| h.0) != 0),
            );
            server.plugin_manager.fire_blocking(server, &mut at_event);
            if at_event.cancelled {
                return;
            }
        }
        let packet_hand = interact.hand.map_or(0, |hand| hand.0);
        let Ok(mut hand) = pumpkin_util::Hand::from_packet_id(packet_hand) else {
            return;
        };
        let mut stack = player.inventory().get_stack_in_hand(hand);
        let mut before = stack.clone();
        let native_interaction = target.interact_with_hand(player, &mut stack, hand);
        let interacted = native_interaction.unwrap_or_else(|| {
            hand = pumpkin_util::Hand::Right;
            stack = player.inventory().held_item();
            before = stack.clone();
            target.interact(player, &mut stack)
        });
        let item_id = before.item.id;
        if !interacted {
            server
                .item_registry
                .use_on_entity(&mut stack, player, target.clone());
        }
        if interacted || !stack.are_equal(&before) {
            let is_baby = target.get_entity().age.load(Ordering::Relaxed) < 0;
            let player_wearing_safe_armor =
                crate::entity::mob::piglin_ai::PiglinAi::is_wearing_safe_armor(
                    &player.living_entity,
                );
            player.trigger_advancement(
                crate::entity::player::advancement::trigger::AdvancementTrigger::PlayerInteractedWithEntity {
                    entity_type: format!(
                        "minecraft:{}",
                        target.get_entity().entity_type.resource_name
                    ),
                    item_id: before.item.registry_key.to_string(),
                    is_baby,
                    player_wearing_safe_armor,
                },
            );
        }
        if native_interaction.is_none() && !stack.are_equal(&before) {
            player.increment_stat(StatisticCategory::Used, item_id as i32, 1);
            if before.is_damageable() && stack.is_empty() {
                player.increment_stat(StatisticCategory::Broken, item_id as i32, 1);
                player.world().send_entity_status(
                    player.get_entity(),
                    equipment_break_status(&EquipmentSlot::MAIN_HAND),
                    ActorEventID::None,
                );
            }
        }
        player.inventory().set_stack_in_hand(hand, stack);
    }
}
