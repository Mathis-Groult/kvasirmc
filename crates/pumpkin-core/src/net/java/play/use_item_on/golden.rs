//! A record of what `JavaClient::handle_use_item_on` does, made before the function was split
//! (`tests/golden/use_item_on.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene lays one block on a stone floor in a loaded area of a real server's world, puts a
//! fresh user (fixed identity, two blocks away, the scene's game mode, items, sneaking) and a
//! fresh watcher next to it, then hands the user's client one use-item-on packet (hand, face,
//! cursor height). One line per scene: the result, the clicked block, the blocks above and north
//! of it, the items and entities lying around, the user's packet sequence, held and offhand
//! items, `Used`/`Broken` statistics, food level, open screen and the advancements whose
//! progress changed, then the packets of the user and the watcher (length and FNV-1a of the
//! bytes; random packets by id).
//! Not covered: plugins cancelling the interact event (none is loaded).

use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use pumpkin_data::block_properties::{
    AttachFace, BeeNestLikeProperties, CopperBulbLikeProperties, HorizontalFacing,
    LeverLikeProperties, NetherWartLikeProperties, RespawnAnchorLikeProperties,
};
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::statistic::CustomStatistic;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::java::server::play::SUseItemOn;
use pumpkin_util::GameMode;
use pumpkin_util::Hand;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use crate::block::entities::BlockEntity;
use crate::block::entities::chest::ChestBlockEntity;
use crate::block::entities::sign::SignBlockEntity;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::EntityBase;
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::player::Player;
use crate::entity::player::statistics::StatisticCategory;
use crate::net::ClientPlatform;
use crate::net::java::PacketTap;
use crate::server::Server;
use crate::world::World;

const USER: Identity = Identity {
    name: "User",
    uuid: Uuid::from_u128(0x0000_00f1),
    entity_id: 9_301,
};
const WATCHER: Identity = Identity {
    name: "Watcher",
    uuid: Uuid::from_u128(0x0000_00f2),
    entity_id: 9_302,
};

const UP: i32 = 1;
const NORTH: i32 = 2;
const DOWN: i32 = 0;
const RIGHT: i32 = 0;
const LEFT: i32 = 1;

#[derive(Clone, Copy)]
enum Kind {
    Stone,
    Air,
    Lever,
    Chest,
    Dirt,
    GrassBlock,
    OakLog,
    LitBulb,
    UnlitBulb,
    Copper,
    Anchor,
    Beehive,
    Cake,
    NoteBlock,
    Tnt,
    ShortGrass,
    Sign,
    RipeBerryBush,
    Wheat,
}

#[derive(Clone, Copy)]
struct Scene {
    name: &'static str,
    kind: Kind,
    gamemode: GameMode,
    held: Option<(&'static Item, u8)>,
    offhand: Option<(&'static Item, u8)>,
    /// The held (or, with nothing held, offhand) tool has one use left.
    worn: bool,
    hand: i32,
    face: i32,
    cursor_y: f32,
    sneaking: bool,
    /// The client has not sent `PlayerLoaded`.
    unloaded: bool,
    /// The user stands eight blocks away (out of reach).
    far: bool,
    /// The user stands just inside reach only with the 1.0 margin.
    margin: bool,
    /// The user stands where a block placed on top would go.
    in_the_way: bool,
    /// Height of the clicked block.
    height: i32,
    /// A campfire two blocks under the clicked block, air between.
    smoke: bool,
    /// Appended: the clicked block gets its block entity (chest, sign).
    block_entity: bool,
    /// Appended: the user's food level is 10.
    hungry: bool,
    /// Appended: blocks and items by name only (random growth and drop counts).
    names_only: bool,
    /// Appended scenes end their line with whether the packet counted as activity and the
    /// chest, cake and note block statistics.
    extra: bool,
}

const SCENE: Scene = Scene {
    name: "",
    kind: Kind::Stone,
    gamemode: GameMode::Survival,
    held: None,
    offhand: None,
    worn: false,
    hand: RIGHT,
    face: UP,
    cursor_y: 0.5,
    sneaking: false,
    unloaded: false,
    far: false,
    margin: false,
    in_the_way: false,
    height: 64,
    smoke: false,
    block_entity: false,
    hungry: false,
    names_only: false,
    extra: false,
};

const fn scene(name: &'static str, kind: Kind) -> Scene {
    Scene {
        name,
        kind,
        ..SCENE
    }
}

const STONE: Option<(&Item, u8)> = Some((&Item::STONE, 5));

fn scenes() -> Vec<Scene> {
    let mut scenes = guard_scenes();
    scenes.extend(block_use_scenes());
    scenes.extend(placing_scenes());
    scenes.extend(item_use_scenes());
    scenes.extend(widened_scenes());
    scenes
}

const WIDE: Scene = Scene {
    extra: true,
    ..SCENE
};

const fn wide(name: &'static str, kind: Kind) -> Scene {
    Scene { name, kind, ..WIDE }
}

/// Scenes appended in the first widening.
fn widened_scenes() -> Vec<Scene> {
    vec![
        Scene {
            block_entity: true,
            ..wide("empty hand on a chest with its block entity", Kind::Chest)
        },
        Scene {
            gamemode: GameMode::Spectator,
            block_entity: true,
            ..wide("spectator on a chest with its block entity", Kind::Chest)
        },
        Scene {
            hungry: true,
            ..wide("empty hand on a cake, hungry", Kind::Cake)
        },
        wide("empty hand on a note block, statistics", Kind::NoteBlock),
        wide("holding stone on stone, activity", Kind::Stone),
        Scene {
            block_entity: true,
            ..wide("empty hand on a sign with its block entity", Kind::Sign)
        },
        Scene {
            block_entity: true,
            held: Some((&Item::GLOW_INK_SAC, 2)),
            ..wide("glow ink sac on a sign with its block entity", Kind::Sign)
        },
        Scene {
            names_only: true,
            ..wide("empty hand on a ripe sweet berry bush", Kind::RipeBerryBush)
        },
        Scene {
            names_only: true,
            held: Some((&Item::BONE_MEAL, 3)),
            ..wide("bone meal on wheat", Kind::Wheat)
        },
        Scene {
            gamemode: GameMode::Creative,
            held: Some((&Item::IRON_AXE, 1)),
            ..wide(
                "axe on a lit exposed copper bulb in creative",
                Kind::LitBulb,
            )
        },
        Scene {
            offhand: Some((&Item::BONE_MEAL, 3)),
            hand: LEFT,
            names_only: true,
            ..wide("offhand bone meal on wheat", Kind::Wheat)
        },
    ]
}

fn guard_scenes() -> Vec<Scene> {
    vec![
        Scene {
            unloaded: true,
            held: STONE,
            ..scene("client not loaded", Kind::Stone)
        },
        Scene {
            far: true,
            held: STONE,
            ..scene("out of reach", Kind::Stone)
        },
        Scene {
            margin: true,
            held: STONE,
            ..scene("just inside the reach margin", Kind::Stone)
        },
        Scene {
            face: 9,
            held: STONE,
            ..scene("invalid face", Kind::Stone)
        },
        Scene {
            hand: 7,
            held: STONE,
            ..scene("invalid hand", Kind::Stone)
        },
        Scene {
            gamemode: GameMode::Spectator,
            ..scene("spectator on a chest", Kind::Chest)
        },
        Scene {
            gamemode: GameMode::Spectator,
            held: STONE,
            ..scene("spectator on stone holding stone", Kind::Stone)
        },
        Scene {
            gamemode: GameMode::Spectator,
            ..scene("spectator on a lever", Kind::Lever)
        },
    ]
}

fn block_use_scenes() -> Vec<Scene> {
    vec![
        scene("empty hand on a lever", Kind::Lever),
        scene("empty hand on stone", Kind::Stone),
        scene("empty hand on a chest", Kind::Chest),
        Scene {
            held: STONE,
            ..scene("holding stone on a lever", Kind::Lever)
        },
        Scene {
            held: STONE,
            sneaking: true,
            ..scene("sneaking with stone on a lever", Kind::Lever)
        },
        Scene {
            sneaking: true,
            ..scene("sneaking with empty hands on a lever", Kind::Lever)
        },
        Scene {
            offhand: STONE,
            sneaking: true,
            ..scene(
                "sneaking, stone in the offhand only, on a lever",
                Kind::Lever,
            )
        },
        Scene {
            held: STONE,
            ..scene("holding stone on a chest", Kind::Chest)
        },
        scene("empty hand on a cake", Kind::Cake),
        scene("empty hand on a note block", Kind::NoteBlock),
        Scene {
            held: Some((&Item::GLOWSTONE, 3)),
            ..scene("glowstone on a respawn anchor with 3 charges", Kind::Anchor)
        },
        Scene {
            held: Some((&Item::IRON_AXE, 1)),
            ..scene("axe on a lit exposed copper bulb", Kind::LitBulb)
        },
        Scene {
            held: Some((&Item::IRON_AXE, 1)),
            ..scene("axe on an unlit exposed copper bulb", Kind::UnlitBulb)
        },
        Scene {
            held: Some((&Item::HONEYCOMB, 4)),
            ..scene("honeycomb on a copper block", Kind::Copper)
        },
        Scene {
            held: Some((&Item::FLINT_AND_STEEL, 1)),
            ..scene("flint and steel on tnt", Kind::Tnt)
        },
        Scene {
            held: Some((&Item::SHEARS, 1)),
            smoke: true,
            ..scene("shears on a full beehive over a campfire", Kind::Beehive)
        },
        Scene {
            held: Some((&Item::GLASS_BOTTLE, 2)),
            smoke: true,
            ..scene(
                "glass bottle on a full beehive over a campfire",
                Kind::Beehive,
            )
        },
    ]
}

fn placing_scenes() -> Vec<Scene> {
    vec![
        Scene {
            held: STONE,
            ..scene("place stone on top", Kind::Stone)
        },
        Scene {
            held: Some((&Item::STONE, 1)),
            ..scene("place the last stone", Kind::Stone)
        },
        Scene {
            gamemode: GameMode::Creative,
            held: STONE,
            ..scene("place stone in creative", Kind::Stone)
        },
        Scene {
            gamemode: GameMode::Adventure,
            held: STONE,
            ..scene("place stone in adventure", Kind::Stone)
        },
        Scene {
            offhand: STONE,
            hand: LEFT,
            ..scene("place stone from the offhand", Kind::Stone)
        },
        Scene {
            held: Some((&Item::APPLE, 2)),
            offhand: STONE,
            hand: LEFT,
            ..scene("place stone from the offhand, apple held", Kind::Stone)
        },
        Scene {
            held: STONE,
            height: 319,
            ..scene("place on top of the world", Kind::Stone)
        },
        Scene {
            held: STONE,
            height: -64,
            face: DOWN,
            ..scene("place under the bottom of the world", Kind::Stone)
        },
        Scene {
            held: STONE,
            in_the_way: true,
            ..scene("place where the user stands", Kind::Stone)
        },
        Scene {
            held: STONE,
            ..scene("place stone on short grass", Kind::ShortGrass)
        },
        Scene {
            held: Some((&Item::TORCH, 3)),
            face: NORTH,
            ..scene("place a torch on the north face", Kind::Stone)
        },
        Scene {
            held: Some((&Item::STONE_SLAB, 3)),
            face: NORTH,
            cursor_y: 0.8,
            ..scene("place a slab high on the north face", Kind::Stone)
        },
        Scene {
            held: Some((&Item::STONE_SLAB, 3)),
            face: NORTH,
            cursor_y: 0.2,
            ..scene("place a slab low on the north face", Kind::Stone)
        },
        Scene {
            held: STONE,
            ..scene("place stone against air", Kind::Air)
        },
    ]
}

fn item_use_scenes() -> Vec<Scene> {
    vec![
        Scene {
            held: Some((&Item::IRON_HOE, 1)),
            ..scene("hoe on dirt", Kind::Dirt)
        },
        Scene {
            held: Some((&Item::IRON_HOE, 1)),
            worn: true,
            ..scene("worn hoe on dirt (breaks)", Kind::Dirt)
        },
        Scene {
            offhand: Some((&Item::IRON_HOE, 1)),
            worn: true,
            hand: LEFT,
            ..scene("worn hoe in the offhand on dirt (breaks)", Kind::Dirt)
        },
        Scene {
            gamemode: GameMode::Creative,
            held: Some((&Item::IRON_HOE, 1)),
            worn: true,
            ..scene("worn hoe on dirt in creative", Kind::Dirt)
        },
        Scene {
            held: Some((&Item::IRON_HOE, 1)),
            ..scene("hoe on stone", Kind::Stone)
        },
        Scene {
            held: Some((&Item::IRON_SHOVEL, 1)),
            ..scene("shovel on a grass block", Kind::GrassBlock)
        },
        Scene {
            held: Some((&Item::IRON_AXE, 1)),
            ..scene("axe on an oak log", Kind::OakLog)
        },
        Scene {
            held: Some((&Item::FLINT_AND_STEEL, 1)),
            ..scene("flint and steel on stone", Kind::Stone)
        },
        Scene {
            held: Some((&Item::WATER_BUCKET, 1)),
            ..scene("water bucket on stone", Kind::Stone)
        },
        Scene {
            held: Some((&Item::BONE_MEAL, 3)),
            ..scene("bone meal on stone", Kind::Stone)
        },
        Scene {
            held: Some((&Item::APPLE, 3)),
            ..scene("apple on stone", Kind::Stone)
        },
        Scene {
            held: Some((&Item::IRON_HOE, 1)),
            sneaking: true,
            ..scene("sneaking with a hoe on dirt", Kind::Dirt)
        },
    ]
}

const fn position_of(index: usize, height: i32) -> BlockPos {
    BlockPos::new(index as i32 * 16 + 200, height, 8)
}

fn put(world: &Arc<World>, pos: &BlockPos, state: BlockStateId) {
    world.set_block_state(
        pos,
        state,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

fn bulb(lit: bool) -> BlockStateId {
    CopperBulbLikeProperties {
        lit,
        powered: false,
    }
    .to_state_id(&Block::EXPOSED_COPPER_BULB)
}

fn state_of(kind: Kind) -> BlockStateId {
    match kind {
        Kind::Stone => Block::STONE.default_state.id,
        Kind::Air => Block::AIR.default_state.id,
        Kind::Lever => LeverLikeProperties {
            face: AttachFace::Floor,
            facing: HorizontalFacing::North,
            powered: false,
        }
        .to_state_id(&Block::LEVER),
        Kind::Chest => Block::CHEST.default_state.id,
        Kind::Dirt => Block::DIRT.default_state.id,
        Kind::GrassBlock => Block::GRASS_BLOCK.default_state.id,
        Kind::OakLog => Block::OAK_LOG.default_state.id,
        Kind::LitBulb => bulb(true),
        Kind::UnlitBulb => bulb(false),
        Kind::Copper => Block::COPPER_BLOCK.default_state.id,
        Kind::Anchor => {
            RespawnAnchorLikeProperties { charges: 3 }.to_state_id(&Block::RESPAWN_ANCHOR)
        }
        Kind::Beehive => {
            let mut hive = BeeNestLikeProperties::default(&Block::BEEHIVE);
            hive.honey_level = 5;
            hive.to_state_id(&Block::BEEHIVE)
        }
        Kind::Cake => Block::CAKE.default_state.id,
        Kind::NoteBlock => Block::NOTE_BLOCK.default_state.id,
        Kind::Tnt => Block::TNT.default_state.id,
        Kind::ShortGrass => Block::SHORT_GRASS.default_state.id,
        Kind::Sign => Block::OAK_SIGN.default_state.id,
        Kind::RipeBerryBush => {
            NetherWartLikeProperties { age: 3 }.to_state_id(&Block::SWEET_BERRY_BUSH)
        }
        Kind::Wheat => Block::WHEAT.default_state.id,
    }
}

/// Lays the stone floor, the campfire of a smoke scene, then the clicked block last.
fn lay(world: &Arc<World>, pos: BlockPos, scene: &Scene) {
    for x in -3..=9 {
        for z in -3..=3 {
            put(
                world,
                &BlockPos::new(pos.0.x + x, pos.0.y - 1, pos.0.z + z),
                Block::STONE.default_state.id,
            );
        }
    }
    if scene.smoke {
        put(world, &pos.down().down(), Block::CAMPFIRE.default_state.id);
        put(world, &pos.down(), Block::AIR.default_state.id);
    }
    put(world, &pos, state_of(scene.kind));
    if scene.block_entity {
        let entity: Arc<dyn BlockEntity> = match scene.kind {
            Kind::Sign => Arc::new(SignBlockEntity::empty(pos)),
            _ => Arc::new(ChestBlockEntity::new(pos)),
        };
        world.add_block_entity(entity);
    }
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Packet ids whose bytes are random like in vanilla: Add Entity (UUID, TNT velocity) and the
/// two sound packets (seed).
const RANDOM_PACKETS: [u8; 3] = [1, 118, 119];

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

/// Like [`packets`], every packet by id only (scenes with random growth or drops).
fn packet_ids(tap: &mut PacketTap) -> String {
    tap.drain()
        .iter()
        .map(|p| format!("{}:id{}", p.len(), p.first().copied().unwrap_or_default()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn tap_text(tap: &mut PacketTap, names_only: bool) -> String {
    if names_only {
        packet_ids(tap)
    } else {
        packets(tap)
    }
}

fn near(at: Vector3<f64>, pos: BlockPos) -> bool {
    (at.x - f64::from(pos.0.x)).abs() < 6.0 && (at.z - f64::from(pos.0.z)).abs() < 6.0
}

/// Items (name x count, or the name only) and other non-player entities (type) within six blocks
/// of `pos`, sorted.
fn things_near(world: &World, pos: BlockPos, names_only: bool) -> Vec<String> {
    let mut found: Vec<String> = world
        .entities
        .load()
        .iter()
        .filter(|entity| near(entity.get_entity().pos.load(), pos))
        .map(|entity| {
            entity.get_item_entity().map_or_else(
                || entity.get_entity().entity_type.resource_name.to_string(),
                |item| {
                    let stack = item
                        .get_item_stack()
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone();
                    if names_only {
                        return stack.item.registry_key.to_string();
                    }
                    format!("{}x{}", stack.item.registry_key, stack.item_count)
                },
            )
        })
        .collect();
    found.sort();
    found
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

fn block_text(world: &World, pos: &BlockPos, names_only: bool) -> String {
    if names_only {
        return world.get_block(pos).name.to_string();
    }
    format!(
        "{}#{}",
        world.get_block(pos).name,
        world.get_block_state_id(pos).as_u16()
    )
}

fn java(player: &Player) -> Option<&crate::net::java::JavaClient> {
    match player.client.as_ref() {
        ClientPlatform::Java(client) => Some(client),
        ClientPlatform::Bedrock(_) => None,
    }
}

fn user_text(player: &Player, used: &Item) -> String {
    let inventory = player.inventory();
    let stats = player
        .stats
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut changed: Vec<String> = player
        .advancements
        .try_lock()
        .map(|advancements| {
            advancements
                .progress_changed
                .iter()
                .map(|advancement| advancement.id.to_string())
                .collect()
        })
        .unwrap_or_default();
    changed.sort();
    format!(
        "sequence {} held {} offhand {} slot {} used {} broken {} food {} screen {} advancements {changed:?}",
        java(player).map_or(-2, |client| client.packet_sequence.load(Ordering::Relaxed)),
        stack_text(&inventory.held_item()),
        stack_text(&inventory.off_hand_item()),
        inventory.get_selected_slot(),
        stats.get(StatisticCategory::Used, used.id as i32),
        stats.get(StatisticCategory::Broken, used.id as i32),
        player.hunger_manager.level.load(),
        player.screen_handler_sync_id.load(Ordering::Relaxed),
    )
}

/// Appended scenes: whether the packet counted as activity, and the custom statistics of the
/// blocks they use.
fn extra_text(player: &Player) -> String {
    let stats = player
        .stats
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let custom = |stat: CustomStatistic| stats.get(StatisticCategory::Custom, stat as i32);
    format!(
        " | acted {} open chest {} eat cake {} tune note block {}",
        player.last_action_time.load().elapsed() < Duration::from_secs(300),
        custom(CustomStatistic::OpenChest),
        custom(CustomStatistic::EatCakeSlice),
        custom(CustomStatistic::TuneNoteblock),
    )
}

fn place(player: &Player, at: Vector3<f64>, pos: BlockPos) {
    player.get_entity().set_pos(at);
    player.watched_section.store(Cylindrical::new(
        pos.chunk_position(),
        NonZero::<u8>::MIN.saturating_add(1),
    ));
}

fn worn(item: &'static Item, count: u8, worn: bool) -> ItemStack {
    let mut stack = ItemStack::new(count, item);
    if worn && let Some(max) = stack.get_max_damage() {
        stack.set_damage(max - 1);
    }
    stack
}

/// Gives the user the scene's items, sneaking and recorded advancements.
fn equip(user: &Arc<Player>, scene: &Scene) {
    let inventory = user.inventory();
    if let Some((item, count)) = scene.held {
        inventory.set_stack_in_hand(Hand::Right, worn(item, count, scene.worn));
    }
    if let Some((item, count)) = scene.offhand {
        let worn_offhand = scene.worn && scene.held.is_none();
        inventory.set_stack_in_hand(Hand::Left, worn(item, count, worn_offhand));
    }
    if scene.sneaking {
        user.get_entity().set_sneaking(true);
    }
    if scene.hungry {
        user.hunger_manager.level.store(10);
    }
    if scene.extra
        && let Some(long_ago) = Instant::now().checked_sub(Duration::from_secs(600))
    {
        user.last_action_time.store(long_ago);
    }
    // As the server's login does, so that awarded criteria are recorded.
    user.advancements
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .player = Arc::downgrade(user);
}

fn user_at(scene: &Scene, pos: BlockPos) -> Vector3<f64> {
    let x = f64::from(pos.0.x) + 0.5;
    let y = f64::from(pos.0.y);
    let z = f64::from(pos.0.z) + 0.5;
    if scene.in_the_way {
        return Vector3::new(x, y + 1.0, z);
    }
    let away = if scene.far {
        8.0
    } else if scene.margin {
        5.5
    } else {
        2.0
    };
    Vector3::new(x, y, z + away)
}

fn run(
    server: &Arc<Server>,
    world: &Arc<World>,
    index: usize,
    scene: &Scene,
) -> Result<String, String> {
    let pos = position_of(index, scene.height);
    lay(world, pos, scene);
    let (user, mut user_tap) =
        test_support::java_player_as(world, scene.gamemode, &USER).ok_or("no Java packet tap")?;
    let (watcher, mut watcher_tap) =
        test_support::java_player_as(world, GameMode::Survival, &WATCHER)
            .ok_or("no Java packet tap")?;
    place(&user, user_at(scene, pos), pos);
    place(
        &watcher,
        Vector3::new(
            f64::from(pos.0.x) + 3.5,
            f64::from(pos.0.y),
            f64::from(pos.0.z) + 0.5,
        ),
        pos,
    );
    user.client_loaded.store(!scene.unloaded, Ordering::Relaxed);
    equip(&user, scene);
    world.add_player(&user)?;
    world.add_player(&watcher)?;
    let client = java(&user).ok_or("the user is not a Java player")?;
    let used = if scene.hand == LEFT {
        user.inventory().off_hand_item().item
    } else {
        user.inventory().held_item().item
    };
    drop(packets(&mut user_tap));
    drop(packets(&mut watcher_tap));
    let packet = SUseItemOn {
        hand: VarInt(scene.hand),
        position: pos,
        face: VarInt(scene.face),
        cursor_pos: Vector3::new(0.5, scene.cursor_y, 0.5),
        inside_block: false,
        is_against_world_border: false,
        sequence: VarInt(5),
    };
    let result = client.handle_use_item_on(&user, &packet, server);
    let names = scene.names_only;
    let mut line = format!(
        "{}: {result:?} | at {} above {} north {} | {:?} | user [{}] | sent [{}] | watcher [{}]",
        scene.name,
        block_text(world, &pos, names),
        block_text(world, &pos.up(), names),
        block_text(world, &pos.north(), names),
        things_near(world, pos, names),
        user_text(&user, used),
        tap_text(&mut user_tap, names),
        tap_text(&mut watcher_tap, names),
    );
    if scene.extra {
        line.push_str(&extra_text(&user));
    }
    Ok(line + "\n")
}

#[tokio::test(flavor = "multi_thread")]
async fn use_item_on_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let scenes = scenes();
    for index in 0..scenes.len() {
        let chunk = position_of(index, 64).chunk_position();
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
    script_golden::bless_or_compare("use_item_on.txt", &out)
}
