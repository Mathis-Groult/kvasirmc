//! A record of every field `Entity::from_uuid_with_id` sets, made before the function was split
//! (`tests/golden/entity_from_uuid_with_id.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Part 1 builds a few entity types at positions with fractional, negative and chunk-border
//! coordinates and prints every field; part 2 builds every `EntityType` at one position and prints
//! the type-dependent fields (dimensions, bounding box, index membership).

use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

use pumpkin_data::entity::EntityType;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::chunk::ChunkData;
use uuid::Uuid;

use crate::entity::Entity;
use crate::entity::ai::test_support;
use crate::world::World;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const POSITIONS: &[(f64, f64, f64)] = &[
    (0.5, 64.0, 0.5),
    (-0.5, -10.25, -17.75),
    (15.999, 0.0, 16.0),
    (-16.0, -64.0, -0.0001),
    (-16.5, 319.9, 31.5),
    (1000.7, 100.5, -2000.3),
    (-1e-9, 1e-9, 47.999),
    (123.0, -1.0, -33.0),
];

/// Chunk of the position at index 5 (1000.7, -2000.3): never loaded.
const UNLOADED: (i32, i32) = (62, -126);

const KINDS: &[&EntityType] = &[
    &EntityType::PLAYER,
    &EntityType::ZOMBIE,
    &EntityType::ITEM,
    &EntityType::ENDER_DRAGON,
    &EntityType::SLIME,
];

/// Loads the chunk around each position with a biome that depends on all three quart
/// coordinates and the chunk, so a swapped or wrong coordinate in the biome lookup shows. The
/// chunk of position 5 stays unloaded (the lookup then falls back to biome 0).
fn load_biomes(world: &Arc<World>) {
    for &(x, _, z) in POSITIONS {
        let cx = (x.floor() as i32) >> 4;
        let cz = (z.floor() as i32) >> 4;
        if cx == UNLOADED.0 && cz == UNLOADED.1 {
            continue;
        }
        let chunk = ChunkData::empty_sync(cx, cz);
        let quarts = chunk
            .section
            .biome_sections
            .read()
            .map_or(0, |sections| sections.len())
            * 4;
        for qx in 0..4 {
            for qy in 0..quarts {
                for qz in 0..4 {
                    let id = (qx + 2 * qy + 3 * qz) as i32 + 5 * cx + 7 * cz;
                    chunk
                        .section
                        .set_relative_biome(qx, qy, qz, id.rem_euclid(50) as u8);
                }
            }
        }
        world
            .level
            .loaded_chunks
            .insert(Vector2::new(cx, cz), chunk);
    }
}

fn flags(entity: &Entity) -> String {
    let bools = [
        ("on_ground", &entity.on_ground),
        ("touching_water", &entity.touching_water),
        ("touching_lava", &entity.touching_lava),
        ("horizontal_collision", &entity.horizontal_collision),
        ("vertical_collision", &entity.vertical_collision),
        ("sneaking", &entity.sneaking),
        ("sprinting", &entity.sprinting),
        ("swimming", &entity.swimming),
        ("invisible", &entity.invisible),
        ("glowing", &entity.glowing),
        ("has_glowing_tag", &entity.has_glowing_tag),
        ("glowing_from_effect", &entity.glowing_from_effect),
        ("fall_flying", &entity.fall_flying),
        ("invulnerable", &entity.invulnerable),
        ("fire_immune", &entity.fire_immune),
        ("has_visual_fire", &entity.has_visual_fire),
        ("is_in_powder_snow", &entity.is_in_powder_snow),
        ("was_in_powder_snow", &entity.was_in_powder_snow),
        ("custom_name_visible", &entity.custom_name_visible),
        ("silent", &entity.silent),
        ("has_no_gravity", &entity.has_no_gravity),
        ("no_physics", &entity.no_physics),
        ("first_tick", &entity.first_tick),
        ("velocity_dirty", &entity.velocity_dirty),
        ("removed", &entity.removed),
    ];
    let mut text = String::new();
    for (name, value) in bools {
        let _ = write!(text, " {name}={}", u8::from(value.load(Relaxed)));
    }
    text
}

fn counters(entity: &Entity) -> String {
    format!(
        "fire_ticks={} frozen_ticks={} riding_cooldown={} tick_count={} age={} air_supply={} \
         portal_cooldown={} data={} flags={} bedrock_flags={} bedrock_flags_two={} \
         last_crystal_sound_play_tick={} last_sent_yaw={} last_sent_pitch={} last_sent_head_yaw={}",
        entity.fire_ticks.load(Relaxed),
        entity.frozen_ticks.load(Relaxed),
        entity.riding_cooldown.load(Relaxed),
        entity.tick_count.load(Relaxed),
        entity.age.load(Relaxed),
        entity.air_supply.load(Relaxed),
        entity.portal_cooldown.load(Relaxed),
        entity.data.load(Relaxed),
        entity.flags.load(Relaxed),
        entity.bedrock_flags.load(Relaxed),
        entity.bedrock_flags_two.load(Relaxed),
        entity.last_crystal_sound_play_tick.load(Relaxed),
        entity.last_sent_yaw.load(Relaxed),
        entity.last_sent_pitch.load(Relaxed),
        entity.last_sent_head_yaw.load(Relaxed),
    )
}

fn floats(entity: &Entity) -> String {
    format!(
        "water_height={:?} lava_height={:?} yaw={:?} head_yaw={:?} body_yaw={:?} pitch={:?} \
         move_dist={:?} fly_dist={:?} next_step={:?} crystal={:?}",
        entity.water_height.load(),
        entity.lava_height.load(),
        entity.yaw.load(),
        entity.head_yaw.load(),
        entity.body_yaw.load(),
        entity.pitch.load(),
        entity.move_dist.load(),
        entity.fly_dist.load(),
        entity.next_step.load(),
        entity.crystal_sound_intensity.load(),
    )
}

fn vectors(entity: &Entity) -> String {
    format!(
        "pos={:?} last_pos={:?} movement={:?} velocity={:?} multiplier={:?} last_sent_pos={:?} \
         last_sent_velocity={:?}",
        entity.pos.load(),
        entity.last_pos.load(),
        entity.movement.load(),
        entity.velocity.load(),
        entity.movement_multiplier.load(),
        entity.last_sent_pos.load(),
        entity.last_sent_velocity.load(),
    )
}

fn places(entity: &Entity) -> String {
    format!(
        "block_pos={:?} supporting={:?} chunk_pos={:?} last_biome_update_pos={:?} biome={}",
        entity.block_pos.load(),
        entity.supporting_block_pos.load(),
        entity.chunk_pos.load(),
        entity.last_biome_update_pos.load(),
        entity.current_biome.load().registry_id,
    )
}

fn containers(entity: &Entity) -> String {
    let lock = |what: &str, empty: bool| format!("{what}={}", u8::from(empty));
    let parts = [
        lock(
            "passengers_empty",
            entity.passengers.lock().unwrap().is_empty(),
        ),
        lock("vehicle_none", entity.vehicle.lock().unwrap().is_none()),
        lock("leashed_none", entity.leashed_to.lock().unwrap().is_none()),
        lock(
            "delayed_leash_none",
            entity.delayed_leash.lock().unwrap().is_none(),
        ),
        lock(
            "portal_none",
            entity.portal_manager.lock().unwrap().is_none(),
        ),
        lock(
            "immunities_empty",
            entity.damage_immunities.lock().unwrap().is_empty(),
        ),
        lock(
            "tags_empty",
            entity.scoreboard_tags.lock().unwrap().is_empty(),
        ),
        lock(
            "custom_data_empty",
            entity.custom_data.lock().unwrap().child_tags.is_empty(),
        ),
        lock("custom_name_none", entity.custom_name.load().is_none()),
        lock("removal_none", entity.removal_reason.load().is_none()),
        lock("synched_clean", !entity.synched_data.is_dirty()),
        lock(
            "default_dims_none",
            entity.default_dimensions.load().is_none(),
        ),
    ];
    format!(
        "{} blocks_building={}",
        parts.join(" "),
        entity.blocks_building
    )
}

fn boxed(entity: &Entity, world: &Arc<World>) -> String {
    let bounds = entity.bounding_box.load();
    let index = entity
        .bounding_box
        .index
        .read()
        .map(|weak| weak.upgrade())
        .ok()
        .flatten();
    format!(
        "bounds={bounds:?} indexed={} index_id={} index_is_world's={} dims={:?} pose={}",
        entity.bounding_box.indexed,
        entity.bounding_box.entity_id,
        index.is_some_and(|index| Arc::ptr_eq(&index, &world.entity_index)),
        entity.entity_dimension.load(),
        entity.pose.load() as i32,
    )
}

fn full(entity: &Entity, world: &Arc<World>) -> String {
    format!(
        "  id={} uuid={} type={} world_same={}\n  {}\n  {}\n  {}\n  {}\n  {}\n  {}\n  {}\n  dims_const={:?}\n",
        entity.entity_id,
        entity.entity_uuid,
        entity.entity_type.resource_name,
        Arc::ptr_eq(&entity.world.load_full(), world),
        vectors(entity),
        places(entity),
        floats(entity),
        counters(entity),
        flags(entity).trim_start(),
        containers(entity),
        boxed(entity, world),
        Entity::type_dimensions(entity.entity_type),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn from_uuid_with_id_sets_every_field() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load().first().cloned().ok_or("no world")?;
    load_biomes(&world);
    let mut now = String::new();
    let ids = [7, 0, -3, i32::MAX, 123_456];
    for (index, &(x, y, z)) in POSITIONS.iter().enumerate() {
        for (k, kind) in KINDS.iter().enumerate() {
            let id = ids[(index + k) % ids.len()];
            let uuid = Uuid::from_u128((index * 10 + k) as u128 + 1);
            let entity =
                Entity::from_uuid_with_id(id, uuid, world.clone(), Vector3::new(x, y, z), kind);
            writeln!(now, "position {index} {x:?} {y:?} {z:?}")?;
            now.push_str(&full(&entity, &world));
        }
    }
    now.push_str("--- every entity type at (0.5, 64, 0.5) ---\n");
    for (index, kind) in EntityType::ALL.iter().enumerate() {
        let entity = Entity::from_uuid_with_id(
            index as i32 + 1,
            Uuid::from_u128(index as u128 + 1),
            world.clone(),
            Vector3::new(0.5, 64.0, 0.5),
            kind,
        );
        writeln!(
            now,
            "{} {} {}",
            kind.resource_name,
            boxed(&entity, &world),
            places(&entity)
        )?;
    }
    server.stop();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/entity_from_uuid_with_id.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &now)?;
    } else {
        assert_eq!(std::fs::read_to_string(&path)?, now);
    }
    Ok(())
}
