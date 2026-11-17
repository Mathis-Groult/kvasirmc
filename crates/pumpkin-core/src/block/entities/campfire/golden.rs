//! A record of the campfire's tick (`tests/golden/campfire_tick.txt`; rewrite it on purpose
//! with `KVASIR_BLESS=1`), made before `tick` was split.
//!
//! Each scene places a campfire (lit or not, plain or soul) with its block entity in the world,
//! fills the four slots with preset progress and total times, ticks it and applies the scene's
//! changes (light, put out, replace the block) at their ticks. A line (with the progress) is
//! written whenever anything but the progress changed: slots, totals, whether the chunk was
//! marked dirty (then cleared), whether a block entity update was queued for the chunk (then
//! cleared), and the items lying in the scene's area (block position, item, count; the drop
//! position inside the block is random). The end line gives the final state.

use std::fmt::Write;
use std::sync::Arc;

use pumpkin_data::block_properties::CampfireLikeProperties;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_world::chunk::io::Dirtiable;
use pumpkin_world::world::BlockFlags;

use super::CampfireBlockEntity;
use crate::block::entities::BlockEntity;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;
use crate::world::World;

/// What a scene does to the campfire block at a tick, before the campfire ticks.
#[derive(Clone, Copy)]
enum Change {
    Lit(bool),
    Stone,
}

struct Scene {
    name: &'static str,
    soul: bool,
    lit: bool,
    /// (item, count, progress, total) per slot; `None` = empty.
    slots: [Option<(&'static Item, u8, i32, i32)>; 4],
    ticks: u32,
    changes: &'static [(u32, Change)],
}

const fn beef(progress: i32, total: i32) -> (&'static Item, u8, i32, i32) {
    (&Item::BEEF, 1, progress, total)
}

const fn lit(
    name: &'static str,
    slots: [Option<(&'static Item, u8, i32, i32)>; 4],
    ticks: u32,
) -> Scene {
    Scene {
        name,
        soul: false,
        lit: true,
        slots,
        ticks,
        changes: &[],
    }
}

const fn unlit(mut s: Scene) -> Scene {
    s.lit = false;
    s
}

const fn soul(mut s: Scene) -> Scene {
    s.soul = true;
    s
}

const fn with_changes(mut s: Scene, changes: &'static [(u32, Change)]) -> Scene {
    s.changes = changes;
    s
}

const SCENES: &[Scene] = &[
    lit("empty", [None; 4], 5),
    lit("one beef", [Some(beef(0, 0)), None, None, None], 605),
    lit(
        "four foods",
        [
            Some(beef(0, 0)),
            Some((&Item::CHICKEN, 1, 0, 0)),
            Some((&Item::POTATO, 1, 0, 0)),
            Some((&Item::KELP, 1, 0, 0)),
        ],
        605,
    ),
    lit(
        "not cookable among foods",
        [
            None,
            Some((&Item::STICK, 1, 0, 0)),
            Some(beef(590, 0)),
            None,
        ],
        15,
    ),
    lit(
        "preset total kept",
        [Some(beef(8, 10)), None, None, None],
        5,
    ),
    lit(
        "preset progress, total from the recipe",
        [Some(beef(599, 0)), None, None, None],
        5,
    ),
    lit(
        "progress past the total",
        [Some(beef(700, 600)), None, None, None],
        5,
    ),
    lit(
        "two finish on one tick",
        [Some(beef(599, 600)), None, Some(beef(599, 600)), None],
        5,
    ),
    lit(
        "stack of three",
        [Some((&Item::BEEF, 3, 598, 600)), None, None, None],
        5,
    ),
    soul(lit(
        "soul campfire",
        [Some(beef(595, 600)), None, None, None],
        10,
    )),
    unlit(lit(
        "unlit cools down",
        [
            Some(beef(5, 600)),
            Some(beef(1, 600)),
            Some(beef(0, 600)),
            Some(beef(3, 600)),
        ],
        5,
    )),
    unlit(lit(
        "unlit with nothing to cool",
        [Some(beef(0, 600)), None, None, None],
        5,
    )),
    with_changes(
        lit(
            "put out then relit",
            [Some(beef(590, 600)), None, None, None],
            30,
        ),
        &[(3, Change::Lit(false)), (6, Change::Lit(true))],
    ),
    with_changes(
        lit(
            "replaced by stone",
            [Some(beef(595, 600)), None, None, None],
            10,
        ),
        &[(2, Change::Stone)],
    ),
    unlit(soul(lit(
        "unlit soul campfire",
        [Some(beef(4, 600)), None, None, None],
        5,
    ))),
    lit(
        "preset total of one",
        [Some(beef(0, 1)), None, None, None],
        3,
    ),
];

fn campfire_state(soul: bool, lit: bool) -> BlockStateId {
    let block = if soul {
        &Block::SOUL_CAMPFIRE
    } else {
        &Block::CAMPFIRE
    };
    let mut props = CampfireLikeProperties::default(block);
    props.lit = lit;
    props.to_state_id(block)
}

fn put(world: &Arc<World>, pos: &BlockPos, state: BlockStateId) {
    world.set_block_state(
        pos,
        state,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

fn stack_text(stack: &ItemStack) -> String {
    if stack.is_empty() {
        "-".to_string()
    } else {
        format!("{}x{}", stack.get_item().registry_key, stack.item_count)
    }
}

/// Chunk dirty flag and whether a block entity update is pending, both cleared after reading.
fn chunk_flags(world: &World, pos: &BlockPos) -> String {
    world
        .level
        .read_chunk_sync(&pos.chunk_position(), |chunk| {
            let dirty = chunk.is_dirty();
            chunk.mark_dirty(false);
            let pending = chunk
                .pending_block_entities
                .lock()
                .is_ok_and(|mut pending| pending.remove(pos).is_some());
            format!("dirty {dirty} update {pending}")
        })
        .unwrap_or_else(|| "no chunk".to_string())
}

fn items_on_ground(world: &World, x0: i32) -> String {
    let mut items = Vec::new();
    for entity in world.entities.load().iter() {
        let pos = entity.get_entity().pos.load();
        if pos.x < f64::from(x0) || pos.x >= f64::from(x0 + 16) {
            continue;
        }
        if let Some(item) = entity.get_item_entity() {
            let stack = item
                .get_item_stack()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            items.push(format!(
                "{},{},{}:{}",
                pos.x.floor() as i32 - x0,
                pos.y.floor() as i32,
                pos.z.floor() as i32,
                stack_text(&stack)
            ));
        }
    }
    items.sort();
    items.join(" ")
}

/// (everything but the progress, everything): lines are written when the first part changes,
/// so a cooking slot does not write one line per tick.
fn state(campfire: &CampfireBlockEntity, world: &World, x0: i32) -> (String, String) {
    let mut key = Vec::new();
    let mut slots = Vec::new();
    for slot in 0..CampfireBlockEntity::SLOT_COUNT {
        let stack = campfire.items[slot]
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let progress = *campfire.cooking_times[slot]
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let total = *campfire.cooking_total_times[slot]
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        key.push(format!("{}/{total}", stack_text(&stack)));
        slots.push(format!("{}@{progress}/{total}", stack_text(&stack)));
    }
    let rest = format!(
        "{} ground [{}]",
        chunk_flags(world, &campfire.position),
        items_on_ground(world, x0)
    );
    (
        format!("[{}] {rest}", key.join(" ")),
        format!("[{}] {rest}", slots.join(" ")),
    )
}

fn run(out: &mut String, world: &Arc<World>, index: usize, scene: &Scene) -> std::fmt::Result {
    let x0 = index as i32 * 16;
    let position = BlockPos::new(x0 + 8, 64, 8);
    put(world, &position.down(), Block::STONE.default_state.id);
    put(world, &position, campfire_state(scene.soul, scene.lit));
    let campfire = Arc::new(CampfireBlockEntity::new(position));
    for (slot, content) in scene.slots.iter().enumerate() {
        if let Some((item, count, progress, total)) = content {
            *campfire.items[slot]
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = ItemStack::new(*count, item);
            *campfire.cooking_times[slot]
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = *progress;
            *campfire.cooking_total_times[slot]
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = *total;
        }
    }
    world.add_block_entity(campfire.clone());
    let (mut last, full) = state(&campfire, world, x0);
    writeln!(out, "{}: start {full}", scene.name)?;
    for tick in 1..=scene.ticks {
        for (at, change) in scene.changes {
            if *at == tick {
                let block_state = match change {
                    Change::Lit(lit) => campfire_state(scene.soul, *lit),
                    Change::Stone => Block::STONE.default_state.id,
                };
                put(world, &position, block_state);
            }
        }
        campfire.tick(world);
        let (now, full) = state(&campfire, world, x0);
        if now != last {
            writeln!(out, "{}: tick {tick} {full}", scene.name)?;
            last = now;
        }
    }
    let (_, full) = state(&campfire, world, x0);
    writeln!(out, "{}: end {full}", scene.name)
}

#[tokio::test(flavor = "multi_thread")]
async fn campfire_tick_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    for cx in -1..=SCENES.len() as i32 {
        for cz in -1..=1 {
            world.level.loaded_chunks.insert(
                Vector2::new(cx, cz),
                pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
            );
        }
    }
    let mut now = String::new();
    for (index, scene) in SCENES.iter().enumerate() {
        run(&mut now, &world, index, scene)?;
    }
    script_golden::bless_or_compare("campfire_tick.txt", &now)?;
    server.stop();
    Ok(())
}
