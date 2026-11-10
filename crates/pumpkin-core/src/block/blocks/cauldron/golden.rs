//! A record of what `CauldronBlock::use_with_item` does, made before the function was split
//! (`tests/golden/cauldron_use.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! The test starts a real [`Server`] (the arguments carry one), loads an empty chunk, and uses
//! every held item of the corpus on every cauldron state, in survival and creative, with a
//! stack of 1 and of 16, and with an empty inventory, a full one, and a full one that still has
//! room for a bucket, a glass bottle or a lava bucket. Each case gets a fresh player standing on
//! its own cauldron, a real [`Player`] over an in-memory connection. One line per case: the
//! result, the block state after, the held stack, a hash of the inventory, the number of items
//! dropped into the world and the five cauldron statistics.
//!
//! Not seen: sounds (the test player is not in the world, so nobody hears them) and a plugin
//! cancelling the level-change event (no plugin is loaded).

use std::fmt::Write;
use std::sync::Arc;

use pumpkin_data::data_component::DataComponent;
use pumpkin_data::data_component_impl::{
    BannerPatternsImpl, DataComponentImpl, DyedColorImpl, EquipmentSlot,
};
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::statistic::CustomStatistic;
use pumpkin_data::{Block, BlockDirection, BlockStateId, Property};
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::BlockFlags;

use super::CauldronBlock;
use crate::block::registry::BlockActionResult;
use crate::block::{BlockBehaviour, BlockHitResult, UseWithItemArgs};
use crate::entity::EntityBase;
use crate::entity::ai::test_support;
use crate::entity::player::Player;
use crate::server::Server;
use crate::world::World;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// FNV-1a: a hash that does not change with the Rust version.
fn fnv(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in text.bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn level_state(block: &Block, level: u8) -> BlockStateId {
    block
        .from_properties(&[Property::Level(level)])
        .to_state_id(block)
}

/// The cauldron states of the corpus: label, block, state.
fn cauldrons() -> Vec<(&'static str, &'static Block, BlockStateId)> {
    vec![
        ("empty", &Block::CAULDRON, Block::CAULDRON.default_state.id),
        (
            "water1",
            &Block::WATER_CAULDRON,
            level_state(&Block::WATER_CAULDRON, 1),
        ),
        (
            "water2",
            &Block::WATER_CAULDRON,
            level_state(&Block::WATER_CAULDRON, 2),
        ),
        (
            "water3",
            &Block::WATER_CAULDRON,
            level_state(&Block::WATER_CAULDRON, 3),
        ),
        (
            "lava",
            &Block::LAVA_CAULDRON,
            Block::LAVA_CAULDRON.default_state.id,
        ),
        (
            "snow1",
            &Block::POWDER_SNOW_CAULDRON,
            level_state(&Block::POWDER_SNOW_CAULDRON, 1),
        ),
        (
            "snow2",
            &Block::POWDER_SNOW_CAULDRON,
            level_state(&Block::POWDER_SNOW_CAULDRON, 2),
        ),
        (
            "snow3",
            &Block::POWDER_SNOW_CAULDRON,
            level_state(&Block::POWDER_SNOW_CAULDRON, 3),
        ),
    ]
}

/// The held items of the corpus.
fn held(kind: usize, count: u8) -> (&'static str, ItemStack) {
    let with = |item: &'static Item,
                component: DataComponent,
                value: Option<Box<dyn DataComponentImpl>>| {
        ItemStack::new_with_component(count, item, vec![(component, value)])
    };
    match kind {
        0 => ("water_bucket", ItemStack::new(count, &Item::WATER_BUCKET)),
        1 => ("lava_bucket", ItemStack::new(count, &Item::LAVA_BUCKET)),
        2 => (
            "snow_bucket",
            ItemStack::new(count, &Item::POWDER_SNOW_BUCKET),
        ),
        3 => ("potion", ItemStack::new(count, &Item::POTION)),
        4 => {
            let mut bottle = crate::item::items::glass_bottle::water_bottle();
            bottle.item_count = count;
            ("water_potion", bottle)
        }
        5 => ("bucket", ItemStack::new(count, &Item::BUCKET)),
        6 => ("glass_bottle", ItemStack::new(count, &Item::GLASS_BOTTLE)),
        7 => ("red_shulker", ItemStack::new(count, &Item::RED_SHULKER_BOX)),
        8 => ("shulker", ItemStack::new(count, &Item::SHULKER_BOX)),
        9 => ("banner", ItemStack::new(count, &Item::WHITE_BANNER)),
        10 => (
            "banner_patterned",
            with(
                &Item::WHITE_BANNER,
                DataComponent::BannerPatterns,
                Some(Box::new(BannerPatternsImpl::EMPTY)),
            ),
        ),
        11 => (
            "banner_removed",
            with(&Item::WHITE_BANNER, DataComponent::BannerPatterns, None),
        ),
        12 => ("leather", ItemStack::new(count, &Item::LEATHER_CHESTPLATE)),
        13 => (
            "leather_dyed",
            with(
                &Item::LEATHER_CHESTPLATE,
                DataComponent::DyedColor,
                Some(Box::new(DyedColorImpl { rgb: 0x00ff_0000 })),
            ),
        ),
        _ => ("stone", ItemStack::new(count, &Item::STONE)),
    }
}
const HELD_KINDS: usize = 15;

/// Inventory set-ups: 0 empty, 1 full of stone, 2 full but with room for a bucket, a glass
/// bottle and (in an empty slot) one more item.
fn fill_inventory(player: &Player, setup: usize) {
    if setup == 0 {
        return;
    }
    let mut main = player
        .inventory
        .main_inventory
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for slot in main.iter_mut() {
        *slot = ItemStack::new(64, &Item::STONE);
    }
    if setup == 2 {
        main[0] = ItemStack::new(15, &Item::BUCKET);
        main[1] = ItemStack::new(63, &Item::GLASS_BOTTLE);
        main[2] = ItemStack::EMPTY.clone();
    }
}

fn stack_text(stack: &ItemStack) -> String {
    let mut text = format!("{} x{}", stack.item.registry_key, stack.item_count);
    for (component, value) in &stack.patch {
        let _ = write!(
            text,
            " {component:?}{}",
            if value.is_some() { "" } else { "-" }
        );
    }
    text
}

fn inventory_hash(player: &Player) -> u64 {
    let main = player
        .inventory
        .main_inventory
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut text = String::new();
    for stack in main.iter() {
        text.push_str(&stack_text(stack));
        text.push(';');
    }
    fnv(&text)
}

const fn result_name(result: &BlockActionResult) -> &'static str {
    match result {
        BlockActionResult::Success => "success",
        BlockActionResult::SuccessServer => "success_server",
        BlockActionResult::Consume => "consume",
        BlockActionResult::Fail => "fail",
        BlockActionResult::Pass => "pass",
        BlockActionResult::PassToDefaultBlockAction => "pass_to_default",
    }
}

fn stats(player: &Player) -> [i32; 5] {
    [
        CustomStatistic::FillCauldron,
        CustomStatistic::UseCauldron,
        CustomStatistic::CleanShulkerBox,
        CustomStatistic::CleanBanner,
        CustomStatistic::CleanArmor,
    ]
    .map(|stat| player.get_custom_stat(stat))
}

/// One case of the corpus.
struct Case {
    index: usize,
    cauldron: (&'static str, &'static Block, BlockStateId),
    kind: usize,
    gamemode: GameMode,
    count: u8,
    setup: usize,
}

fn run_case(server: &Server, world: &Arc<World>, case: &Case) -> String {
    let i = case.index as i32;
    let position = BlockPos::new((i % 8) * 2, -60 + (i / 64) * 3, ((i / 8) % 8) * 2);
    let (label, block, state) = case.cauldron;
    world.set_block_state(&position, state, BlockFlags::NOTIFY_ALL);
    let feet = Vector3::new(
        f64::from(position.0.x) + 0.5,
        f64::from(position.0.y) + 1.0,
        f64::from(position.0.z) + 0.5,
    );
    let player = test_support::java_player(world, case.gamemode);
    player.get_entity().set_pos(feet);
    fill_inventory(&player, case.setup);
    let (held_label, mut stack) = held(case.kind, case.count);
    let entities_before = world.entities.load().len();
    let result = CauldronBlock.use_with_item(UseWithItemArgs {
        server,
        world,
        block,
        position: &position,
        player: &player,
        hit: &BlockHitResult {
            face: &BlockDirection::Up,
            cursor_pos: &Vector3::new(0.5, 1.0, 0.5),
        },
        item_stack: &mut stack,
        equipment_slot: &EquipmentSlot::MAIN_HAND,
    });
    let drops = world.entities.load().len() - entities_before;
    format!(
        "{label} {held_label} {:?} x{} inv{}: {} state {} hand [{}] inv {:016x} drops {drops} stats {:?}",
        case.gamemode,
        case.count,
        case.setup,
        result_name(&result),
        world.get_block_state_id(&position),
        stack_text(&stack),
        inventory_hash(&player),
        stats(&player),
    )
}

fn run(server: &Server, world: &Arc<World>) -> String {
    let mut report = String::new();
    let mut index = 0;
    for cauldron in cauldrons() {
        for kind in 0..HELD_KINDS {
            for gamemode in [GameMode::Survival, GameMode::Creative] {
                for count in [1, 16] {
                    for setup in 0..3 {
                        let case = Case {
                            index,
                            cauldron,
                            kind,
                            gamemode,
                            count,
                            setup,
                        };
                        report.push_str(&run_case(server, world, &case));
                        report.push('\n');
                        index += 1;
                    }
                }
            }
        }
    }
    report
}

#[tokio::test(flavor = "multi_thread")]
async fn cauldron_use() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server
        .worlds
        .load()
        .first()
        .cloned()
        .ok_or("the server has no world")?;
    world.level.loaded_chunks.insert(
        Vector2::new(0, 0),
        pumpkin_world::chunk::ChunkData::empty_sync(0, 0),
    );
    let now = run(&server, &world);
    server.stop();

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("cauldron_use.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}
