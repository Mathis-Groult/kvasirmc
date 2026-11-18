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
    ChestLikeProperties, ChestType, ComparatorLikeProperties, ComposterLikeProperties,
    EastRedstone, HorizontalFacing, RedstoneOreLikeProperties, RedstoneWireLikeProperties,
    WestRedstone,
};
use pumpkin_data::fluid::Fluid;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_inventory::Inventory;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_world::chunk::format::LightContainer;
use pumpkin_world::poi::PoiRegion;
use pumpkin_world::world::BlockFlags;

use crate::block::entities::chest::ChestBlockEntity;
use crate::block::entities::shulker_box::ShulkerBoxBlockEntity;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;
use crate::world::World;

type Setup = fn(&Arc<World>, BlockPos);

struct Scene {
    name: &'static str,
    setup: Setup,
    new_state: fn() -> BlockStateId,
    flags: BlockFlags,
    /// Appended after the first mutation run: the chunks get light sections, and the line also
    /// lists every non-air state and scheduled tick in the box around the position.
    wide: bool,
    /// Appended after the second mutation run: extra text at the end of a wide line.
    probe: Probe,
}

type Probe = fn(&World, BlockPos) -> String;

const fn no_probe(_world: &World, _pos: BlockPos) -> String {
    String::new()
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

fn on_shulker_box(world: &Arc<World>, pos: BlockPos) {
    floor(world, pos);
    put(world, pos, Block::SHULKER_BOX.default_state.id);
    world.add_block_entity(Arc::new(ShulkerBoxBlockEntity::new(pos)));
}

/// Stone with a chest block entity left over at its position.
fn on_stone_with_stray_chest(world: &Arc<World>, pos: BlockPos) {
    on_stone(world, pos);
    world.add_block_entity(Arc::new(ChestBlockEntity::new(pos)));
}

fn dust(power: u8) -> BlockStateId {
    let mut wire = RedstoneWireLikeProperties::default(&Block::REDSTONE_WIRE);
    wire.power = power;
    wire.to_state_id(&Block::REDSTONE_WIRE)
}

fn torch(lit: bool) -> BlockStateId {
    RedstoneOreLikeProperties { lit }.to_state_id(&Block::REDSTONE_TORCH)
}

/// A lit torch under a solid block carrying powered dust: only the torch's own
/// `on_state_replaced` reaches the dust two blocks up.
fn torch_under_powered_dust(world: &Arc<World>, pos: BlockPos) {
    floor(world, pos);
    put(world, pos, torch(true));
    put(world, pos.up(), Block::STONE.default_state.id);
    put(world, pos.up().up(), dust(15));
}

/// Stone carrying a torch, with dust beside the torch on another stone.
fn stone_under_torch_beside_dust(world: &Arc<World>, pos: BlockPos) {
    on_stone(world, pos);
    put(world, pos.up(), Block::TORCH.default_state.id);
    put(world, pos.east(), Block::STONE.default_state.id);
    put(world, pos.east().up(), dust(15));
}

/// Dust on the floor whose east neighbour carries dust one block higher.
fn dust_below_raised_dust(world: &Arc<World>, pos: BlockPos) {
    floor(world, pos);
    put(world, pos, Block::REDSTONE_WIRE.default_state.id);
    put(world, pos.east(), Block::STONE.default_state.id);
    put(
        world,
        pos.east().up(),
        Block::REDSTONE_WIRE.default_state.id,
    );
}

fn air_below_raised_dust(world: &Arc<World>, pos: BlockPos) {
    floor(world, pos);
    put(world, pos.east(), Block::STONE.default_state.id);
    put(
        world,
        pos.east().up(),
        Block::REDSTONE_WIRE.default_state.id,
    );
}

fn comparator(facing: HorizontalFacing) -> BlockStateId {
    let mut gate = ComparatorLikeProperties::default(&Block::COMPARATOR);
    gate.facing = facing;
    gate.to_state_id(&Block::COMPARATOR)
}

/// Comparators two blocks away, behind stone on each side, facing east and west.
fn composter_behind_stone(world: &Arc<World>, pos: BlockPos) {
    floor(world, pos);
    put(world, pos, composter(0));
    for (side, far, facing) in [
        (pos.east(), pos.east().east(), HorizontalFacing::East),
        (pos.west(), pos.west().west(), HorizontalFacing::West),
    ] {
        put(world, side, Block::STONE.default_state.id);
        put(world, far.down(), Block::STONE.default_state.id);
        put(world, far, comparator(facing));
    }
}

/// A nether portal block with its portal POI.
fn on_nether_portal(world: &Arc<World>, pos: BlockPos) {
    floor(world, pos);
    put(world, pos, Block::NETHER_PORTAL.default_state.id);
    world
        .portal_poi
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .add_portal(pos);
}

/// Comparators two blocks away behind stone, each reading through the stone towards `pos`.
fn composter_behind_stone_facing_it(world: &Arc<World>, pos: BlockPos) {
    floor(world, pos);
    put(world, pos, composter(0));
    for (side, far, facing) in [
        (pos.east(), pos.east().east(), HorizontalFacing::West),
        (pos.west(), pos.west().west(), HorizontalFacing::East),
    ] {
        put(world, side, Block::STONE.default_state.id);
        put(world, far.down(), Block::STONE.default_state.id);
        put(world, far, comparator(facing));
    }
}

fn wire(east: EastRedstone, west: WestRedstone) -> BlockStateId {
    let mut wire = RedstoneWireLikeProperties::default(&Block::REDSTONE_WIRE);
    wire.east = east;
    wire.west = west;
    wire.to_state_id(&Block::REDSTONE_WIRE)
}

/// Dust going up the east stone: only `prepare` reaches the raised dust diagonally.
fn dust_up_east() -> BlockStateId {
    wire(EastRedstone::Up, WestRedstone::None)
}

/// Raised dust on the east stone, with unpowered TNT east of it beside a redstone block:
/// only a neighbour update (not a shape update) of the raised dust primes the TNT. The wire's
/// own placed callback would update the raised dust's neighbours too, so the scene skips it.
fn raised_dust_by_tnt(world: &Arc<World>, pos: BlockPos) {
    floor(world, pos);
    put(world, pos.east(), Block::STONE.default_state.id);
    put(
        world,
        pos.east().up(),
        Block::REDSTONE_WIRE.default_state.id,
    );
    put(world, pos.east().east(), Block::STONE.default_state.id);
    put(world, pos.east().east().up(), Block::TNT.default_state.id);
    put(
        world,
        pos.east().east().east().up(),
        Block::REDSTONE_BLOCK.default_state.id,
    );
}

/// Dust going up the east stone to raised dust that connects back down to it.
fn dust_up_to_raised_dust(world: &Arc<World>, pos: BlockPos) {
    floor(world, pos);
    put(world, pos, dust_up_east());
    put(world, pos.east(), Block::STONE.default_state.id);
    put(
        world,
        pos.east().up(),
        wire(EastRedstone::None, WestRedstone::Side),
    );
}

/// Dot dust (no side connected) under raised dust whose west side connects down to it: the old
/// state's own sides, not the new state's, decide which indirect neighbours `prepare` updates.
fn dot_dust_under_raised_dust(world: &Arc<World>, pos: BlockPos) {
    floor(world, pos);
    put(world, pos, Block::REDSTONE_WIRE.default_state.id);
    put(world, pos.east(), Block::STONE.default_state.id);
    put(
        world,
        pos.east().up(),
        wire(EastRedstone::None, WestRedstone::Side),
    );
}

/// The same blocks, the dot dust written LAST: written first, `prepare` of the raised dust
/// reshapes it into a wire going up east (5862), which hides the old state's sides.
fn dot_dust_put_after_raised_dust(world: &Arc<World>, pos: BlockPos) {
    floor(world, pos);
    put(world, pos.east(), Block::STONE.default_state.id);
    put(
        world,
        pos.east().up(),
        wire(EastRedstone::None, WestRedstone::Side),
    );
    put(world, pos, Block::REDSTONE_WIRE.default_state.id);
}

/// The POI entry saved to disk at `pos` (type and free tickets).
fn saved_poi(world: &World, pos: BlockPos) -> String {
    let mut storage = world
        .portal_poi
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if storage.save_all().is_err() {
        return " saved poi: save failed".to_string();
    }
    drop(storage);
    let region = (pos.0.x >> 9, pos.0.z >> 9);
    let path = world
        .level
        .level_folder
        .poi_folder
        .join(format!("r.{}.{}.mca", region.0, region.1));
    let Ok(region) = PoiRegion::load(&path) else {
        return " saved poi: no region".to_string();
    };
    let entries: Vec<String> = region
        .get_all()
        .iter()
        .filter(|entry| entry.pos() == pos)
        .map(|entry| format!("{} tickets {}", entry.poi_type, entry.free_tickets))
        .collect();
    format!(" saved poi {entries:?}")
}

fn red_shulker_box() -> BlockStateId {
    Block::RED_SHULKER_BOX.default_state.id
}

fn dirt() -> BlockStateId {
    Block::DIRT.default_state.id
}

fn unlit_torch() -> BlockStateId {
    torch(false)
}

fn plain_dust() -> BlockStateId {
    Block::REDSTONE_WIRE.default_state.id
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
        wide: false,
        probe: no_probe,
    }
}

const fn wide(
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
        wide: true,
        probe: no_probe,
    }
}

const fn probed(
    name: &'static str,
    setup: Setup,
    new_state: fn() -> BlockStateId,
    flags: BlockFlags,
    probe: Probe,
) -> Scene {
    Scene {
        name,
        setup,
        new_state,
        flags,
        wide: true,
        probe,
    }
}

const fn with(flags: BlockFlags, more: BlockFlags) -> BlockFlags {
    flags.union(more)
}

fn scenes() -> Vec<Scene> {
    let mut scenes = block_scenes();
    scenes.extend(neighbour_scenes());
    scenes.extend(wide_block_scenes());
    scenes.extend(wide_neighbour_scenes());
    scenes.extend(probe_scenes());
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

fn wide_block_scenes() -> Vec<Scene> {
    vec![
        wide(
            "red shulker box on a shulker box",
            on_shulker_box,
            red_shulker_box,
            ALL,
        ),
        wide(
            "dirt on stone with a stray chest entity",
            on_stone_with_stray_chest,
            dirt,
            ALL,
        ),
        wide("glowstone on air, lit", on_air, glowstone, ALL),
        wide("air on glowstone, lit", on_glowstone, air, ALL),
        wide("lectern on air, lit", on_air, lectern, ALL),
    ]
}

fn wide_neighbour_scenes() -> Vec<Scene> {
    vec![
        wide(
            "air on a torch under powered dust",
            torch_under_powered_dust,
            air,
            ALL,
        ),
        wide(
            "air on a torch under powered dust, listeners only",
            torch_under_powered_dust,
            air,
            BlockFlags::NOTIFY_LISTENERS,
        ),
        wide(
            "air on a torch under powered dust, moved",
            torch_under_powered_dust,
            air,
            with(BlockFlags::NOTIFY_LISTENERS, BlockFlags::MOVED),
        ),
        wide(
            "unlit torch on a torch under powered dust",
            torch_under_powered_dust,
            unlit_torch,
            ALL,
        ),
        wide(
            "air on stone under a torch, skip drops",
            stone_under_torch_beside_dust,
            air,
            with(ALL, BlockFlags::SKIP_DROPS),
        ),
        wide(
            "stone on dust below raised dust",
            dust_below_raised_dust,
            stone,
            ALL,
        ),
        wide(
            "dust below raised dust",
            air_below_raised_dust,
            plain_dust,
            ALL,
        ),
        wide(
            "composter filled, comparators behind stone",
            composter_behind_stone,
            composter_three,
            ALL,
        ),
    ]
}

fn probe_scenes() -> Vec<Scene> {
    let skip_added = with(ALL, BlockFlags::SKIP_BLOCK_ADDED_CALLBACK);
    vec![
        wide(
            "barrel on stone with a stray chest entity, no added callback",
            on_stone_with_stray_chest,
            barrel,
            skip_added,
        ),
        wide(
            "red shulker box on a shulker box, no added callback",
            on_shulker_box,
            red_shulker_box,
            skip_added,
        ),
        wide(
            "air on a nether portal, moved",
            on_nether_portal,
            air,
            with(BlockFlags::NOTIFY_LISTENERS, BlockFlags::MOVED),
        ),
        wide(
            "composter filled, comparators behind stone facing it",
            composter_behind_stone_facing_it,
            composter_three,
            ALL,
        ),
        wide(
            "dust up the stone to raised dust by tnt, no added callback",
            raised_dust_by_tnt,
            dust_up_east,
            skip_added,
        ),
        wide(
            "air on dust going up to raised dust",
            dust_up_to_raised_dust,
            air,
            ALL,
        ),
        probed("lectern on air, saved", on_air, lectern, ALL, saved_poi),
        wide(
            "air on dot dust under raised dust connected to it",
            dot_dust_under_raised_dust,
            air,
            ALL,
        ),
        wide(
            "air on dot dust under raised dust connected to it, listeners only",
            dot_dust_under_raised_dust,
            air,
            BlockFlags::NOTIFY_LISTENERS,
        ),
        wide(
            "air on dot dust written after the raised dust, listeners only",
            dot_dust_put_after_raised_dust,
            air,
            BlockFlags::NOTIFY_LISTENERS,
        ),
    ]
}

/// Every non-air state and scheduled tick in x/z -2..=2, y -1..=2 around `pos`.
fn box_text(world: &World, pos: BlockPos) -> String {
    let mut text = String::new();
    for dy in -1..=2 {
        for dz in -1..=1 {
            for dx in -2..=2 {
                let at = BlockPos::new(pos.0.x + dx, pos.0.y + dy, pos.0.z + dz);
                let state = world.get_block_state_id(&at);
                let block = Block::from_state_id(state);
                let ticked = world.is_block_tick_scheduled(&at, block);
                if !block.is_air() || ticked {
                    let tick = if ticked { "+tick" } else { "" };
                    let _ = write!(
                        text,
                        " {dx},{dy},{dz}={}#{}{tick}",
                        block.name,
                        state.as_u16()
                    );
                }
            }
        }
    }
    text
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
    if scene.wide {
        out.pop();
        let _ = writeln!(
            out,
            " box{}{}",
            box_text(world, pos),
            (scene.probe)(world, pos)
        );
    }
}

/// Dark light sections, as a generated chunk has before lighting (empty chunks have none, so
/// the light engine has nothing to write into).
fn give_light_sections(chunk: &pumpkin_world::chunk::ChunkData) {
    let count = chunk.section.count;
    let mut light = chunk
        .light_engine
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    light.block_light = (0..count).map(|_| LightContainer::new_empty(0)).collect();
    light.sky_light = (0..count).map(|_| LightContainer::new_empty(0)).collect();
}

/// Wide scenes sit far from the others, so their lit chunks never replace a neighbour of an
/// older scene's chunk.
const fn position_of(index: usize, wide: bool) -> BlockPos {
    let apart = if wide { 8192 } else { 0 };
    BlockPos::new(index as i32 * 16 + 3008 + apart, 64, 8)
}

#[tokio::test(flavor = "multi_thread")]
async fn block_state_set_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let scenes = scenes();
    for (index, scene) in scenes.iter().enumerate() {
        let chunk = position_of(index, scene.wide).chunk_position();
        for dx in -1..=1 {
            for dz in -1..=1 {
                let at = Vector2::new(chunk.x + dx, chunk.y + dz);
                let data = pumpkin_world::chunk::ChunkData::empty_sync(at.x, at.y);
                if scene.wide {
                    give_light_sections(&data);
                }
                world.level.loaded_chunks.insert(at, data);
            }
        }
    }
    let mut out = String::new();
    for (index, scene) in scenes.iter().enumerate() {
        let pos = position_of(index, scene.wide);
        (scene.setup)(&world, pos);
        drop(queued_changes(&world, pos));
        let returned = world.set_block_state(&pos, (scene.new_state)(), scene.flags);
        record(&mut out, &world, scene, pos, returned);
    }
    server.stop();
    script_golden::bless_or_compare("block_state_set.txt", &out)
}
