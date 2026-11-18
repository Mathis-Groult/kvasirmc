//! A record of what `World::on_block_state_set` does (reached through `set_block_state`), made
//! before the function was split (`tests/golden/block_state_set.txt`; rewrite it on purpose with
//! `KVASIR_BLESS=1`).
//!
//! Each scene lays an old block and its neighbours in a loaded empty chunk (written straight,
//! without callbacks or queued changes), then calls `set_block_state` once with the scene's new
//! state and flags. One line per scene: the returned state, the state at the position and at
//! its six neighbours, the block entity there, items and entities that appeared, the queued
//! client changes, scheduled block and fluid ticks, villager job site and POI entries at the
//! position, and its block light.

use std::fmt::Write;
use std::sync::Arc;

use pumpkin_data::block_properties::{
    ChestLikeProperties, ChestType, ComposterLikeProperties, HorizontalFacing,
};
use pumpkin_data::fluid::Fluid;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_inventory::Inventory;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_world::world::BlockFlags;

use crate::block::entities::chest::ChestBlockEntity;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;
use crate::world::World;

type Setup = fn(&Arc<World>, BlockPos);

struct Scene {
    name: &'static str,
    setup: Setup,
    new_state: fn() -> BlockStateId,
    flags: BlockFlags,
}

const ALL: BlockFlags = BlockFlags::NOTIFY_ALL;

/// Writes a state with no callbacks, no neighbour updates and no queued client change.
fn put(world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
    world.set_block_state(
        &pos,
        state,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

fn floor(world: &Arc<World>, pos: BlockPos) {
    put(world, pos.down(), Block::STONE.default_state.id);
}

fn on_air(world: &Arc<World>, pos: BlockPos) {
    floor(world, pos);
}

fn on_stone(world: &Arc<World>, pos: BlockPos) {
    floor(world, pos);
    put(world, pos, Block::STONE.default_state.id);
}

fn chest_state(facing: HorizontalFacing) -> BlockStateId {
    let mut chest = ChestLikeProperties::default(&Block::CHEST);
    chest.facing = facing;
    chest.r#type = ChestType::Single;
    chest.to_state_id(&Block::CHEST)
}

fn on_chest(world: &Arc<World>, pos: BlockPos) {
    floor(world, pos);
    put(world, pos, chest_state(HorizontalFacing::North));
    let chest = Arc::new(ChestBlockEntity::new(pos));
    chest.set_stack(0, ItemStack::new(4, &Item::APPLE));
    world.add_block_entity(chest);
}

fn beside_fence(world: &Arc<World>, pos: BlockPos) {
    floor(world, pos);
    put(world, pos.west().down(), Block::STONE.default_state.id);
    put(world, pos.west(), Block::OAK_FENCE.default_state.id);
}

fn beside_dust(world: &Arc<World>, pos: BlockPos) {
    floor(world, pos);
    put(world, pos.west().down(), Block::STONE.default_state.id);
    put(world, pos.west(), Block::REDSTONE_WIRE.default_state.id);
}

fn composter(level: u8) -> BlockStateId {
    ComposterLikeProperties { level }.to_state_id(&Block::COMPOSTER)
}

fn composter_between_comparators(world: &Arc<World>, pos: BlockPos) {
    floor(world, pos);
    put(world, pos, composter(0));
    for side in [pos.east(), pos.west()] {
        put(world, side.down(), Block::STONE.default_state.id);
        put(world, side, Block::COMPARATOR.default_state.id);
    }
}

/// Placed through `set_block_state` with every callback, so its job site and POI exist.
fn on_lectern(world: &Arc<World>, pos: BlockPos) {
    floor(world, pos);
    world.set_block_state(&pos, Block::LECTERN.default_state.id, ALL);
}

fn on_glowstone(world: &Arc<World>, pos: BlockPos) {
    floor(world, pos);
    world.set_block_state(&pos, Block::GLOWSTONE.default_state.id, ALL);
}

/// Air below: placed sand falls.
fn hanging_air(world: &Arc<World>, pos: BlockPos) {
    put(world, pos.down().down(), Block::STONE.default_state.id);
}

fn beside_redstone_block(world: &Arc<World>, pos: BlockPos) {
    floor(world, pos);
    put(world, pos.east(), Block::REDSTONE_BLOCK.default_state.id);
}

fn stone() -> BlockStateId {
    Block::STONE.default_state.id
}

fn air() -> BlockStateId {
    Block::AIR.default_state.id
}

fn chest_east() -> BlockStateId {
    chest_state(HorizontalFacing::East)
}

fn trapped_chest() -> BlockStateId {
    Block::TRAPPED_CHEST.default_state.id
}

fn barrel() -> BlockStateId {
    Block::BARREL.default_state.id
}

fn redstone_block() -> BlockStateId {
    Block::REDSTONE_BLOCK.default_state.id
}

fn composter_three() -> BlockStateId {
    composter(3)
}

fn lectern() -> BlockStateId {
    Block::LECTERN.default_state.id
}

fn composter_empty() -> BlockStateId {
    composter(0)
}

fn glowstone() -> BlockStateId {
    Block::GLOWSTONE.default_state.id
}

fn sand() -> BlockStateId {
    Block::SAND.default_state.id
}

fn water() -> BlockStateId {
    Block::WATER.default_state.id
}

fn tnt() -> BlockStateId {
    Block::TNT.default_state.id
}

const fn scene(
    name: &'static str,
    setup: Setup,
    new_state: fn() -> BlockStateId,
    flags: BlockFlags,
) -> Scene {
    Scene {
        name,
        setup,
        new_state,
        flags,
    }
}

const fn with(flags: BlockFlags, more: BlockFlags) -> BlockFlags {
    flags.union(more)
}

fn scenes() -> Vec<Scene> {
    let mut scenes = block_scenes();
    scenes.extend(neighbour_scenes());
    scenes
}

fn block_scenes() -> Vec<Scene> {
    vec![
        scene("stone on air", on_air, stone, ALL),
        scene("air on stone", on_stone, air, ALL),
        scene("stone on stone", on_stone, stone, ALL),
        scene(
            "stone on stone, forced",
            on_stone,
            stone,
            with(ALL, BlockFlags::FORCE_STATE),
        ),
        scene("stone on a chest", on_chest, stone, ALL),
        scene(
            "stone on a chest, no replaced callback",
            on_chest,
            stone,
            with(ALL, BlockFlags::SKIP_BLOCK_ENTITY_REPLACED_CALLBACK),
        ),
        scene("chest turned east", on_chest, chest_east, ALL),
        scene("trapped chest on a chest", on_chest, trapped_chest, ALL),
        scene("barrel on a chest", on_chest, barrel, ALL),
        scene("lectern on air", on_air, lectern, ALL),
        scene("stone on a lectern", on_lectern, stone, ALL),
        scene("composter on a lectern", on_lectern, composter_empty, ALL),
        scene(
            "stone on a lectern, listeners only",
            on_lectern,
            stone,
            BlockFlags::NOTIFY_LISTENERS,
        ),
        scene("glowstone on air", on_air, glowstone, ALL),
        scene("air on glowstone", on_glowstone, air, ALL),
        scene("sand over air", hanging_air, sand, ALL),
        scene(
            "sand over air, no added callback",
            hanging_air,
            sand,
            with(ALL, BlockFlags::SKIP_BLOCK_ADDED_CALLBACK),
        ),
        scene(
            "sand over air, moved",
            hanging_air,
            sand,
            with(ALL, BlockFlags::MOVED),
        ),
        scene("water on air", on_air, water, ALL),
        scene(
            "water on air, no added callback",
            on_air,
            water,
            with(ALL, BlockFlags::SKIP_BLOCK_ADDED_CALLBACK),
        ),
    ]
}

fn neighbour_scenes() -> Vec<Scene> {
    vec![
        scene("stone beside a fence", beside_fence, stone, ALL),
        scene(
            "stone beside a fence, known shape",
            beside_fence,
            stone,
            with(ALL, BlockFlags::UPDATE_KNOWN_SHAPE),
        ),
        scene(
            "stone beside a fence, moved",
            beside_fence,
            stone,
            with(ALL, BlockFlags::MOVED),
        ),
        scene(
            "stone beside a fence, listeners only",
            beside_fence,
            stone,
            BlockFlags::NOTIFY_LISTENERS,
        ),
        scene(
            "stone beside a fence, no flags",
            beside_fence,
            stone,
            BlockFlags::empty(),
        ),
        scene(
            "redstone block beside dust",
            beside_dust,
            redstone_block,
            ALL,
        ),
        scene(
            "redstone block beside dust, listeners only",
            beside_dust,
            redstone_block,
            BlockFlags::NOTIFY_LISTENERS,
        ),
        scene(
            "composter filled between comparators",
            composter_between_comparators,
            composter_three,
            ALL,
        ),
        scene(
            "composter filled between comparators, listeners only",
            composter_between_comparators,
            composter_three,
            BlockFlags::NOTIFY_LISTENERS,
        ),
        scene(
            "tnt beside a redstone block",
            beside_redstone_block,
            tnt,
            ALL,
        ),
        scene(
            "tnt beside a redstone block, no added callback",
            beside_redstone_block,
            tnt,
            with(ALL, BlockFlags::SKIP_BLOCK_ADDED_CALLBACK),
        ),
    ]
}

fn state_text(world: &World, pos: &BlockPos) -> String {
    let state = world.get_block_state_id(pos);
    format!("{}#{}", Block::from_state_id(state).name, state.as_u16())
}

/// Items (name x count) and other entities (type) within two blocks of `pos`, sorted.
fn entities_near(world: &World, pos: BlockPos) -> Vec<String> {
    let mut found: Vec<String> = world
        .entities
        .load()
        .iter()
        .filter_map(|entity| {
            let base = entity.get_entity();
            let at = base.pos.load();
            let near = (at.x - f64::from(pos.0.x) - 0.5).abs() < 2.0
                && (at.y - f64::from(pos.0.y)).abs() < 3.0
                && (at.z - f64::from(pos.0.z) - 0.5).abs() < 2.0;
            if !near {
                return None;
            }
            Some(entity.get_item_entity().map_or_else(
                || base.entity_type.resource_name.to_string(),
                |item| {
                    let stack = item
                        .get_item_stack()
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone();
                    format!("{}x{}", stack.item.registry_key, stack.item_count)
                },
            ))
        })
        .collect();
    found.sort();
    found
}

/// The client changes queued so far, as offsets from `pos`, sorted; the queue is emptied.
fn queued_changes(world: &World, pos: BlockPos) -> Vec<String> {
    let changes = std::mem::take(
        &mut *world
            .unsent_block_changes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    let mut offsets: Vec<String> = changes
        .iter()
        .map(|(at, state)| {
            format!(
                "{},{},{}#{}",
                at.0.x - pos.0.x,
                at.0.y - pos.0.y,
                at.0.z - pos.0.z,
                state.as_u16()
            )
        })
        .collect();
    offsets.sort();
    offsets
}

fn around(pos: BlockPos) -> [BlockPos; 7] {
    [
        pos,
        pos.down(),
        pos.up(),
        pos.north(),
        pos.south(),
        pos.west(),
        pos.east(),
    ]
}

/// Indexes into [`around`] whose block or fluid has a scheduled tick.
fn ticks(world: &World, pos: BlockPos) -> String {
    let mut text = String::new();
    for (index, at) in around(pos).iter().enumerate() {
        if world.is_block_tick_scheduled(at, world.get_block(at)) {
            let _ = write!(text, "b{index}");
        }
        for fluid in [&Fluid::WATER, &Fluid::FLOWING_WATER] {
            if world.is_fluid_tick_scheduled(at, fluid) {
                let _ = write!(text, "f{index}:{}", fluid.name);
            }
        }
    }
    text
}

fn record(out: &mut String, world: &World, scene: &Scene, pos: BlockPos, returned: BlockStateId) {
    let states: Vec<String> = around(pos).iter().map(|at| state_text(world, at)).collect();
    let entity = world.get_block_entity(&pos).map(|entity| entity.get_id());
    let job_site = world
        .villager_poi
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get_nearest_job_site(pos, 0)
        .is_some();
    let poi = world
        .portal_poi
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get_in_square(pos, 0, None)
        .iter()
        .filter(|at| **at == pos)
        .count();
    let _ = writeln!(
        out,
        "{}: returned {}#{} states {states:?} block entity {entity:?} entities {:?} queued {:?} ticks [{}] job site {job_site} poi {poi} light {:?}",
        scene.name,
        Block::from_state_id(returned).name,
        returned.as_u16(),
        entities_near(world, pos),
        queued_changes(world, pos),
        ticks(world, pos),
        world.get_block_light_level(&pos),
    );
}

const fn position_of(index: usize) -> BlockPos {
    BlockPos::new(index as i32 * 16 + 3008, 64, 8)
}

#[tokio::test(flavor = "multi_thread")]
async fn block_state_set_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let scenes = scenes();
    for index in 0..scenes.len() {
        let chunk = position_of(index).chunk_position();
        for dx in -1..=1 {
            for dz in -1..=1 {
                let at = Vector2::new(chunk.x + dx, chunk.y + dz);
                world
                    .level
                    .loaded_chunks
                    .insert(at, pumpkin_world::chunk::ChunkData::empty_sync(at.x, at.y));
            }
        }
    }
    let mut out = String::new();
    for (index, scene) in scenes.iter().enumerate() {
        let pos = position_of(index);
        (scene.setup)(&world, pos);
        drop(queued_changes(&world, pos));
        let returned = world.set_block_state(&pos, (scene.new_state)(), scene.flags);
        record(&mut out, &world, scene, pos, returned);
    }
    server.stop();
    script_golden::bless_or_compare("block_state_set.txt", &out)
}
