//! A record of what `AxeItem::use_on_block` does, made before the function was split
//! (`tests/golden/axe_use_on_block.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene lays blocks into a loaded area of a real server's world (the clicked block at
//! offset 0,0,0), puts a fresh user next to it and calls the item's `use_on_block` with the
//! scene's stack, game mode and face, with a sculk sensor four blocks north that listens for the
//! block change (`heard N` = ticks until it reacted). One line per scene: the result, the non-air blocks
//! within two blocks sideways and one below / two above, the stack afterwards and the user's
//! packets (length and id; the sound seed is random).
//! Scenes: every block the axe transformer names (default state), copper doors (both halves,
//! other half equal / missing / another block), copper chests (every type and facing, the
//! partner on either side, of the same or another kind), creative mode, a worn axe, other faces,
//! a stack without a transformer and a block the axe leaves alone.

use std::fmt::Write as _;
use std::num::NonZero;
use std::sync::Arc;

use pumpkin_data::block_properties::{
    ChestLikeProperties, ChestType, DoorHinge, DoubleBlockHalf, HorizontalFacing,
    OakDoorLikeProperties, SculkSensorLikeProperties, SculkSensorPhase,
};
use pumpkin_data::block_transformer::{AXE, BlockPredicate};
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::{Block, BlockDirection, BlockStateId};
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use super::AxeItem;
use crate::block::registry::BlockActionResult;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::EntityBase;
use crate::entity::ai::test_support::{self, Identity};
use crate::item::ItemBehaviour;
use crate::net::java::PacketTap;
use crate::server::Server;
use crate::world::World;

const USER: Identity = Identity {
    name: "Axeman",
    uuid: Uuid::from_u128(0x0000_00a1),
    entity_id: 9_401,
};

struct Scene {
    name: String,
    item: &'static Item,
    gamemode: GameMode,
    worn: bool,
    face: BlockDirection,
    /// Blocks to lay, as offsets from the clicked block; the first one is the clicked block.
    laid: Vec<([i32; 3], BlockStateId)>,
}

impl Scene {
    fn new(name: impl Into<String>, laid: Vec<([i32; 3], BlockStateId)>) -> Self {
        Self {
            name: name.into(),
            item: &Item::IRON_AXE,
            gamemode: GameMode::Survival,
            worn: false,
            face: BlockDirection::Up,
            laid,
        }
    }

    fn click(&self) -> &'static Block {
        Block::from_state_id(self.laid[0].1)
    }
}

fn state(block: &Block) -> BlockStateId {
    block.default_state.id
}

fn door(block: &Block, half: DoubleBlockHalf, other_props: bool) -> BlockStateId {
    OakDoorLikeProperties {
        facing: if other_props {
            HorizontalFacing::East
        } else {
            HorizontalFacing::North
        },
        half,
        hinge: if other_props {
            DoorHinge::Right
        } else {
            DoorHinge::Left
        },
        open: other_props,
        powered: other_props,
    }
    .to_state_id(block)
}

fn chest(block: &Block, facing: HorizontalFacing, r#type: ChestType) -> BlockStateId {
    ChestLikeProperties {
        facing,
        r#type,
        waterlogged: false,
    }
    .to_state_id(block)
}

/// Every block the transformer names as a source at the clicked position.
fn source_blocks() -> Vec<&'static Block> {
    let mut found: Vec<&'static Block> = Vec::new();
    for entry in AXE.entries {
        for rule in entry.rules {
            if let BlockPredicate::MatchingBlocks { blocks, .. } = rule.predicate {
                for id in blocks {
                    let block = id.to_block();
                    if !found.contains(&block) {
                        found.push(block);
                    }
                }
            }
        }
    }
    found.sort_by_key(|block| block.name);
    found
}

const DOORS: [&Block; 2] = [
    &Block::EXPOSED_COPPER_DOOR,
    &Block::WAXED_OXIDIZED_COPPER_DOOR,
];
const CHESTS: [&Block; 2] = [
    &Block::EXPOSED_COPPER_CHEST,
    &Block::WAXED_OXIDIZED_COPPER_CHEST,
];
const FACINGS: [HorizontalFacing; 4] = [
    HorizontalFacing::North,
    HorizontalFacing::East,
    HorizontalFacing::South,
    HorizontalFacing::West,
];

/// Offset one step along `facing`, clockwise (seen from above) when `clockwise`.
const fn side(facing: HorizontalFacing, clockwise: bool) -> [i32; 3] {
    let offsets = [[1, 0, 0], [0, 0, 1], [-1, 0, 0], [0, 0, -1]];
    let at = match facing {
        HorizontalFacing::North => 0,
        HorizontalFacing::East => 1,
        HorizontalFacing::South => 2,
        HorizontalFacing::West => 3,
    };
    offsets[if clockwise { at } else { (at + 2) % 4 }]
}

fn scenes() -> Vec<Scene> {
    let mut scenes = Vec::new();
    for block in source_blocks() {
        scenes.push(Scene::new(
            format!("axe on {}", block.name),
            vec![([0, 0, 0], state(block))],
        ));
    }
    let log = state(&Block::OAK_LOG);
    scenes.push(Scene {
        gamemode: GameMode::Creative,
        ..Scene::new("creative axe on an oak log", vec![([0, 0, 0], log)])
    });
    scenes.push(Scene {
        gamemode: GameMode::Adventure,
        ..Scene::new("adventure axe on an oak log", vec![([0, 0, 0], log)])
    });
    scenes.push(Scene {
        worn: true,
        ..Scene::new("worn axe on an oak log", vec![([0, 0, 0], log)])
    });
    for (face, name) in [
        (BlockDirection::Down, "down"),
        (BlockDirection::North, "north"),
        (BlockDirection::East, "east"),
    ] {
        scenes.push(Scene {
            face,
            ..Scene::new(
                format!("axe on an oak log, {name} face"),
                vec![([0, 0, 0], log)],
            )
        });
    }
    scenes.push(Scene {
        item: &Item::STICK,
        ..Scene::new("a stick on an oak log", vec![([0, 0, 0], log)])
    });
    scenes.push(Scene::new(
        "axe on stone",
        vec![([0, 0, 0], state(&Block::STONE))],
    ));
    scenes.push(Scene::new(
        "axe on a normal oak door",
        vec![
            (
                [0, 0, 0],
                door(&Block::OAK_DOOR, DoubleBlockHalf::Lower, false),
            ),
            (
                [0, 1, 0],
                door(&Block::OAK_DOOR, DoubleBlockHalf::Upper, false),
            ),
        ],
    ));
    door_scenes(&mut scenes);
    chest_scenes(&mut scenes);
    scenes
}

fn door_scenes(scenes: &mut Vec<Scene>) {
    for block in DOORS {
        for (half, other_at, half_name) in [
            (DoubleBlockHalf::Lower, [0, 1, 0], "lower"),
            (DoubleBlockHalf::Upper, [0, -1, 0], "upper"),
        ] {
            let other_half = match half {
                DoubleBlockHalf::Lower => DoubleBlockHalf::Upper,
                DoubleBlockHalf::Upper => DoubleBlockHalf::Lower,
            };
            let clicked = ([0, 0, 0], door(block, half, false));
            let variants = [
                ("with the other half", Some(door(block, other_half, true))),
                ("alone", None),
            ];
            for (what, other) in variants {
                let mut laid = vec![clicked];
                laid.extend(other.map(|other| (other_at, other)));
                scenes.push(Scene::new(
                    format!("axe on the {half_name} half of {} {what}", block.name),
                    laid,
                ));
            }
        }
    }
}

fn chest_scenes(scenes: &mut Vec<Scene>) {
    for block in CHESTS {
        let other_kind = if block == &Block::EXPOSED_COPPER_CHEST {
            &Block::WEATHERED_COPPER_CHEST
        } else {
            &Block::WAXED_EXPOSED_COPPER_CHEST
        };
        for facing in FACINGS {
            for r#type in [ChestType::Single, ChestType::Left, ChestType::Right] {
                let partner_type = match r#type {
                    ChestType::Left => ChestType::Right,
                    ChestType::Right | ChestType::Single => ChestType::Left,
                };
                let clicked = ([0, 0, 0], chest(block, facing, r#type));
                for (clockwise, side_name) in [(true, "clockwise"), (false, "counter-clockwise")] {
                    let at = side(facing, clockwise);
                    for (what, partner) in [
                        ("same kind", chest(block, facing, partner_type)),
                        ("other kind", chest(other_kind, facing, partner_type)),
                    ] {
                        if r#type == ChestType::Single && !clockwise && what == "other kind" {
                            continue;
                        }
                        scenes.push(Scene::new(
                            format!(
                                "axe on a {type:?} {} facing {facing:?}, {what} {side_name}",
                                block.name
                            ),
                            vec![clicked, (at, partner)],
                        ));
                    }
                }
                scenes.push(Scene::new(
                    format!("axe on a {type:?} {} facing {facing:?}, alone", block.name),
                    vec![clicked],
                ));
            }
        }
    }
}

const SPACING: i32 = 8;

const fn position_of(index: usize) -> BlockPos {
    BlockPos::new(index as i32 * SPACING + 200, 64, 8)
}

fn put(world: &Arc<World>, pos: &BlockPos, state: BlockStateId) {
    world.set_block_state(
        pos,
        state,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

fn result_text(result: &BlockActionResult) -> &'static str {
    match result {
        BlockActionResult::Success => "Success",
        BlockActionResult::SuccessServer => "SuccessServer",
        BlockActionResult::Consume => "Consume",
        BlockActionResult::Fail => "Fail",
        BlockActionResult::Pass => "Pass",
        BlockActionResult::PassToDefaultBlockAction => "PassToDefaultBlockAction",
    }
}

fn stack_text(stack: &ItemStack) -> String {
    if stack.is_empty() {
        return "-".to_string();
    }
    format!(
        "{}x{} dmg {}",
        stack.item.registry_key,
        stack.item_count,
        stack.get_damage()
    )
}

/// Packet ids whose bytes are random like in vanilla: the sound packet (seed).
const RANDOM_PACKETS: [u8; 1] = [119];

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn packets(tap: &mut PacketTap) -> String {
    tap.drain()
        .iter()
        .map(|p| match p.first() {
            Some(id) if RANDOM_PACKETS.contains(id) => format!("{}:id{id}", p.len()),
            _ => format!("{}:{:08x}", p.len(), fnv(p) as u32),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Ticks the sensor's block entity until it is no longer inactive (at most 40 ticks).
fn heard(world: &Arc<World>, sensor: BlockPos) -> String {
    for tick in 1..=40 {
        world
            .level_time
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .world_age += 1;
        if let Some(entity) = world.get_block_entity(&sensor) {
            entity.tick(world);
        }
        let id = world.get_block_state(&sensor).id;
        if SculkSensorLikeProperties::from_state_id(id).sculk_sensor_phase
            != SculkSensorPhase::Inactive
        {
            return format!("heard {tick}");
        }
    }
    "deaf".to_string()
}

fn region_text(world: &World, pos: BlockPos) -> String {
    let mut text = String::new();
    for dx in -2..=2 {
        for dy in -1..=2 {
            for dz in -2..=2 {
                let at = BlockPos::new(pos.0.x + dx, pos.0.y + dy, pos.0.z + dz);
                let block = world.get_block(&at);
                if block != &Block::AIR {
                    let _ = write!(
                        text,
                        " {dx},{dy},{dz}:{}#{}",
                        block.name,
                        world.get_block_state_id(&at).as_u16()
                    );
                }
            }
        }
    }
    text
}

fn run(
    server: &Arc<Server>,
    world: &Arc<World>,
    index: usize,
    scene: &Scene,
) -> Result<String, String> {
    let pos = position_of(index);
    for (offset, laid) in &scene.laid {
        put(
            world,
            &BlockPos::new(
                pos.0.x + offset[0],
                pos.0.y + offset[1],
                pos.0.z + offset[2],
            ),
            *laid,
        );
    }
    let sensor = BlockPos::new(pos.0.x, pos.0.y, pos.0.z - 4);
    world.set_block_state(&sensor, state(&Block::SCULK_SENSOR), BlockFlags::NOTIFY_ALL);
    let (user, mut tap) =
        test_support::java_player_as(world, scene.gamemode, &USER).ok_or("no Java packet tap")?;
    user.get_entity().set_pos(Vector3::new(
        f64::from(pos.0.x) + 0.5,
        f64::from(pos.0.y),
        f64::from(pos.0.z) + 2.5,
    ));
    user.watched_section.store(Cylindrical::new(
        pos.chunk_position(),
        NonZero::<u8>::MIN.saturating_add(1),
    ));
    world.add_player(&user)?;
    drop(packets(&mut tap));
    let mut stack = ItemStack::new(1, scene.item);
    if scene.worn
        && let Some(max) = stack.get_max_damage()
    {
        stack.set_damage(max - 1);
    }
    let result = AxeItem.use_on_block(
        &mut stack,
        &user,
        pos,
        scene.face,
        Vector3::new(0.5, 0.5, 0.5),
        scene.click(),
        server,
    );
    let sent = packets(&mut tap);
    Ok(format!(
        "{}: {} | stack {} | {} | sent [{sent}] |{}\n",
        scene.name,
        result_text(&result),
        stack_text(&stack),
        heard(world, sensor),
        region_text(world, pos),
    ))
}

#[tokio::test(flavor = "multi_thread")]
async fn axe_use_on_block_is_the_same() -> TestResult {
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
        out.push_str(&run(&server, &world, index, scene)?);
        for player in world.players.load().iter() {
            world.remove_player(player, false).await;
        }
    }
    server.stop();
    script_golden::bless_or_compare("axe_use_on_block.txt", &out)
}
