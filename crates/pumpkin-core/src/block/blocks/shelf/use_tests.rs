//! Using a shelf (vanilla 26.3 `ShelfBlock.useItemOn`): the clicked column swaps with the hand;
//! on a powered chain the hotbar swaps with every shelf, leftmost shelf first.

use std::sync::Arc;

use pumpkin_data::block_properties::{
    AcaciaShelfLikeProperties as ShelfProperties, HorizontalFacing,
};
use pumpkin_data::data_component_impl::EquipmentSlot;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::{Block, BlockDirection};
use pumpkin_inventory::Inventory;
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::BlockFlags;

use super::ShelfBlock;
use crate::block::entities::shelf::ShelfBlockEntity;
use crate::block::registry::BlockActionResult;
use crate::block::{BlockBehaviour, BlockHitResult, UseWithItemArgs};
use crate::entity::ai::test_support;
use crate::entity::player::Player;
use crate::server::Server;
use crate::world::World;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Stage {
    server: Arc<Server>,
    world: Arc<World>,
    player: Arc<Player>,
}

async fn stage(mode: GameMode) -> Result<(Stage, tempfile::TempDir), Box<dyn std::error::Error>> {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    world.level.loaded_chunks.insert(
        Vector2::new(0, 0),
        pumpkin_world::chunk::ChunkData::empty_sync(0, 0),
    );
    let player = test_support::java_player(&world, mode);
    Ok((
        Stage {
            server,
            world,
            player,
        },
        folder,
    ))
}

fn shelf_pos(i: i32) -> BlockPos {
    BlockPos::new(i, -60, 0)
}

impl Stage {
    /// A north-facing shelf, powered the way a world powers it: a redstone block below.
    fn put_shelf(&self, i: i32, powered: bool) {
        let mut props = ShelfProperties::default(&Block::OAK_SHELF);
        props.facing = HorizontalFacing::North;
        self.world.set_block_state(
            &shelf_pos(i),
            props.to_state_id(&Block::OAK_SHELF),
            BlockFlags::NOTIFY_ALL,
        );
        if powered {
            let below = BlockPos::new(i, -61, 0);
            self.world.set_block_state(
                &below,
                Block::REDSTONE_BLOCK.default_state.id,
                BlockFlags::NOTIFY_ALL,
            );
        }
    }

    fn shelf(&self, i: i32) -> Arc<dyn crate::block::entities::BlockEntity> {
        self.world
            .get_block_entity(&shelf_pos(i))
            .expect("shelf entity")
    }

    fn items(&self, i: i32) -> Vec<String> {
        let entity = self.shelf(i);
        let shelf = entity
            .as_any()
            .downcast_ref::<ShelfBlockEntity>()
            .expect("shelf");
        (0..3).map(|s| text(&shelf.get_stack(s))).collect()
    }

    fn set_shelf_slot(&self, i: i32, slot: usize, stack: ItemStack) {
        let entity = self.shelf(i);
        entity
            .as_any()
            .downcast_ref::<ShelfBlockEntity>()
            .expect("shelf")
            .set_stack(slot, stack);
    }

    fn hotbar(&self) -> Vec<String> {
        let inventory = self.player.inventory();
        (0..9).map(|s| text(&inventory.get_stack(s))).collect()
    }

    /// Clicks shelf `i` on `face` at `cursor`; the held stack is the selected hotbar slot.
    fn click(&self, i: i32, face: BlockDirection, cursor_x: f32) -> (String, String) {
        let inventory = self.player.inventory();
        let mut held = inventory.held_item();
        let result = ShelfBlock.use_with_item(UseWithItemArgs {
            server: &self.server,
            world: &self.world,
            block: &Block::OAK_SHELF,
            position: &shelf_pos(i),
            player: &self.player,
            hit: &BlockHitResult {
                face: &face,
                cursor_pos: &Vector3::new(cursor_x, 0.5, 1.0),
            },
            item_stack: &mut held,
            equipment_slot: &EquipmentSlot::MAIN_HAND,
        });
        let name = match result {
            BlockActionResult::Success => "Success",
            BlockActionResult::SuccessServer => "SuccessServer",
            BlockActionResult::Consume => "Consume",
            BlockActionResult::Fail => "Fail",
            BlockActionResult::Pass => "Pass",
            BlockActionResult::PassToDefaultBlockAction => "PassToDefault",
        };
        (name.to_string(), text(&held))
    }

    fn hold(&self, slot: u8, stack: ItemStack) {
        let inventory = self.player.inventory();
        inventory.set_selected_slot(slot);
        inventory.set_stack(usize::from(slot), stack);
    }
}

fn text(stack: &ItemStack) -> String {
    if stack.is_empty() {
        "-".into()
    } else {
        format!("{}x{}", stack.item_count, stack.item.registry_key)
    }
}

fn stone(n: u8) -> ItemStack {
    ItemStack::new(n, &Item::STONE)
}

fn apple(n: u8) -> ItemStack {
    ItemStack::new(n, &Item::APPLE)
}

// A north-facing shelf is hit on its north face; the column runs from the viewer's left, which
// is east, so a low `x` on the face is the third column.
const LEFT_COLUMN: f32 = 0.9;
const MIDDLE_COLUMN: f32 = 0.5;
const RIGHT_COLUMN: f32 = 0.1;

#[tokio::test(flavor = "multi_thread")]
async fn a_click_puts_the_hand_in_the_slot_and_takes_it_back() -> TestResult {
    let (s, _folder) = stage(GameMode::Survival).await?;
    s.put_shelf(0, false);
    s.hold(0, stone(5));
    let put = s.click(0, BlockDirection::North, LEFT_COLUMN);
    assert_eq!(put, ("SuccessServer".into(), "-".into()));
    assert_eq!(s.items(0), ["5xstone", "-", "-"]);
    assert_eq!(s.hotbar()[0], "-");
    let take = s.click(0, BlockDirection::North, LEFT_COLUMN);
    assert_eq!(take, ("SuccessServer".into(), "5xstone".into()));
    assert_eq!(s.items(0), ["-", "-", "-"]);
    s.server.stop();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_click_swaps_the_hand_with_the_clicked_column() -> TestResult {
    let (s, _folder) = stage(GameMode::Survival).await?;
    s.put_shelf(0, false);
    s.set_shelf_slot(0, 1, apple(3));
    s.hold(0, stone(5));
    let swap = s.click(0, BlockDirection::North, MIDDLE_COLUMN);
    assert_eq!(swap, ("SuccessServer".into(), "3xapple".into()));
    assert_eq!(s.items(0), ["-", "5xstone", "-"]);
    s.click(0, BlockDirection::North, RIGHT_COLUMN);
    assert_eq!(s.items(0), ["-", "5xstone", "3xapple"]);
    s.server.stop();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn clicks_that_do_nothing_pass() -> TestResult {
    let (s, _folder) = stage(GameMode::Survival).await?;
    s.put_shelf(0, false);
    s.hold(0, stone(5));
    let side = s.click(0, BlockDirection::East, MIDDLE_COLUMN);
    assert_eq!(side, ("Pass".into(), "5xstone".into()));
    s.hold(0, ItemStack::EMPTY.clone());
    let empty = s.click(0, BlockDirection::North, MIDDLE_COLUMN);
    assert_eq!(empty, ("Pass".into(), "-".into()));
    assert_eq!(s.items(0), ["-", "-", "-"]);
    s.server.stop();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn creative_keeps_the_stack_in_hand_when_placing() -> TestResult {
    let (s, _folder) = stage(GameMode::Creative).await?;
    s.put_shelf(0, false);
    s.hold(0, stone(5));
    let put = s.click(0, BlockDirection::North, LEFT_COLUMN);
    assert_eq!(put, ("SuccessServer".into(), "5xstone".into()));
    assert_eq!(s.items(0), ["5xstone", "-", "-"]);
    s.server.stop();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_powered_chain_swaps_with_the_hotbar_leftmost_shelf_first() -> TestResult {
    let (s, _folder) = stage(GameMode::Survival).await?;
    // Two powered shelves side by side: from the front the left one is the one at x = 1.
    s.put_shelf(0, true);
    s.put_shelf(1, true);
    s.hold(8, stone(2));
    for slot in 3..8 {
        s.player.inventory().set_stack(slot, apple(slot as u8));
    }
    s.set_shelf_slot(1, 0, ItemStack::new(7, &Item::DIRT));
    let result = s.click(1, BlockDirection::North, MIDDLE_COLUMN);
    assert_eq!(result.0, "SuccessServer");
    // The left shelf (x = 1) took hotbar 3..5, the right one (x = 0) took 6..8.
    assert_eq!(s.items(1), ["3xapple", "4xapple", "5xapple"]);
    assert_eq!(s.items(0), ["6xapple", "7xapple", "2xstone"]);
    assert_eq!(s.hotbar()[3..], ["7xdirt", "-", "-", "-", "-", "-"]);
    s.server.stop();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_powered_swap_with_nothing_to_move_is_consumed() -> TestResult {
    let (s, _folder) = stage(GameMode::Survival).await?;
    s.put_shelf(0, true);
    let result = s.click(0, BlockDirection::North, MIDDLE_COLUMN);
    assert_eq!(result.0, "Consume");
    s.server.stop();
    Ok(())
}
