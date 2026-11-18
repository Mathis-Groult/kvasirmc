//! Strength and Weakness reach a player's melee damage once, through the `ATTACK_DAMAGE`
//! attribute their effects modify (vanilla 26.3 `Player.attack` reads only
//! `getAttributeValue(ATTACK_DAMAGE)`; card #82), and a hit that would deal no damage does not
//! land (vanilla hits only when the scaled base damage or the enchantment bonus is above zero;
//! card #81).

use std::sync::Arc;
use std::sync::atomic::Ordering;

use pumpkin_data::data_component_impl::EquipmentSlot;
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::potion::Effect;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::{GameMode, Hand};

use crate::entity::ai::test_support;
use crate::entity::living::LivingEntity;
use crate::entity::{Entity, EntityBase};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// What a fully charged hit on the ground did to a fresh zombie: its health, its hurt
/// cooldown, and the attacker's exhaustion.
async fn hit(
    held: Option<&'static Item>,
    effect: &'static StatusEffect,
    amplifier: u8,
) -> Result<(f32, i32, f32), Box<dyn std::error::Error>> {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    for dx in -1..=1 {
        for dz in -1..=1 {
            let at = Vector2::new(dx, dz);
            world
                .level
                .loaded_chunks
                .insert(at, pumpkin_world::chunk::ChunkData::empty_sync(at.x, at.y));
        }
    }
    let attacker = test_support::java_player(&world, GameMode::Survival);
    attacker.get_entity().set_pos(Vector3::new(8.5, 64.0, 8.5));
    attacker
        .get_entity()
        .on_ground
        .store(true, Ordering::Relaxed);
    attacker.last_attacked_ticks.store(100, Ordering::Relaxed);
    if let Some(item) = held {
        let stack = ItemStack::new(1, item);
        attacker
            .living_entity
            .apply_and_send_equipment_attribute_modifiers(&[(
                EquipmentSlot::MAIN_HAND,
                stack.clone(),
            )]);
        attacker.inventory().set_stack_in_hand(Hand::Right, stack);
    }
    attacker.living_entity.add_effect(Effect {
        effect_type: effect,
        duration: 2000,
        amplifier,
        ambient: false,
        show_particles: false,
        show_icon: false,
        blend: false,
    });
    let zombie: Arc<dyn EntityBase> = Arc::new(LivingEntity::new(Entity::new(
        world.clone(),
        Vector3::new(8.5, 64.0, 10.5),
        &EntityType::ZOMBIE,
    )));
    world.spawn_entity(zombie.clone());
    attacker.attack(&zombie);
    let living = zombie
        .get_living_entity()
        .ok_or("the zombie is not living")?;
    let outcome = (
        living.health.load(),
        living.hurt_cooldown.load(Ordering::Relaxed),
        attacker.hunger_manager.exhaustion.load(),
    );
    server.stop();
    Ok(outcome)
}

/// The damage a fully charged hit on the ground deals to a fresh zombie.
async fn hit_damage(
    held: Option<&'static Item>,
    effect: &'static StatusEffect,
    amplifier: u8,
) -> Result<f32, Box<dyn std::error::Error>> {
    Ok(20.0 - hit(held, effect, amplifier).await?.0)
}

#[tokio::test(flavor = "multi_thread")]
async fn strength_two_adds_six_to_a_fist() -> TestResult {
    assert!((hit_damage(None, &StatusEffect::STRENGTH, 1).await? - 7.0).abs() < 1e-4);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn weakness_one_takes_four_from_a_diamond_sword() -> TestResult {
    assert!(
        (hit_damage(Some(&Item::DIAMOND_SWORD), &StatusEffect::WEAKNESS, 0).await? - 3.0).abs()
            < 1e-4
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fist_with_weakness_does_not_land() -> TestResult {
    let (health, hurt_cooldown, exhaustion) = hit(None, &StatusEffect::WEAKNESS, 0).await?;
    assert!((health - 20.0).abs() < 1e-4);
    assert_eq!(hurt_cooldown, 0);
    assert!(exhaustion.abs() < 1e-6);
    Ok(())
}
