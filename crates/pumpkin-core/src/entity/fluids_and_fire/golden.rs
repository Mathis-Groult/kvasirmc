//! A record of what `Entity::update_fluid_interaction` does, made before the function was split
//! (`tests/golden/fluid_interaction.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene lays fluids around one block of a real server's world (overworld, or the nether
//! for fast lava), puts one entity there (a zombie `LivingEntity`, an item, an interaction
//! entity, which fluids do not push, or a player) with a preset velocity, fall distance and
//! touching/first-tick flags, and calls `update_fluid_interaction` twice. After each call: the
//! fluids returned, the velocity, both fluid heights, both touching flags, the fall distance of
//! a living entity, and the splash sounds a listener player heard (sound packet without its
//! random pitch and seed).
//!
//! Not seen: game events (splash), a plugin, entities other than these four kinds.

use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use pumpkin_data::block_properties::WhiteWoolSlabLikeProperties;
use pumpkin_data::entity::EntityType;
use pumpkin_data::fluid::{Fluid, FluidProperties, Level};
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::packet::CURRENT_MC_VERSION;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_protocol::java::client::play::CSoundEffect;
use pumpkin_protocol::packet::MultiVersionJavaPacket;
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::BlockFlags;

use crate::block::fluid::flowing_trait::FlowingFluid;
use crate::block::fluid::lava::FlowingLava;
use crate::block::fluid::water::FlowingWater;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;
use crate::entity::interaction::InteractionEntity;
use crate::entity::item::ItemEntity;
use crate::entity::living::LivingEntity;
use crate::entity::{Entity, EntityBase};
use crate::net::java::PacketTap;
use crate::world::World;

/// The entity a scene puts into the fluid.
#[derive(Clone, Copy)]
enum Subject {
    Zombie,
    Item,
    Interaction,
    Player,
}

/// One scene: a 5x5 map per layer (`dy` from the feet block), letters as in [`cell`].
struct Scene {
    name: &'static str,
    layers: &'static [(i32, [&'static str; 5])],
    subject: Subject,
    /// Offset of the entity from the north-west corner of the centre feet block.
    at: (f64, f64, f64),
    velocity: (f64, f64, f64),
    fall: f32,
    touching_water: bool,
    first_tick: bool,
    nether: bool,
}

const fn scene(
    name: &'static str,
    layers: &'static [(i32, [&'static str; 5])],
    subject: Subject,
) -> Scene {
    Scene {
        name,
        layers,
        subject,
        at: (0.5, 0.0, 0.5),
        velocity: (0.0, 0.0, 0.0),
        fall: 0.0,
        touching_water: false,
        first_tick: false,
        nether: false,
    }
}

const fn at(mut scene: Scene, x: f64, y: f64, z: f64) -> Scene {
    scene.at = (x, y, z);
    scene
}

const fn moving(mut scene: Scene, x: f64, y: f64, z: f64) -> Scene {
    scene.velocity = (x, y, z);
    scene
}

const fn falling(mut scene: Scene, fall: f32) -> Scene {
    scene.fall = fall;
    scene
}

const fn wet(mut scene: Scene) -> Scene {
    scene.touching_water = true;
    scene
}

const fn first(mut scene: Scene) -> Scene {
    scene.first_tick = true;
    scene
}

const fn nether(mut scene: Scene) -> Scene {
    scene.nether = true;
    scene
}

const ALL_S: [&str; 5] = ["SSSSS", "SSSSS", "SSSSS", "SSSSS", "SSSSS"];
const ALL_L: [&str; 5] = ["LLLLL", "LLLLL", "LLLLL", "LLLLL", "LLLLL"];
const DRY: [&str; 5] = [".....", ".....", ".....", ".....", "....."];
const CENTRE_S: [&str; 5] = [".....", ".....", "..S..", ".....", "....."];
const EAST_FLOW: [&str; 5] = ["S7654", "S7654", "S7654", "S7654", "S7654"];
const LAVA_EAST: [&str; 5] = ["Ll1..", "Ll1..", "Ll1..", "Ll1..", "Ll1.."];
const VALLEY: [&str; 5] = ["76567", "76567", "76567", "76567", "76567"];
const SHALLOW: [&str; 5] = ["11111", "11111", "11111", "11111", "11111"];
const FALLS: [&str; 5] = ["FFFFF", "FFFFF", "FFFFF", "FFFFF", "FFFFF"];
const MIXED: [&str; 5] = ["SSLLL", "SSLLL", "SSLLL", "SSLLL", "SSLLL"];
const SLABS: [&str; 5] = ["WWWWW", "WWWWW", "WWWWW", "WWWWW", "WWWWW"];
const STONE: [&str; 5] = ["#####", "#####", "#####", "#####", "#####"];
const SHALLOW_EAST: [&str; 5] = ["321..", "321..", "321..", "321..", "321.."];
const EAST_COLUMN: [&str; 5] = ["...S.", "...S.", "...S.", "...S.", "...S."];
const WEST_COLUMN: [&str; 5] = [".S...", ".S...", ".S...", ".S...", ".S..."];
const SOUTH_ROW: [&str; 5] = [".....", ".....", ".....", "SSSSS", "....."];
const NORTH_ROW: [&str; 5] = [".....", "SSSSS", ".....", ".....", "....."];

const SCENES: &[Scene] = &[
    scene("dry zombie", &[(0, DRY)], Subject::Zombie),
    falling(
        scene("zombie in still water", &[(0, ALL_S)], Subject::Zombie),
        5.0,
    ),
    scene(
        "zombie in one water cell",
        &[(0, CENTRE_S)],
        Subject::Zombie,
    ),
    scene(
        "zombie in water flowing east",
        &[(0, EAST_FLOW)],
        Subject::Zombie,
    ),
    moving(
        scene(
            "zombie moving in water flowing east",
            &[(0, EAST_FLOW)],
            Subject::Zombie,
        ),
        0.2,
        0.0,
        0.1,
    ),
    at(
        scene("zombie across two flows", &[(0, VALLEY)], Subject::Zombie),
        0.0,
        0.0,
        0.5,
    ),
    scene("zombie in shallow water", &[(0, SHALLOW)], Subject::Zombie),
    scene(
        "zombie in falling water",
        &[(0, FALLS), (1, FALLS)],
        Subject::Zombie,
    ),
    at(
        scene(
            "zombie just above still water",
            &[(0, ALL_S)],
            Subject::Zombie,
        ),
        0.5,
        0.95,
        0.5,
    ),
    at(
        scene(
            "zombie standing on still water",
            &[(0, ALL_S)],
            Subject::Zombie,
        ),
        0.5,
        1.0,
        0.5,
    ),
    at(
        scene(
            "zombie with water at its head",
            &[(0, DRY), (1, ALL_S)],
            Subject::Zombie,
        ),
        0.5,
        0.0,
        0.5,
    ),
    scene(
        "zombie under two layers of water",
        &[(0, ALL_S), (1, ALL_S)],
        Subject::Zombie,
    ),
    scene(
        "zombie in waterlogged slabs",
        &[(0, SLABS)],
        Subject::Zombie,
    ),
    falling(scene("zombie in lava", &[(0, ALL_L)], Subject::Zombie), 6.0),
    falling(
        scene("zombie in lava, no fall", &[(0, ALL_L)], Subject::Zombie),
        0.0,
    ),
    scene(
        "zombie in lava flowing east",
        &[(0, LAVA_EAST)],
        Subject::Zombie,
    ),
    nether(scene(
        "nether zombie in lava flowing east",
        &[(0, LAVA_EAST)],
        Subject::Zombie,
    )),
    at(
        scene(
            "zombie between water and lava",
            &[(0, MIXED)],
            Subject::Zombie,
        ),
        0.0,
        0.0,
        0.5,
    ),
    wet(scene("zombie already wet", &[(0, ALL_S)], Subject::Zombie)),
    first(scene(
        "zombie on its first tick",
        &[(0, ALL_S)],
        Subject::Zombie,
    )),
    moving(
        scene(
            "zombie diving fast into water",
            &[(0, ALL_S)],
            Subject::Zombie,
        ),
        0.0,
        -1.2,
        0.0,
    ),
    scene("zombie in stone", &[(0, STONE)], Subject::Zombie),
    scene(
        "zombie in shallow water flowing east",
        &[(0, SHALLOW_EAST)],
        Subject::Zombie,
    ),
    scene(
        "zombie in two layers flowing east",
        &[(0, EAST_FLOW), (1, EAST_FLOW)],
        Subject::Zombie,
    ),
    moving(
        scene(
            "zombie drifting in water flowing east",
            &[(0, EAST_FLOW)],
            Subject::Zombie,
        ),
        0.0029,
        0.0,
        0.0029,
    ),
    moving(
        scene(
            "zombie drifting east past the threshold",
            &[(0, EAST_FLOW)],
            Subject::Zombie,
        ),
        0.0031,
        0.0,
        0.0,
    ),
    moving(
        scene(
            "zombie rising slowly in water flowing east",
            &[(0, EAST_FLOW)],
            Subject::Zombie,
        ),
        0.0,
        0.004,
        0.0,
    ),
    moving(
        scene(
            "zombie rising in water flowing east",
            &[(0, EAST_FLOW)],
            Subject::Zombie,
        ),
        0.0,
        0.005,
        0.0,
    ),
    at(
        scene("player across two flows", &[(0, VALLEY)], Subject::Player),
        0.0,
        0.0,
        0.5,
    ),
    at(
        scene("item across two flows", &[(0, VALLEY)], Subject::Item),
        0.0,
        0.0,
        0.5,
    ),
    scene(
        "item in water flowing east",
        &[(0, EAST_FLOW)],
        Subject::Item,
    ),
    scene("item in lava", &[(0, ALL_L)], Subject::Item),
    scene(
        "interaction in water flowing east",
        &[(0, EAST_FLOW)],
        Subject::Interaction,
    ),
    scene(
        "interaction between water and lava",
        &[(0, MIXED)],
        Subject::Interaction,
    ),
    falling(
        scene(
            "player in water flowing east",
            &[(0, EAST_FLOW)],
            Subject::Player,
        ),
        3.0,
    ),
    scene(
        "player in lava flowing east",
        &[(0, LAVA_EAST)],
        Subject::Player,
    ),
    moving(
        scene(
            "player in still water, slow",
            &[(0, ALL_S)],
            Subject::Player,
        ),
        0.001,
        0.0,
        0.001,
    ),
    // Appended after the first mutation run: box edges exactly on a block border, the shallow
    // scaling and the lava cell count seen by a moving player, a fall distance that halves to
    // exactly 1, and velocities on the small-velocity thresholds.
    at(
        scene(
            "zombie touching water to the east",
            &[(0, EAST_COLUMN)],
            Subject::Zombie,
        ),
        0.7,
        0.0,
        0.5,
    ),
    at(
        scene(
            "zombie touching water to the west",
            &[(0, WEST_COLUMN)],
            Subject::Zombie,
        ),
        0.3,
        0.0,
        0.5,
    ),
    at(
        scene(
            "zombie touching water to the south",
            &[(0, SOUTH_ROW)],
            Subject::Zombie,
        ),
        0.5,
        0.0,
        0.7,
    ),
    at(
        scene(
            "zombie touching water to the north",
            &[(0, NORTH_ROW)],
            Subject::Zombie,
        ),
        0.5,
        0.0,
        0.3,
    ),
    at(
        scene(
            "zombie with its head touching water",
            &[(0, DRY), (1, DRY), (2, ALL_S)],
            Subject::Zombie,
        ),
        0.5,
        0.05,
        0.5,
    ),
    scene("zombie on falling water", &[(-1, FALLS)], Subject::Zombie),
    moving(
        scene(
            "player moving in shallow water flowing east",
            &[(0, SHALLOW_EAST)],
            Subject::Player,
        ),
        0.1,
        0.0,
        0.0,
    ),
    moving(
        at(
            scene(
                "player moving across two lava flows",
                &[(0, LAVA_EAST)],
                Subject::Player,
            ),
            0.0,
            0.0,
            0.5,
        ),
        0.1,
        0.0,
        0.0,
    ),
    falling(
        scene("zombie in lava, fall two", &[(0, ALL_L)], Subject::Zombie),
        2.0,
    ),
    moving(
        scene(
            "zombie drifting east at the threshold",
            &[(0, EAST_FLOW)],
            Subject::Zombie,
        ),
        0.003,
        0.0,
        0.0,
    ),
    moving(
        scene(
            "zombie drifting south at the threshold",
            &[(0, EAST_FLOW)],
            Subject::Zombie,
        ),
        0.0,
        0.0,
        0.003,
    ),
    moving(
        scene(
            "zombie drifting south past the threshold",
            &[(0, EAST_FLOW)],
            Subject::Zombie,
        ),
        0.0,
        0.0,
        0.004,
    ),
];

fn flowing<T: FlowingFluid>(
    fluid_impl: &T,
    fluid: &Fluid,
    level: Level,
    falls: bool,
) -> BlockStateId {
    fluid_impl
        .get_flowing(fluid, level, falls)
        .to_state_id(fluid)
}

/// The block state a map letter stands for: `S`/`L` water/lava source, `7`..`1` flowing water
/// of that level (in a map holding an `l`, `1` is flowing lava of level 4), `l` flowing lava of
/// level 6, `F` falling water, `W` a waterlogged slab, `#` stone, anything else air.
fn cell(letter: char, lava_map: bool) -> BlockStateId {
    let water = (&FlowingWater, &Fluid::FLOWING_WATER);
    let level = |digit: u32| match digit {
        7 => Level::L7,
        6 => Level::L6,
        5 => Level::L5,
        4 => Level::L4,
        3 => Level::L3,
        2 => Level::L2,
        _ => Level::L1,
    };
    match letter {
        'S' => Block::WATER.default_state.id,
        'L' => Block::LAVA.default_state.id,
        'l' => flowing(&FlowingLava, &Fluid::FLOWING_LAVA, Level::L6, false),
        '1' if lava_map => flowing(&FlowingLava, &Fluid::FLOWING_LAVA, Level::L4, false),
        'F' => flowing(water.0, water.1, Level::L7, true),
        'W' => {
            let mut slab = WhiteWoolSlabLikeProperties::default(&Block::OAK_SLAB);
            slab.waterlogged = true;
            slab.to_state_id(&Block::OAK_SLAB)
        }
        '#' => Block::STONE.default_state.id,
        digit @ '1'..='7' => flowing(water.0, water.1, level(digit as u32 - '0' as u32), false),
        _ => Block::AIR.default_state.id,
    }
}

fn put(world: &Arc<World>, pos: &BlockPos, state: BlockStateId) {
    world.set_block_state(
        pos,
        state,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

/// Lays the scene's layers around `centre` (feet block), clearing two layers above them.
fn lay(world: &Arc<World>, centre: BlockPos, scene: &Scene) {
    for dy in -1..=3 {
        for dz in -2..=2 {
            for dx in -2..=2 {
                let pos = BlockPos::new(centre.0.x + dx, centre.0.y + dy, centre.0.z + dz);
                let floor = if dy == -1 { Block::STONE } else { Block::AIR };
                put(world, &pos, floor.default_state.id);
            }
        }
    }
    for (dy, rows) in scene.layers {
        let lava_map = rows.iter().any(|row| row.contains('l'));
        for (dz, row) in rows.iter().enumerate() {
            for (dx, letter) in row.chars().enumerate() {
                let pos = BlockPos::new(
                    centre.0.x + dx as i32 - 2,
                    centre.0.y + dy,
                    centre.0.z + dz as i32 - 2,
                );
                put(world, &pos, cell(letter, lava_map));
            }
        }
    }
}

fn subject(world: &Arc<World>, kind: Subject, pos: Vector3<f64>) -> Arc<dyn EntityBase> {
    match kind {
        Subject::Zombie => Arc::new(LivingEntity::new(Entity::new(
            world.clone(),
            pos,
            &EntityType::ZOMBIE,
        ))),
        Subject::Item => Arc::new(ItemEntity::new(
            Entity::new(world.clone(), pos, &EntityType::ITEM),
            ItemStack::new(1, &Item::STICK),
        )),
        Subject::Interaction => {
            InteractionEntity::new(Entity::new(world.clone(), pos, &EntityType::INTERACTION))
        }
        Subject::Player => {
            let player = test_support::java_player(world, GameMode::Survival);
            player.get_entity().set_pos(pos);
            player
        }
    }
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

/// The sound packets queued for the listener, without their random pitch and seed.
fn sounds(tap: &mut PacketTap) -> String {
    let id = CSoundEffect::to_id(CURRENT_MC_VERSION);
    let mut text = String::new();
    for packet in tap.drain() {
        let first = var_int(&packet, 0);
        let is_sound = first.is_some_and(|(value, _)| value == id)
            || first
                .and_then(|(_, next)| var_int(&packet, next))
                .is_some_and(|(value, _)| value == id);
        if is_sound && packet.len() >= 12 {
            text.push_str(" sound ");
            for byte in &packet[..packet.len() - 12] {
                let _ = write!(text, "{byte:02x}");
            }
        }
    }
    text
}

fn state_line(entity: &dyn EntityBase) -> String {
    let base = entity.get_entity();
    let v = base.velocity.load();
    let mut line = format!(
        "v ({:.9}, {:.9}, {:.9}) water {:.9} lava {:.9} touching {} {}",
        v.x,
        v.y,
        v.z,
        base.water_height.load(),
        base.lava_height.load(),
        base.touching_water.load(Ordering::SeqCst),
        base.touching_lava.load(Ordering::SeqCst),
    );
    if let Some(living) = entity.get_living_entity() {
        let _ = write!(line, " fall {:.3}", living.fall_distance.load());
    }
    line
}

fn run(out: &mut String, world: &Arc<World>, tap: &mut PacketTap, index: usize, scene: &Scene) {
    let centre = BlockPos::new(index as i32 * 16 + 8, 64, 8);
    lay(world, centre, scene);
    let (x, y, z) = scene.at;
    let pos = Vector3::new(
        f64::from(centre.0.x) + x,
        f64::from(centre.0.y) + y,
        f64::from(centre.0.z) + z,
    );
    let entity = subject(world, scene.subject, pos);
    let base = entity.get_entity();
    let (vx, vy, vz) = scene.velocity;
    base.velocity.store(Vector3::new(vx, vy, vz));
    base.touching_water
        .store(scene.touching_water, Ordering::SeqCst);
    base.first_tick.store(scene.first_tick, Ordering::Relaxed);
    if let Some(living) = entity.get_living_entity() {
        living.fall_distance.store(scene.fall);
    }
    drop(sounds(tap));
    for call in 1..=2 {
        let fluids: Vec<&str> = base
            .update_fluid_interaction(entity.as_ref())
            .iter()
            .map(|fluid| fluid.name)
            .collect();
        let _ = writeln!(
            out,
            "{}: call {call} fluids {fluids:?} {}{}",
            scene.name,
            state_line(entity.as_ref()),
            sounds(tap)
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn fluid_interaction_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let worlds = server.worlds.load();
    let overworld = worlds[0].clone();
    let nether = worlds
        .iter()
        .find(|world| world.dimension.fast_lava)
        .ok_or("no fast-lava dimension")?
        .clone();
    let mut out = String::new();
    for world in [&overworld, &nether] {
        for cx in -1..=SCENES.len() as i32 {
            for cz in -1..=1 {
                world.level.loaded_chunks.insert(
                    Vector2::new(cx, cz),
                    pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
                );
            }
        }
    }
    for world in [&overworld, &nether] {
        let (listener, mut tap) =
            test_support::java_player_with_tap(world, GameMode::Survival).ok_or("no packet tap")?;
        world.add_player(&listener)?;
        for (index, scene) in SCENES.iter().enumerate() {
            if scene.nether != Arc::ptr_eq(world, &nether) {
                continue;
            }
            let centre = Vector3::new(index as f64 * 16.0 + 8.5, 70.0, 8.5);
            listener.get_entity().set_pos(centre);
            run(&mut out, world, &mut tap, index, scene);
        }
        world.remove_player(&listener, false).await;
    }
    server.stop();
    script_golden::bless_or_compare("fluid_interaction.txt", &out)
}
