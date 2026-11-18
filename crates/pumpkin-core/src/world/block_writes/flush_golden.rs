//! A record of what `World::flush_block_updates` sends, made before the function was split
//! (`tests/golden/flush_block_updates.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene places blocks (stone, dirt, a chest holding an apple, a waterlogged slab) in
//! loaded empty chunks, queues their changes with `register_block_change` and flushes once. A
//! Java and a Bedrock player watch the scene's chunk, a second Java player watches a chunk far
//! away. One line per scene: each player's packets, sorted (the changes sit in hash maps, so
//! the order between chunk sections is not fixed); a multi-block update is shown as its section
//! and its sorted entries.

use std::fmt::Write;
use std::num::NonZero;
use std::sync::Arc;

use pumpkin_data::block_properties::WhiteWoolSlabLikeProperties;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::packet::CURRENT_MC_VERSION;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_inventory::Inventory;
use pumpkin_protocol::java::client::play::CMultiBlockUpdate;
use pumpkin_protocol::packet::MultiVersionJavaPacket;
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use pumpkin_world::world::BlockFlags;

use crate::block::entities::chest::ChestBlockEntity;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;
use crate::entity::player::Player;
use crate::net::bedrock::BedrockPacketTap;
use crate::net::java::PacketTap;
use crate::world::World;

#[derive(Clone, Copy)]
enum Kind {
    Stone,
    Dirt,
    Chest,
    WaterloggedSlab,
}

/// One queued change: offset from the scene's base position and the block placed there.
type Change = (i32, i32, i32, Kind);

struct Scene {
    name: &'static str,
    changes: &'static [Change],
    /// Nobody watches the scene's chunk.
    unwatched: bool,
}

const fn scene(name: &'static str, changes: &'static [Change]) -> Scene {
    Scene {
        name,
        changes,
        unwatched: false,
    }
}

const SCENES: &[Scene] = &[
    scene("nothing queued", &[]),
    scene("one stone", &[(0, 0, 0, Kind::Stone)]),
    scene("one chest", &[(0, 0, 0, Kind::Chest)]),
    scene("one waterlogged slab", &[(0, 0, 0, Kind::WaterloggedSlab)]),
    scene(
        "two in one section",
        &[(0, 0, 0, Kind::Stone), (1, 0, 0, Kind::Dirt)],
    ),
    scene(
        "three with a chest",
        &[
            (0, 0, 0, Kind::Stone),
            (2, 0, 1, Kind::Chest),
            (1, 1, 0, Kind::Dirt),
        ],
    ),
    scene(
        "two with a waterlogged slab",
        &[(0, 0, 0, Kind::WaterloggedSlab), (1, 0, 0, Kind::Stone)],
    ),
    scene(
        "two sections apart",
        &[(0, 0, 0, Kind::Stone), (0, 16, 0, Kind::Dirt)],
    ),
    scene(
        "two chunks apart",
        &[(0, 0, 0, Kind::Stone), (16, 0, 0, Kind::Chest)],
    ),
    scene(
        "the same position twice",
        &[(0, 0, 0, Kind::Stone), (0, 0, 0, Kind::Dirt)],
    ),
    Scene {
        name: "watched by nobody",
        changes: &[(0, 0, 0, Kind::Stone), (1, 0, 0, Kind::Chest)],
        unwatched: true,
    },
    Scene {
        name: "one, watched by nobody",
        changes: &[(0, 0, 0, Kind::Chest)],
        unwatched: true,
    },
];

fn state_of(kind: Kind) -> BlockStateId {
    match kind {
        Kind::Stone => Block::STONE.default_state.id,
        Kind::Dirt => Block::DIRT.default_state.id,
        Kind::Chest => Block::CHEST.default_state.id,
        Kind::WaterloggedSlab => {
            let mut slab = WhiteWoolSlabLikeProperties::default(&Block::OAK_SLAB);
            slab.waterlogged = true;
            slab.to_state_id(&Block::OAK_SLAB)
        }
    }
}

/// Puts the block without queueing a change, then queues it like `set_block_state` would.
fn place(world: &Arc<World>, pos: BlockPos, kind: Kind) -> BlockStateId {
    let state = state_of(kind);
    world.set_block_state(
        &pos,
        state,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
    if matches!(kind, Kind::Chest) {
        let chest = Arc::new(ChestBlockEntity::new(pos));
        chest.set_stack(0, ItemStack::new(1, &Item::APPLE));
        world.add_block_entity(chest);
    }
    state
}

fn var_int(bytes: &[u8], at: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = *bytes.get(*at)?;
        *at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::new();
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// A multi-block update as `multi <section> [entries sorted]`; anything else as hex.
fn java_packet_text(packet: &[u8]) -> String {
    let multi = u64::from(CMultiBlockUpdate::to_id(CURRENT_MC_VERSION).cast_unsigned());
    let mut at = 0;
    if var_int(packet, &mut at) != Some(multi) || packet.len() < at + 8 {
        return hex(packet);
    }
    let section = hex(&packet[at..at + 8]);
    at += 8;
    let Some(count) = var_int(packet, &mut at) else {
        return hex(packet);
    };
    let mut entries = Vec::new();
    for _ in 0..count {
        let Some(entry) = var_int(packet, &mut at) else {
            return hex(packet);
        };
        entries.push(entry);
    }
    entries.sort_unstable();
    let entries: Vec<String> = entries.iter().map(|e| format!("{e:x}")).collect();
    format!(
        "multi {section} [{}] rest {}",
        entries.join(" "),
        hex(&packet[at..])
    )
}

fn java_packets(tap: &mut PacketTap) -> Vec<String> {
    let mut texts: Vec<String> = tap.drain().iter().map(|p| java_packet_text(p)).collect();
    texts.sort();
    texts
}

fn bedrock_packets(tap: &mut BedrockPacketTap) -> Vec<String> {
    let mut texts: Vec<String> = tap.drain().iter().map(|p| hex(p)).collect();
    texts.sort();
    texts
}

fn watch(player: &Player, chunk: Vector2<i32>) {
    player.watched_section.store(Cylindrical::new(
        chunk,
        NonZero::<u8>::MIN.saturating_add(1),
    ));
}

struct Watchers {
    java: Arc<Player>,
    java_tap: PacketTap,
    bedrock: Arc<Player>,
    bedrock_tap: BedrockPacketTap,
    far: Arc<Player>,
    far_tap: PacketTap,
}

impl Watchers {
    fn drain(&mut self) {
        drop(java_packets(&mut self.java_tap));
        drop(bedrock_packets(&mut self.bedrock_tap));
        drop(java_packets(&mut self.far_tap));
    }
}

const fn base_of(index: usize) -> BlockPos {
    BlockPos::new(index as i32 * 64 + 1000, 64, 8)
}

fn run(out: &mut String, world: &Arc<World>, watchers: &mut Watchers, index: usize, scene: &Scene) {
    let base = base_of(index);
    let scene_chunk = base.chunk_position();
    let away = Vector2::new(scene_chunk.x + 40, scene_chunk.y);
    let watched = if scene.unwatched { away } else { scene_chunk };
    watch(&watchers.java, watched);
    watch(&watchers.bedrock, watched);
    watch(&watchers.far, away);
    let mut queued = Vec::new();
    for &(dx, dy, dz, kind) in scene.changes {
        let pos = BlockPos::new(base.0.x + dx, base.0.y + dy, base.0.z + dz);
        queued.push((pos, place(world, pos, kind)));
    }
    world.flush_block_updates();
    watchers.drain();
    for (pos, state) in queued {
        world.register_block_change(pos, state);
    }
    world.flush_block_updates();
    let _ = writeln!(
        out,
        "{}: java {:?}; bedrock {:?}; far {:?}",
        scene.name,
        java_packets(&mut watchers.java_tap),
        bedrock_packets(&mut watchers.bedrock_tap),
        java_packets(&mut watchers.far_tap),
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn flush_block_updates_sends_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    for index in 0..SCENES.len() {
        let chunk = base_of(index).chunk_position();
        for dx in 0..=1 {
            let at = Vector2::new(chunk.x + dx, chunk.y);
            world
                .level
                .loaded_chunks
                .insert(at, pumpkin_world::chunk::ChunkData::empty_sync(at.x, at.y));
        }
    }
    let (java, java_tap) =
        test_support::java_player_with_tap(&world, GameMode::Survival).ok_or("no packet tap")?;
    let (bedrock, bedrock_tap) =
        test_support::bedrock_player_with_tap(&world, GameMode::Survival).await?;
    let (far, far_tap) =
        test_support::java_player_with_tap(&world, GameMode::Survival).ok_or("no packet tap")?;
    world.add_player(&java)?;
    world.add_player(&bedrock)?;
    world.add_player(&far)?;
    let mut watchers = Watchers {
        java,
        java_tap,
        bedrock,
        bedrock_tap,
        far,
        far_tap,
    };
    let mut out = String::new();
    for (index, scene) in SCENES.iter().enumerate() {
        run(&mut out, &world, &mut watchers, index, scene);
    }
    world.remove_player(&watchers.java, false).await;
    world.remove_player(&watchers.bedrock, false).await;
    world.remove_player(&watchers.far, false).await;
    server.stop();
    script_golden::bless_or_compare("flush_block_updates.txt", &out)
}
