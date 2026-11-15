//! A record of `/clone` run through the real dispatcher (`tests/golden/clone_command.txt`;
//! rewrite it on purpose with `KVASIR_BLESS=1`), made before `CloneExecutor::execute` was split.
//!
//! Every case runs in its own area of loaded empty chunks, on the same small terrain: a stone
//! floor, a dirt checkerboard, a chest holding diamonds, glowstone and a redstone block inside the
//! source region, and redstone lamps next to the source and the destination. Source and
//! destination both cross a chunk border. One line per case: the command (relative to the area),
//! its result (count or error text), a hash of every block state around both regions, the bottom
//! layers and the empty volume used by the size cases, which positions hold a block entity (with a
//! chest's items), both lamps, and how many entities the world holds. A case that does not return
//! within the deadline is recorded as `HANG`.

use std::fmt::Write;
use std::sync::Arc;
use std::time::Duration;

use pumpkin_data::Block;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::text::TextComponent;
use pumpkin_world::world::BlockFlags;

use crate::block::entities::chest::ChestBlockEntity;
use crate::command::{CommandSender, CommandSource};
use crate::entity::ai::test_support;
use crate::server::Server;
use crate::world::World;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Source corners relative to the area (x 10..=21 crosses the chunk border at 16) and the
/// destination corner (x 30..=41 crosses 32).
const FROM: (i32, i32, i32) = (10, 60, 2);
const TO: (i32, i32, i32) = (21, 63, 7);
const DEST: (i32, i32, i32) = (30, 60, 2);
const LAMP: (i32, i32, i32) = (22, 61, 4);
const DEST_LAMP: (i32, i32, i32) = (42, 61, 4);
const CHEST: (i32, i32, i32) = (14, 61, 4);
const GLOW: (i32, i32, i32) = (18, 62, 5);
const POWER: (i32, i32, i32) = (21, 61, 4);

/// `{s}` is replaced by the source corners, `{d}` by the destination corner.
const CASES: &[&str] = &[
    "clone {s} {d}",
    "clone {s} {d} replace",
    "clone {s} {d} replace force",
    "clone {s} {d} replace move",
    "clone {s} {d} replace normal",
    "clone {s} {d} masked",
    "clone {s} {d} masked force",
    "clone {s} {d} masked move",
    "clone {s} {d} masked normal",
    "clone {s} {d} filtered minecraft:dirt",
    "clone {s} {d} filtered minecraft:dirt move",
    "clone {s} {d} filtered minecraft:dirt force",
    "clone {s} {d} filtered #minecraft:dirt normal",
    "clone {s} {d} filtered minecraft:chest",
    "clone {s} {d} filtered minecraft:chest move",
    "clone {s} {d} filtered minecraft:diamond_block",
    "clone {s} {d} filtered minecraft:air move",
    "clone {s} 12 60 2",
    "clone {s} 12 60 2 replace force",
    "clone {s} 12 60 2 replace move",
    "clone {s} 12 60 2 masked force",
    "clone {s} 12 60 2 filtered minecraft:dirt force",
    "clone {s} 10 60 2 replace force",
    "clone {s} 16 61 3 replace force",
    "clone {s} 4 59 0 masked force",
    "clone {s} 22 60 2",
    "clone {s} 21 60 2",
    "clone {s} 10 64 2",
    "clone {s} 10 60 8",
    "clone 0 0 0 40 40 40 0 0 0",
    "clone 0 0 0 31 31 31 32 0 0",
    "clone 0 0 0 31 31 32 32 0 0",
    "clone {s} 30 316 2",
    "clone {s} 30 317 2",
    "clone 10 -65 2 12 -64 2 30 60 2",
    "clone {s} 30 -64 2",
    "clone {s} 30 60 100",
    "clone 10 60 100 12 60 100 30 60 2",
    "clone 14 61 4 14 61 4 30 61 4",
    "clone 14 61 4 14 61 4 30 61 4 replace move",
    "clone 21 63 7 10 60 2 30 60 2",
    "clone {s} 30 60 -3",
    "clone {s} 30 60 -3 replace move",
    // Added after the mutation run: a destination just beside / just touching the source on
    // each side (the overlap test and the destination's far corner), a source above the build
    // limit, and moves along each axis (which sources count as inside the destination).
    "clone {s} 10 60 -4",
    "clone {s} 10 60 -3",
    "clone {s} -2 60 2",
    "clone {s} -1 60 2",
    "clone {s} 10 56 2",
    "clone {s} 10 57 2",
    "clone {s} 10 63 2",
    "clone {s} 10 60 7",
    "clone 10 318 2 12 320 2 30 60 2",
    "clone 14 61 4 21 63 7 2 61 4 replace move",
    "clone {s} 10 64 2 replace move",
    "clone {s} 10 56 2 replace move",
    "clone {s} 10 60 8 replace move",
    "clone {s} 10 60 -4 replace move",
];

fn place(world: &Arc<World>, pos: BlockPos, block: &Block) {
    world.set_block_state(
        &pos,
        block.default_state.id,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

fn at(x0: i32, (x, y, z): (i32, i32, i32)) -> BlockPos {
    BlockPos::new(x0 + x, y, z)
}

fn build_area(world: &Arc<World>, index: usize) -> i32 {
    let chunk_x = index as i32 * 4;
    for cx in chunk_x..chunk_x + 4 {
        for cz in -1..3 {
            world.level.loaded_chunks.insert(
                Vector2::new(cx, cz),
                pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
            );
        }
    }
    let x0 = chunk_x * 16;
    for x in 8..25 {
        for z in 0..10 {
            place(world, BlockPos::new(x0 + x, 60, z), &Block::STONE);
            if (x + z) % 2 == 0 {
                place(world, BlockPos::new(x0 + x, 61, z), &Block::DIRT);
            }
        }
    }
    // The chest goes through the normal path so it gets its block entity.
    world.set_block_state(
        &at(x0, CHEST),
        Block::CHEST.default_state.id,
        BlockFlags::NOTIFY_ALL,
    );
    if let Some(entity) = world.get_block_entity(&at(x0, CHEST))
        && let Some(chest) = entity.as_any().downcast_ref::<ChestBlockEntity>()
    {
        chest.items.write().expect("chest items")[3] = ItemStack::new(5, &Item::DIAMOND);
    }
    place(world, at(x0, GLOW), &Block::GLOWSTONE);
    place(world, at(x0, POWER), &Block::REDSTONE_BLOCK);
    place(world, at(x0, LAMP), &Block::REDSTONE_LAMP);
    place(world, at(x0, DEST_LAMP), &Block::REDSTONE_LAMP);
    x0
}

fn fnv(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash = (*hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
    }
}

/// A block entity's place and kind; a chest also lists its non-empty slots.
fn describe_block_entity(world: &Arc<World>, pos: &BlockPos, (x, y, z): (i32, i32, i32)) -> String {
    let Some(entity) = world.get_block_entity(pos) else {
        return String::new();
    };
    let mut text = format!("{x},{y},{z}:{}", entity.resource_location());
    if let Some(chest) = entity.as_any().downcast_ref::<ChestBlockEntity>() {
        for (slot, stack) in chest.items.read().expect("chest items").iter().enumerate() {
            if !stack.is_empty() {
                let _ = write!(text, "/{slot}={}x{}", stack.item_count, stack.item.id);
            }
        }
    }
    text
}

fn record_area(world: &Arc<World>, x0: i32) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut entities = Vec::new();
    for x in 0..=63 {
        for y in (-64..=-59).chain(0..=32).chain(58..=65) {
            for z in -4..=32 {
                let pos = BlockPos::new(x0 + x, y, z);
                fnv(
                    &mut hash,
                    &world.get_block_state_id(&pos).as_u16().to_le_bytes(),
                );
                let entity = describe_block_entity(world, &pos, (x, y, z));
                if !entity.is_empty() {
                    entities.push(entity);
                }
            }
        }
    }
    let lamp = world.get_block_state_id(&at(x0, LAMP)).as_u16();
    let dest_lamp = world.get_block_state_id(&at(x0, DEST_LAMP)).as_u16();
    let items = world.entities.load().len();
    format!(
        "blocks={hash:016x} be=[{}] lamps={lamp},{dest_lamp} entities={items}",
        entities.join(" ")
    )
}

fn run(server: &Arc<Server>, world: &Arc<World>, x0: i32, command: &str) -> String {
    let source = CommandSource::new(
        CommandSender::Console,
        world.clone(),
        None,
        Vector3::new(f64::from(x0), 64.0, 0.0),
        Vector2::new(0.0, 0.0),
        "Golden".to_string(),
        TextComponent::text("Golden"),
        server.clone(),
    )
    .with_silent();
    let server = server.clone();
    let command = command.to_owned();
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let dispatcher = server.command_dispatcher.load();
        let result = dispatcher.execute_input(&command, &source);
        let _ = send.send(match result {
            Ok(count) => format!("ok {count}"),
            Err(error) => format!("err {}", error.message.get_text()),
        });
    });
    receive
        .recv_timeout(Duration::from_secs(20))
        .unwrap_or_else(|_| "HANG".to_owned())
}

/// The command with `{s}`/`{d}` expanded and every x coordinate moved into the area.
fn expand(case: &str, x0: i32) -> String {
    let relative = case
        .replace(
            "{s}",
            &format!(
                "{} {} {} {} {} {}",
                FROM.0, FROM.1, FROM.2, TO.0, TO.1, TO.2
            ),
        )
        .replace("{d}", &format!("{} {} {}", DEST.0, DEST.1, DEST.2));
    let mut words: Vec<String> = relative.split(' ').map(str::to_owned).collect();
    // Words 1, 4 and 7 are the x coordinates.
    for i in [1, 4, 7] {
        if let Ok(x) = words[i].parse::<i32>() {
            words[i] = (x0 + x).to_string();
        }
    }
    words.join(" ")
}

#[tokio::test(flavor = "multi_thread")]
async fn clone_command_changes_the_same_blocks() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();

    let mut now = String::new();
    for (index, case) in CASES.iter().enumerate() {
        let x0 = build_area(&world, index + 1);
        let before = record_area(&world, x0);
        let command = expand(case, x0);
        let result = run(&server, &world, x0, &command);
        let after = record_area(&world, x0);
        writeln!(now, "{case} -> {result}; before {before}; after {after}")?;
    }

    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/clone_command.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &now)?;
    } else {
        assert_eq!(std::fs::read_to_string(&path)?, now);
    }
    server.stop();
    Ok(())
}
