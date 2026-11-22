//! A record of the `CustomStatistic` `Player::get_movement_statistic` returns, made before the
//! function was split (`tests/golden/player_movement_statistic.txt`; rewrite it on purpose with
//! `KVASIR_BLESS=1`).
//!
//! Part 1 seats the player on every `EntityType` (plain, and with flying, swimming and sneaking
//! on, so the vehicle is shown to win over the movement flags); part 2 walks all 256 combinations
//! of eight movement flags over four terrains (air, a ladder, water at the feet, water at the
//! eye); part 3 probes the fall-speed and eye-height thresholds.

use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

use pumpkin_data::entity::EntityType;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::chunk::ChunkData;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::Entity;
use crate::entity::EntityBase;
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::player::Player;
use crate::world::World;

const USER: Identity = Identity {
    name: "Strider",
    uuid: Uuid::from_u128(0x0000_00fd),
    entity_id: 9_811,
};

const FLAG_NAMES: [&str; 8] = [
    "fly", "glide", "swim", "touch", "sneak", "sprint", "ground", "fall",
];

/// Terrains: x of the player, and what the world holds there.
const AIR: f64 = 0.5;
const LADDER: f64 = 32.5;
const SHALLOW: f64 = 64.5;
const DEEP: f64 = 96.5;
const TERRAINS: [(&str, f64); 4] = [
    ("air", AIR),
    ("ladder", LADDER),
    ("shallow", SHALLOW),
    ("deep", DEEP),
];

fn put(world: &Arc<World>, x: f64, y: i32, id: BlockStateId) {
    world.set_block_state(
        &BlockPos::new(x.floor() as i32, y, 0),
        id,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

fn build_terrain(world: &Arc<World>) {
    for chunk_x in [0, 2, 4, 6] {
        world
            .level
            .loaded_chunks
            .insert(Vector2::new(chunk_x, 0), ChunkData::empty_sync(chunk_x, 0));
    }
    put(world, LADDER, 64, Block::LADDER.default_state.id);
    put(world, SHALLOW, 64, Block::WATER.default_state.id);
    put(world, DEEP, 64, Block::WATER.default_state.id);
    put(world, DEEP, 65, Block::WATER.default_state.id);
}

fn place(player: &Player, x: f64, y: f64) {
    let entity = player.get_entity();
    let pos = Vector3::new(x, y, 0.5);
    entity.pos.store(pos);
    entity
        .block_pos
        .store(BlockPos::new(x.floor() as i32, y.floor() as i32, 0));
}

fn reset(player: &Player) {
    let entity = player.get_entity();
    entity.fall_flying.store(false, Relaxed);
    entity.swimming.store(false, Relaxed);
    entity.touching_water.store(false, Relaxed);
    entity.sneaking.store(false, Relaxed);
    entity.sprinting.store(false, Relaxed);
    entity.on_ground.store(false, Relaxed);
    entity.velocity.store(Vector3::new(0.0, 0.0, 0.0));
    player
        .abilities
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .flying = false;
    *entity
        .vehicle
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

/// Switches on the flags named by the bits of `mask` (order of `FLAG_NAMES`).
fn apply_flags(player: &Player, mask: usize) {
    let entity = player.get_entity();
    player
        .abilities
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .flying = mask & 1 != 0;
    entity.fall_flying.store(mask & 2 != 0, Relaxed);
    entity.swimming.store(mask & 4 != 0, Relaxed);
    entity.touching_water.store(mask & 8 != 0, Relaxed);
    entity.sneaking.store(mask & 16 != 0, Relaxed);
    entity.sprinting.store(mask & 32 != 0, Relaxed);
    entity.on_ground.store(mask & 64 != 0, Relaxed);
    let fall = if mask & 128 != 0 { -1.0 } else { 0.0 };
    entity.velocity.store(Vector3::new(0.0, fall, 0.0));
}

fn flag_text(mask: usize) -> String {
    let on: Vec<&str> = FLAG_NAMES
        .iter()
        .enumerate()
        .filter(|(bit, _)| mask & (1 << bit) != 0)
        .map(|(_, name)| *name)
        .collect();
    if on.is_empty() {
        "none".to_string()
    } else {
        on.join("+")
    }
}

fn seat(player: &Player, kind: &'static EntityType, id: i32, world: &Arc<World>) {
    let vehicle: Arc<dyn EntityBase> = Arc::new(Entity::from_uuid_with_id(
        id,
        Uuid::from_u128(id as u128),
        world.clone(),
        Vector3::new(AIR, 64.0, 0.5),
        kind,
    ));
    *player
        .get_entity()
        .vehicle
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(vehicle);
}

fn vehicle_lines(player: &Player, world: &Arc<World>, text: &mut String) -> TestResult {
    for (title, mask) in [("plain", 0), ("fly+swim+sneak", 1 | 4 | 16)] {
        writeln!(text, "--- every vehicle type, {title} ---")?;
        for (index, kind) in EntityType::ALL.iter().enumerate() {
            reset(player);
            place(player, AIR, 64.0);
            apply_flags(player, mask);
            seat(player, kind, index as i32 + 1, world);
            writeln!(
                text,
                "{} {:?}",
                kind.resource_name,
                player.get_movement_statistic()
            )?;
        }
    }
    Ok(())
}

fn flag_lines(player: &Player, text: &mut String) -> TestResult {
    for (name, x) in TERRAINS {
        writeln!(text, "--- flags over {name} ---")?;
        for mask in 0..256 {
            reset(player);
            place(player, x, 64.0);
            apply_flags(player, mask);
            writeln!(
                text,
                "{name} {} {:?}",
                flag_text(mask),
                player.get_movement_statistic()
            )?;
        }
    }
    Ok(())
}

/// Fall speed around the -0.005 threshold (airborne and grounded) and eye height around the top
/// of a water source block, with the touching-water flag on.
fn threshold_lines(player: &Player, text: &mut String) -> TestResult {
    writeln!(text, "--- fall speed ---")?;
    for vy in [
        0.1, 0.0, -0.004, -0.0049, -0.005, -0.0051, -0.006, -0.5, -3.0,
    ] {
        for ground in [false, true] {
            reset(player);
            place(player, AIR, 64.0);
            let entity = player.get_entity();
            entity.on_ground.store(ground, Relaxed);
            entity.velocity.store(Vector3::new(0.0, vy, 0.0));
            writeln!(
                text,
                "vy {vy:?} ground {ground} {:?}",
                player.get_movement_statistic()
            )?;
        }
    }
    writeln!(text, "--- eye height over water at y 65 ---")?;
    for y in [64.0, 64.2, 64.26, 64.27, 64.28, 64.3, 64.6, 65.0] {
        reset(player);
        place(player, DEEP, y);
        player.get_entity().touching_water.store(true, Relaxed);
        writeln!(text, "y {y:?} {:?}", player.get_movement_statistic())?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn movement_statistic_every_state() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load().first().cloned().ok_or("no world")?;
    build_terrain(&world);
    let (player, _tap) = test_support::java_player_as(&world, GameMode::Survival, &USER)
        .ok_or("no Java packet tap")?;
    let mut text = String::new();
    vehicle_lines(&player, &world, &mut text)?;
    flag_lines(&player, &mut text)?;
    threshold_lines(&player, &mut text)?;
    server.stop();
    script_golden::bless_or_compare("player_movement_statistic.txt", &text)
}
