//! A record of `/fill` run through the real dispatcher (`tests/golden/fill_command.txt`;
//! rewrite it on purpose with `KVASIR_BLESS=1`), made before `fill_blocks` was split.
//!
//! Every case runs in its own area of loaded empty chunks, on the same small terrain: a stone
//! floor, a dirt checkerboard, a chest and glowstone inside the region, and a redstone lamp just
//! outside it. The region crosses a chunk border. One line per case: the command (relative to the
//! area), its result (count or error text), a hash of every block state in the region plus a
//! one-block margin, which positions hold a block entity, and whether the lamp is lit. A case that
//! does not return within the deadline is recorded as `HANG` and its thread is left behind.

use std::fmt::Write;
use std::sync::Arc;
use std::time::Duration;

use pumpkin_data::Block;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::text::TextComponent;
use pumpkin_world::world::BlockFlags;

use crate::command::{CommandSender, CommandSource};
use crate::entity::ai::test_support;
use crate::server::Server;
use crate::world::World;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Region corners relative to the area; x 10..=21 crosses the chunk border at 16.
const FROM: (i32, i32, i32) = (10, 60, 2);
const TO: (i32, i32, i32) = (21, 63, 7);
const LAMP: (i32, i32, i32) = (22, 61, 4);
const CHEST: (i32, i32, i32) = (14, 61, 4);
const GLOW: (i32, i32, i32) = (18, 62, 5);

/// `{r}` is replaced by the region corners. No `destroy` case yet: `fill .. destroy` deadlocks
/// (board card: it calls `break_block` while holding the chunk's section read lock), so those
/// cases are appended after the fix.
const CASES: &[&str] = &[
    "fill {r} minecraft:glass",
    "fill {r} minecraft:stone",
    "fill {r} minecraft:air",
    "fill {r} minecraft:redstone_block",
    "fill {r} minecraft:chest[facing=east]",
    "fill {r} minecraft:glass outline",
    "fill {r} minecraft:glass hollow",
    "fill {r} minecraft:glass strict",
    "fill {r} minecraft:redstone_block strict",
    "fill {r} minecraft:glass keep",
    "fill {r} minecraft:glass replace",
    "fill {r} minecraft:glass replace minecraft:dirt",
    "fill {r} minecraft:glass replace minecraft:stone outline",
    "fill {r} minecraft:glass replace minecraft:air hollow",
    "fill {r} minecraft:glass replace minecraft:dirt strict",
    "fill {r} minecraft:glass replace #minecraft:dirt",
    "fill {r} minecraft:glass replace minecraft:chest",
    "fill {r} minecraft:glass replace minecraft:diamond_block",
    "fill {r} minecraft:air replace minecraft:glowstone",
    "fill {r} minecraft:stone replace minecraft:stone",
    "fill {r} minecraft:redstone_lamp[lit=true]",
    "fill 10 63 2 10 63 2 minecraft:glass",
    "fill 21 60 7 10 60 2 minecraft:glass",
    "fill 0 0 0 40 40 40 minecraft:glass",
    "fill 0 0 0 31 31 31 minecraft:glass",
    "fill 0 0 0 31 31 32 minecraft:glass",
    "fill 10 60 2 10 60 2 minecraft:stone",
    "fill 10 61 2 12 61 2 minecraft:glass keep",
    "fill 10 70 2 12 70 2 minecraft:glass keep",
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
    place(world, at(x0, GLOW), &Block::GLOWSTONE);
    place(world, at(x0, LAMP), &Block::REDSTONE_LAMP);
    x0
}

fn fnv(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash = (*hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
    }
}

fn record_area(world: &Arc<World>, x0: i32) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut entities = Vec::new();
    for x in FROM.0 - 1..=TO.0 + 1 {
        for y in FROM.1 - 1..=TO.1 + 1 {
            for z in FROM.2 - 1..=TO.2 + 1 {
                let pos = BlockPos::new(x0 + x, y, z);
                fnv(
                    &mut hash,
                    &world.get_block_state_id(&pos).as_u16().to_le_bytes(),
                );
                if let Some(entity) = world.get_block_entity(&pos) {
                    entities.push(format!("{x},{y},{z}:{}", entity.resource_location()));
                }
            }
        }
    }
    let lamp = world.get_block_state_id(&at(x0, LAMP)).as_u16();
    format!("blocks={hash:016x} be=[{}] lamp={lamp}", entities.join(" "))
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

/// The command with `{r}` expanded and every coordinate moved into the area.
fn expand(case: &str, x0: i32) -> String {
    let relative = case.replace(
        "{r}",
        &format!(
            "{} {} {} {} {} {}",
            FROM.0, FROM.1, FROM.2, TO.0, TO.1, TO.2
        ),
    );
    let mut words: Vec<String> = relative.split(' ').map(str::to_owned).collect();
    // Words 1 and 4 are the x coordinates.
    for i in [1, 4] {
        if let Ok(x) = words[i].parse::<i32>() {
            words[i] = (x0 + x).to_string();
        }
    }
    words.join(" ")
}

#[tokio::test(flavor = "multi_thread")]
async fn fill_command_changes_the_same_blocks() -> TestResult {
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
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/fill_command.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &now)?;
    } else {
        assert_eq!(std::fs::read_to_string(&path)?, now);
    }
    server.stop();
    Ok(())
}
