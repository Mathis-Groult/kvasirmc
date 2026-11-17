//! A record of the brewing stand's tick (`tests/golden/brewing_stand_tick.txt`; rewrite it on
//! purpose with `KVASIR_BLESS=1`), made before `tick` and `do_brew` were split.
//!
//! Each scene sets up a stand (slots, stored fuel, brew time), ticks it, applies the scene's
//! slot changes at their ticks, and writes a line whenever anything but the brew time changed:
//! brew time, fuel, the five slots (item, count, potion, custom colour), the remembered
//! ingredient, the bottle bits, the dirty and comparator flags (cleared after each read) and
//! the stand's block state. A final line gives the state after the last tick. Datapack recipes
//! (stick, apple, bone, feather, flint, nether wart) are added to the server first, so both
//! the vanilla table and the dynamic list are exercised.

use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use pumpkin_data::Block;
use pumpkin_data::data_component_impl::PotionContentsImpl;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::packet::CURRENT_MC_VERSION;
use pumpkin_data::potion::Potion;
use pumpkin_inventory::Inventory;
use pumpkin_protocol::codec::recipe::{DynamicRecipe, OwnedBrewingRecipe};
use pumpkin_protocol::java::client::play::CSoundEffect;
use pumpkin_protocol::packet::MultiVersionJavaPacket;
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::BlockFlags;

use super::BrewingStandBlockEntity;
use crate::block::entities::BlockEntity;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;
use crate::net::java::PacketTap;
use crate::world::World;

/// A slot's content in a scene.
#[derive(Clone, Copy)]
enum Spec {
    Empty,
    Plain(&'static Item, u8),
    Potion(&'static Item, &'static Potion, u8),
    /// A water bottle with a custom colour (kept through a brew?).
    Coloured,
}

const fn water() -> Spec {
    Spec::Potion(&Item::POTION, &Potion::WATER, 1)
}

const fn potion(potion: &'static Potion) -> Spec {
    Spec::Potion(&Item::POTION, potion, 1)
}

const fn plain(item: &'static Item, count: u8) -> Spec {
    Spec::Plain(item, count)
}

const E: Spec = Spec::Empty;

struct Scene {
    name: &'static str,
    /// Slots 0-2 bottles, 3 ingredient, 4 fuel.
    slots: [Spec; 5],
    fuel: i32,
    brew_time: i32,
    ticks: u32,
    /// (tick, slot, new content), applied before that tick.
    changes: &'static [(u32, usize, Spec)],
    /// The bottle bits already match the slots, so the first tick does not mark the stand dirty.
    bits_known: bool,
}

const fn scene(name: &'static str, slots: [Spec; 5], ticks: u32) -> Scene {
    Scene {
        name,
        slots,
        fuel: 0,
        brew_time: 0,
        ticks,
        changes: &[],
        bits_known: false,
    }
}

const fn with_bits_known(mut s: Scene) -> Scene {
    s.bits_known = true;
    s
}

const fn with_fuel(mut s: Scene, fuel: i32) -> Scene {
    s.fuel = fuel;
    s
}

const fn with_brew_time(mut s: Scene, brew_time: i32) -> Scene {
    s.brew_time = brew_time;
    s
}

const fn with_changes(mut s: Scene, changes: &'static [(u32, usize, Spec)]) -> Scene {
    s.changes = changes;
    s
}

const BLAZE: Spec = Spec::Plain(&Item::BLAZE_POWDER, 1);
const WART: Spec = Spec::Plain(&Item::NETHER_WART, 1);

const SCENES: &[Scene] = &[
    scene(
        "awkward from water",
        [water(), water(), water(), WART, BLAZE],
        410,
    ),
    scene(
        "two warts, second not brewable",
        [
            water(),
            water(),
            water(),
            plain(&Item::NETHER_WART, 2),
            BLAZE,
        ],
        410,
    ),
    scene("no fuel", [water(), water(), water(), WART, E], 30),
    with_fuel(scene("stored fuel only", [water(), E, E, WART, E], 410), 1),
    scene(
        "fuel slot holds a stick",
        [water(), E, E, WART, plain(&Item::STICK, 1)],
        30,
    ),
    scene(
        "refill with nothing to brew",
        [
            potion(&Potion::AWKWARD),
            E,
            E,
            plain(&Item::DIRT, 1),
            plain(&Item::BLAZE_POWDER, 2),
        ],
        30,
    ),
    with_fuel(
        scene(
            "stored fuel and a fuel item",
            [water(), E, E, WART, BLAZE],
            410,
        ),
        1,
    ),
    with_changes(
        scene(
            "ingredient removed mid-brew",
            [water(), water(), E, WART, BLAZE],
            200,
        ),
        &[(100, 3, Spec::Empty)],
    ),
    with_changes(
        scene(
            "ingredient swapped for another brewable one",
            [
                potion(&Potion::AWKWARD),
                E,
                E,
                plain(&Item::SUGAR, 1),
                BLAZE,
            ],
            200,
        ),
        &[(100, 3, Spec::Plain(&Item::RABBIT_FOOT, 1))],
    ),
    with_changes(
        scene(
            "ingredient swapped for a non-brewable one",
            [
                potion(&Potion::AWKWARD),
                E,
                E,
                plain(&Item::SUGAR, 1),
                BLAZE,
            ],
            200,
        ),
        &[(100, 3, Spec::Plain(&Item::DIRT, 1))],
    ),
    with_changes(
        scene(
            "potions removed mid-brew",
            [water(), water(), E, WART, BLAZE],
            200,
        ),
        &[(100, 0, Spec::Empty), (100, 1, Spec::Empty)],
    ),
    with_changes(
        scene(
            "one potion removed mid-brew",
            [water(), water(), E, WART, BLAZE],
            410,
        ),
        &[(100, 0, Spec::Empty)],
    ),
    with_brew_time(
        scene(
            "brew time 1, brewable, nothing remembered",
            [water(), E, water(), WART, E],
            5,
        ),
        1,
    ),
    with_brew_time(
        scene(
            "brew time 1, not brewable",
            [water(), E, E, plain(&Item::DIRT, 1), E],
            5,
        ),
        1,
    ),
    with_brew_time(
        scene(
            "brew time 50, brewable, nothing remembered",
            [water(), E, E, WART, BLAZE],
            5,
        ),
        50,
    ),
    scene(
        "mixed slots",
        [
            water(),
            potion(&Potion::AWKWARD),
            E,
            plain(&Item::NETHER_WART, 3),
            BLAZE,
        ],
        410,
    ),
    scene(
        "glass bottle and gaps",
        [plain(&Item::GLASS_BOTTLE, 1), E, water(), E, E],
        5,
    ),
    scene(
        "splash night vision",
        [
            potion(&Potion::NIGHT_VISION),
            E,
            potion(&Potion::NIGHT_VISION),
            plain(&Item::GUNPOWDER, 4),
            BLAZE,
        ],
        410,
    ),
    scene(
        "lingering from splash",
        [
            Spec::Potion(&Item::SPLASH_POTION, &Potion::SWIFTNESS, 1),
            E,
            E,
            plain(&Item::DRAGON_BREATH, 1),
            BLAZE,
        ],
        410,
    ),
    scene(
        "custom colour kept",
        [Spec::Coloured, E, E, WART, BLAZE],
        410,
    ),
    scene(
        "stick in a bottle slot",
        [plain(&Item::STICK, 1), E, E, WART, BLAZE],
        30,
    ),
    scene(
        "stacked water bottles",
        [
            Spec::Potion(&Item::POTION, &Potion::WATER, 3),
            E,
            E,
            WART,
            BLAZE,
        ],
        410,
    ),
    scene(
        "datapack chain restarts at once",
        [
            potion(&Potion::MUNDANE),
            E,
            E,
            plain(&Item::STICK, 3),
            BLAZE,
        ],
        820,
    ),
    scene(
        "datapack recipe without potions",
        [
            plain(&Item::GLASS_BOTTLE, 1),
            E,
            E,
            plain(&Item::APPLE, 2),
            BLAZE,
        ],
        410,
    ),
    scene(
        "datapack output item unknown",
        [water(), E, E, plain(&Item::BONE, 2), BLAZE],
        820,
    ),
    scene(
        "datapack output potion unknown",
        [water(), E, E, plain(&Item::FEATHER, 1), BLAZE],
        410,
    ),
    scene(
        "datapack input potion differs",
        [water(), E, E, plain(&Item::FLINT, 1), BLAZE],
        30,
    ),
    with_changes(
        scene(
            "ingredient swapped on the finishing tick",
            [
                potion(&Potion::AWKWARD),
                E,
                E,
                plain(&Item::SUGAR, 1),
                BLAZE,
            ],
            410,
        ),
        &[(401, 3, Spec::Plain(&Item::RABBIT_FOOT, 2))],
    ),
    with_fuel(
        scene(
            "datapack chain without fuel left",
            [potion(&Potion::MUNDANE), E, E, plain(&Item::STICK, 3), E],
            820,
        ),
        1,
    ),
    with_bits_known(with_brew_time(
        scene(
            "brew time 1, not brewable, bits known",
            [water(), E, E, plain(&Item::DIRT, 1), E],
            5,
        ),
        1,
    )),
    with_bits_known(with_brew_time(
        scene(
            "brew time 1, brewable, bits known",
            [water(), E, E, WART, E],
            5,
        ),
        1,
    )),
    with_bits_known(scene(
        "refill with nothing to brew, bits known",
        [
            potion(&Potion::AWKWARD),
            E,
            E,
            plain(&Item::DIRT, 1),
            plain(&Item::BLAZE_POWDER, 2),
        ],
        5,
    )),
    with_bits_known(with_changes(
        scene(
            "ingredient removed mid-brew, bits known",
            [water(), E, E, WART, BLAZE],
            5,
        ),
        &[(3, 3, Spec::Empty)],
    )),
    scene(
        "datapack input potion matches",
        [potion(&Potion::THICK), E, E, plain(&Item::FLINT, 1), BLAZE],
        410,
    ),
];

fn brewing(
    id: &str,
    input: &str,
    potion: Option<&str>,
    reagent: &str,
    out: (&str, Option<&str>),
) -> DynamicRecipe {
    DynamicRecipe::Brewing(OwnedBrewingRecipe {
        recipe_id: format!("test:{id}"),
        input_item: input.to_string(),
        input_potion: potion.map(str::to_string),
        reagent: reagent.to_string(),
        output_item: out.0.to_string(),
        output_potion: out.1.map(str::to_string),
    })
}

fn datapack_recipes() -> Vec<DynamicRecipe> {
    let potion = "minecraft:potion";
    vec![
        brewing(
            "mundane_thick",
            potion,
            Some("minecraft:mundane"),
            "minecraft:stick",
            (potion, Some("minecraft:thick")),
        ),
        brewing(
            "thick_awkward",
            potion,
            Some("minecraft:thick"),
            "minecraft:stick",
            (potion, Some("minecraft:awkward")),
        ),
        brewing(
            "honey",
            "minecraft:glass_bottle",
            None,
            "minecraft:apple",
            ("minecraft:honey_bottle", None),
        ),
        brewing(
            "unknown_item",
            potion,
            None,
            "minecraft:bone",
            ("minecraft:no_such_item", None),
        ),
        brewing(
            "unknown_potion",
            potion,
            None,
            "minecraft:feather",
            (potion, Some("minecraft:no_such_potion")),
        ),
        brewing(
            "thick_flint",
            potion,
            Some("minecraft:thick"),
            "minecraft:flint",
            ("minecraft:splash_potion", Some("minecraft:mundane")),
        ),
        // Its input is its own reagent: only a loop that wrongly treats the ingredient slot as a
        // bottle slot would brew it.
        brewing(
            "apple_self",
            "minecraft:apple",
            None,
            "minecraft:apple",
            ("minecraft:golden_apple", None),
        ),
        // The vanilla table answers water + nether wart first; this must never win.
        brewing(
            "shadowed",
            potion,
            Some("minecraft:water"),
            "minecraft:nether_wart",
            (potion, Some("minecraft:thick")),
        ),
    ]
}

fn stack(spec: Spec) -> ItemStack {
    match spec {
        Spec::Empty => ItemStack::EMPTY.clone(),
        Spec::Plain(item, count) => ItemStack::new(count, item),
        Spec::Potion(item, potion, count) => {
            let mut stack = ItemStack::new(count, item);
            stack.set_data_component(PotionContentsImpl {
                potion_id: Some(i32::from(potion.id)),
                custom_color: None,
                custom_effects: Vec::new(),
                custom_name: None,
            });
            stack
        }
        Spec::Coloured => {
            let mut stack = ItemStack::new(1, &Item::POTION);
            stack.set_data_component(PotionContentsImpl {
                potion_id: Some(i32::from(Potion::WATER.id)),
                custom_color: Some(0x00_12_34_56),
                custom_effects: Vec::new(),
                custom_name: None,
            });
            stack
        }
    }
}

fn slot_text(stack: &ItemStack) -> String {
    if stack.is_empty() {
        return "-".to_string();
    }
    let mut text = format!("{}x{}", stack.get_item().registry_key, stack.item_count);
    if let Some(contents) = stack.get_data_component::<PotionContentsImpl>() {
        let name = contents
            .potion_id
            .and_then(|id| u8::try_from(id).ok())
            .and_then(Potion::from_id)
            .map_or("none", |potion| potion.name);
        text.push(':');
        text.push_str(name);
        if let Some(colour) = contents.custom_color {
            text = format!("{text}#{colour:x}");
        }
    }
    text
}

/// Everything but the brew time, then the brew time.
fn state(stand: &BrewingStandBlockEntity, world: &World) -> (String, i32) {
    let slots: Vec<String> = (0..5)
        .map(|slot| slot_text(&stand.get_stack(slot)))
        .collect();
    let ingredient = stand.ingredient_item.lock().map_or("poisoned", |item| {
        item.map_or("-", |item| item.registry_key)
    });
    let bits = stand
        .last_potion_count
        .lock()
        .map(|bits| format!("{bits:?}"))
        .unwrap_or_default();
    let dirty = stand.is_dirty();
    let comparator = stand.is_comparator_dirty();
    stand.clear_dirty();
    stand.clear_comparator_dirty();
    let block_state = world.get_block_state_id(&stand.position);
    let text = format!(
        "fuel {} slots [{}] ingredient {ingredient} bits {bits} dirty {dirty} comparator {comparator} state {block_state:?}",
        stand.fuel.load(Ordering::Relaxed),
        slots.join(" "),
    );
    (text, stand.brew_time.load(Ordering::Relaxed))
}

fn var_int(bytes: &[u8], mut at: usize) -> Option<(i32, usize)> {
    let mut value = 0i32;
    for shift in (0..35).step_by(7) {
        let byte = *bytes.get(at)?;
        at += 1;
        value |= i32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some((value, at));
        }
    }
    None
}

/// The sound packets the listener was sent since the last call, as hex without their random
/// seed (the last 8 bytes), each prefixed ` sound `; other packets are left out.
fn sounds(tap: &mut PacketTap) -> String {
    let id = CSoundEffect::to_id(CURRENT_MC_VERSION);
    let mut text = String::new();
    for packet in tap.drain() {
        let first = var_int(&packet, 0);
        let is_sound = first.is_some_and(|(value, _)| value == id)
            || first
                .and_then(|(_, next)| var_int(&packet, next))
                .is_some_and(|(value, _)| value == id);
        if is_sound && packet.len() >= 8 {
            text.push_str(" sound ");
            for byte in &packet[..packet.len() - 8] {
                text = format!("{text}{byte:02x}");
            }
        }
    }
    text
}

fn run(
    out: &mut String,
    world: &Arc<World>,
    listener: &mut Listener,
    index: usize,
    scene: &Scene,
) -> std::fmt::Result {
    let position = BlockPos::new(index as i32 * 16 + 8, 64, 8);
    listener
        .player
        .living_entity
        .entity
        .set_pos(Vector3::new(f64::from(position.0.x), 64.0, 8.0));
    world.set_block_state(
        &position,
        Block::BREWING_STAND.default_state.id,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
    let stand = BrewingStandBlockEntity::new(position);
    for (slot, spec) in scene.slots.iter().enumerate() {
        stand.set_stack(slot, stack(*spec));
    }
    stand.fuel.store(scene.fuel, Ordering::Relaxed);
    stand.brew_time.store(scene.brew_time, Ordering::Relaxed);
    if scene.bits_known {
        let bits = [0, 1, 2].map(|slot| {
            let stack = stand.get_stack(slot);
            !stack.is_empty()
                && (stack.get_data_component::<PotionContentsImpl>().is_some()
                    || stack.get_item().id == Item::GLASS_BOTTLE.id)
        });
        if let Ok(mut last) = stand.last_potion_count.lock() {
            *last = Some(bits);
        }
    }
    sounds(&mut listener.tap);
    let (mut last, brew_time) = state(&stand, world);
    writeln!(out, "{}: start brew {brew_time} {last}", scene.name)?;
    for tick in 1..=scene.ticks {
        for (at, slot, spec) in scene.changes {
            if *at == tick {
                stand.set_stack(*slot, stack(*spec));
            }
        }
        stand.tick(world);
        let (mut now, brew_time) = state(&stand, world);
        now.push_str(&sounds(&mut listener.tap));
        if now != last {
            writeln!(out, "{}: tick {tick} brew {brew_time} {now}", scene.name)?;
            last = now;
        }
    }
    let (_, brew_time) = state(&stand, world);
    writeln!(out, "{}: end brew {brew_time}", scene.name)
}

/// A player near the scene's stand who receives its sounds.
struct Listener {
    player: Arc<crate::entity::player::Player>,
    tap: PacketTap,
}

#[tokio::test(flavor = "multi_thread")]
async fn brewing_stand_tick_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    for recipe in datapack_recipes() {
        server.recipe_manager.add_recipe(recipe);
    }
    let world = server.worlds.load()[0].clone();
    for cx in -1..=SCENES.len() as i32 {
        for cz in -1..=1 {
            world.level.loaded_chunks.insert(
                Vector2::new(cx, cz),
                pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
            );
        }
    }
    let (player, tap) =
        test_support::java_player_with_tap(&world, GameMode::Survival).ok_or("no packet tap")?;
    world.add_player(&player)?;
    let mut listener = Listener { player, tap };
    let mut now = String::new();
    for (index, scene) in SCENES.iter().enumerate() {
        run(&mut now, &world, &mut listener, index, scene)?;
    }
    world.remove_player(&listener.player, false).await;
    script_golden::bless_or_compare("brewing_stand_tick.txt", &now)?;
    server.stop();
    Ok(())
}
