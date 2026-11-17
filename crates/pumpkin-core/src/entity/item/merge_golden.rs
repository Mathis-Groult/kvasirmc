//! A record of what `ItemEntity::try_merge_with` does, made before the function was split
//! (`tests/golden/item_merge.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene spawns two item entities (`a` first, so `a` has the lower entity id) in a loaded
//! area of a real server's world, with preset stacks, ages, pickup delays, removed and merge
//! reservation flags, and calls `try_merge_with` once, on `a` with `b` or on `b` with `a`. One
//! line per scene: both stacks, ages, pickup delays and removed flags after the call.

use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use pumpkin_data::data_component_impl::CustomNameImpl;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::text::TextComponent;

use super::ItemEntity;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::Entity;
use crate::entity::ai::test_support;
use crate::world::World;

/// One side of a scene: item, count, named, age, pickup delay, removed, merge reserved.
#[derive(Clone, Copy)]
struct Side {
    item: &'static Item,
    count: u8,
    named: bool,
    age: u32,
    delay: u8,
    removed: bool,
    reserved: bool,
}

const fn side(item: &'static Item, count: u8) -> Side {
    Side {
        item,
        count,
        named: false,
        age: 0,
        delay: 0,
        removed: false,
        reserved: false,
    }
}

const fn aged(mut side: Side, age: u32, delay: u8) -> Side {
    side.age = age;
    side.delay = delay;
    side
}

const fn named(mut side: Side) -> Side {
    side.named = true;
    side
}

const fn removed(mut side: Side) -> Side {
    side.removed = true;
    side
}

const fn reserved(mut side: Side) -> Side {
    side.reserved = true;
    side
}

struct Scene {
    name: &'static str,
    a: Side,
    b: Side,
    /// Calls `b.try_merge_with(a)` instead of `a.try_merge_with(b)`.
    from_b: bool,
}

const fn scene(name: &'static str, a: Side, b: Side) -> Scene {
    Scene {
        name,
        a,
        b,
        from_b: false,
    }
}

const fn reversed(mut scene: Scene) -> Scene {
    scene.from_b = true;
    scene
}

const STONE: &Item = &Item::STONE;
const PEARL: &Item = &Item::ENDER_PEARL;
const SWORD: &Item = &Item::IRON_SWORD;

const SCENES: &[Scene] = &[
    scene("a bigger", side(STONE, 20), side(STONE, 10)),
    scene("b bigger", side(STONE, 10), side(STONE, 20)),
    reversed(scene("a bigger, from b", side(STONE, 20), side(STONE, 10))),
    reversed(scene("b bigger, from b", side(STONE, 10), side(STONE, 20))),
    scene("equal counts", side(STONE, 5), side(STONE, 5)),
    reversed(scene(
        "equal counts, from b",
        side(STONE, 5),
        side(STONE, 5),
    )),
    scene("exactly a full stack", side(STONE, 32), side(STONE, 32)),
    scene("one over a full stack", side(STONE, 40), side(STONE, 25)),
    scene("one and sixty-three", side(STONE, 1), side(STONE, 63)),
    scene("different items", side(STONE, 3), side(&Item::DIRT, 3)),
    scene("one named", named(side(STONE, 3)), side(STONE, 4)),
    scene("both named", named(side(STONE, 3)), named(side(STONE, 4))),
    scene("pearls to sixteen", side(PEARL, 10), side(PEARL, 6)),
    scene("pearls over sixteen", side(PEARL, 10), side(PEARL, 7)),
    scene("unstackable swords", side(SWORD, 1), side(SWORD, 1)),
    scene(
        "ages and delays, target older",
        aged(side(STONE, 20), 300, 5),
        aged(side(STONE, 10), 100, 30),
    ),
    scene(
        "ages and delays, target younger",
        aged(side(STONE, 20), 100, 30),
        aged(side(STONE, 10), 300, 5),
    ),
    scene("a removed", removed(side(STONE, 20)), side(STONE, 10)),
    scene("b removed", side(STONE, 20), removed(side(STONE, 10))),
    scene("a reserved", reserved(side(STONE, 20)), side(STONE, 10)),
    scene("b reserved", side(STONE, 20), reserved(side(STONE, 10))),
];

fn stack(side: &Side) -> ItemStack {
    let mut stack = ItemStack::new(side.count, side.item);
    if side.named {
        stack.set_data_component(CustomNameImpl {
            name: TextComponent::text("Named"),
        });
    }
    stack
}

fn spawn(world: &Arc<World>, at: Vector3<f64>, side: &Side) -> Result<Arc<ItemEntity>, String> {
    let item = Arc::new(ItemEntity::new(
        Entity::new(world.clone(), at, &EntityType::ITEM),
        stack(side),
    ));
    if !world.spawn_entity(item.clone()) {
        return Err("item not spawned".to_string());
    }
    item.item_age.store(side.age, Ordering::Relaxed);
    item.pickup_delay.store(side.delay, Ordering::Relaxed);
    item.merge_reserved.store(side.reserved, Ordering::Relaxed);
    if side.removed {
        item.entity.removed.store(true, Ordering::SeqCst);
    }
    Ok(item)
}

fn item_text(item: &ItemEntity) -> String {
    let stack = item
        .item_stack
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    format!(
        "{}x{} named {} age {} delay {} removed {} reserved {}",
        stack.item.registry_key,
        stack.item_count,
        stack.get_data_component::<CustomNameImpl>().is_some(),
        item.item_age.load(Ordering::Relaxed),
        item.pickup_delay.load(Ordering::Relaxed),
        item.entity.removed.load(Ordering::SeqCst),
        item.merge_reserved.load(Ordering::Relaxed),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn item_merge_is_the_same() -> TestResult {
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
    let mut out = String::new();
    for (index, scene) in SCENES.iter().enumerate() {
        let x = index as f64 * 16.0 + 8.5;
        let a = spawn(&world, Vector3::new(x, 64.0, 8.5), &scene.a)?;
        let b = spawn(&world, Vector3::new(x + 0.2, 64.0, 8.5), &scene.b)?;
        if scene.from_b {
            b.try_merge_with(&a);
        } else {
            a.try_merge_with(&b);
        }
        let _ = writeln!(
            out,
            "{}: a {}; b {}",
            scene.name,
            item_text(&a),
            item_text(&b)
        );
    }
    server.stop();
    script_golden::bless_or_compare("item_merge.txt", &out)
}
