use std::sync::Arc;

use crate::block::registry::BlockActionResult;
use crate::block::{
    BlockBehaviour, BlockMetadata, GetComparatorOutputArgs, PathComputationType, UseWithItemArgs,
};
use pumpkin_data::block_properties::WaterCauldronLikeProperties;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::sound::{Sound, SoundCategory};
use pumpkin_data::statistic::{CustomStatistic, StatisticCategory};
use pumpkin_data::tag::Taggable;
use pumpkin_data::{Block, BlockId, BlockState, BlockStateId};
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_world::world::BlockFlags;

#[cfg(test)]
mod golden;

pub struct CauldronBlock;

impl BlockMetadata for CauldronBlock {
    fn ids() -> Box<[BlockId]> {
        [
            BlockId::CAULDRON,
            BlockId::WATER_CAULDRON,
            BlockId::LAVA_CAULDRON,
            BlockId::POWDER_SNOW_CAULDRON,
        ]
        .into()
    }
}

fn fire_cauldron_change(
    world: &std::sync::Arc<crate::world::World>,
    pos: pumpkin_util::math::position::BlockPos,
    old_level: i32,
    new_level: i32,
    reason: crate::plugin::block::cauldron_level_change::CauldronChangeReason,
    entity: Option<std::sync::Arc<dyn crate::entity::EntityBase>>,
) -> bool {
    let mut event = crate::plugin::block::cauldron_level_change::CauldronLevelChangeEvent {
        block_pos: pos,
        world: world.clone(),
        old_level,
        new_level,
        reason,
        entity,
        cancelled: false,
    };
    if let Some(server) = world.server.upgrade() {
        server.plugin_manager.fire_blocking(&server, &mut event);
    }
    !event.cancelled
}

fn give_item_or_drop(
    player: &crate::entity::player::Player,
    world: &std::sync::Arc<crate::world::World>,
    item: &'static Item,
) {
    give_stack_or_drop(player, world, ItemStack::new(1, item));
}

fn give_stack_or_drop(
    player: &crate::entity::player::Player,
    world: &std::sync::Arc<crate::world::World>,
    mut stack: ItemStack,
) {
    let was_added = player.inventory.insert_stack_anywhere(&mut stack);
    if !was_added && !stack.is_empty() {
        world.drop_stack(&player.position().to_block_pos(), stack);
    }
}

/// The state of a water or powder snow cauldron at `level`.
fn level_state(block: &Block, level: u8) -> BlockStateId {
    block
        .from_properties(&[pumpkin_data::Property::Level(level)])
        .to_state_id(block)
}

/// Puts `state` in place, plays `sound`, takes one held item (unless creative), hands one
/// `result` item to the player and counts `stat`.
fn exchange(
    args: &mut UseWithItemArgs<'_>,
    gamemode: GameMode,
    state: BlockStateId,
    sound: Sound,
    result: &'static Item,
    stat: CustomStatistic,
) -> BlockActionResult {
    args.world
        .set_block_state(args.position, state, BlockFlags::NOTIFY_ALL);
    args.world
        .play_sound(sound, SoundCategory::Blocks, &args.position.to_f64());
    args.item_stack.decrement_unless_creative(gamemode, 1);
    give_item_or_drop(args.player, args.world, result);
    args.player
        .increment_stat(StatisticCategory::Custom, stat as i32, 1);
    BlockActionResult::Success
}

/// A water cauldron one level lower: an empty cauldron below level 1.
fn lower_water_level(world: &Arc<crate::world::World>, position: &BlockPos, level: u8) {
    let next_level = level - 1;
    if next_level > 0 {
        let new_state_id = Block::WATER_CAULDRON
            .from_properties(&[pumpkin_data::Property::Level(next_level)])
            .to_state_id(&Block::WATER_CAULDRON);
        world.set_block_state(position, new_state_id, BlockFlags::NOTIFY_ALL);
    } else {
        world.set_block_state(
            position,
            Block::CAULDRON.default_state.id,
            BlockFlags::NOTIFY_ALL,
        );
    }
}

/// An empty cauldron filled from a bucket or a potion.
fn fill_empty(
    args: &mut UseWithItemArgs<'_>,
    item_id: u16,
    gamemode: GameMode,
) -> Option<BlockActionResult> {
    if item_id == Item::WATER_BUCKET.id {
        if !fire_cauldron_change(
            args.world,
            *args.position,
            0,
            3,
            crate::plugin::block::cauldron_level_change::CauldronChangeReason::BucketEmpty,
            Some(Arc::clone(args.player) as Arc<dyn crate::entity::EntityBase>),
        ) {
            return Some(BlockActionResult::Pass);
        }
        let state_id = level_state(&Block::WATER_CAULDRON, 3);
        Some(exchange(
            args,
            gamemode,
            state_id,
            Sound::ItemBucketEmpty,
            &Item::BUCKET,
            CustomStatistic::FillCauldron,
        ))
    } else if item_id == Item::LAVA_BUCKET.id {
        Some(exchange(
            args,
            gamemode,
            Block::LAVA_CAULDRON.default_state.id,
            Sound::ItemBucketEmptyLava,
            &Item::BUCKET,
            CustomStatistic::FillCauldron,
        ))
    } else if item_id == Item::POWDER_SNOW_BUCKET.id {
        let state_id = level_state(&Block::POWDER_SNOW_CAULDRON, 3);
        Some(exchange(
            args,
            gamemode,
            state_id,
            Sound::ItemBucketEmptyPowderSnow,
            &Item::BUCKET,
            CustomStatistic::FillCauldron,
        ))
    } else if item_id == Item::POTION.id {
        let state_id = level_state(&Block::WATER_CAULDRON, 1);
        Some(exchange(
            args,
            gamemode,
            state_id,
            Sound::ItemBottleEmpty,
            &Item::GLASS_BOTTLE,
            CustomStatistic::UseCauldron,
        ))
    } else {
        None
    }
}

/// A full cauldron emptied into a bucket; `None` when there is nothing to take.
fn take_with_bucket(
    args: &mut UseWithItemArgs<'_>,
    block_id: BlockId,
    gamemode: GameMode,
) -> Option<BlockActionResult> {
    let state_id = args.world.get_block_state_id(args.position);
    let (filled_item, sound) = if block_id == BlockId::WATER_CAULDRON {
        let props = WaterCauldronLikeProperties::from_state_id(state_id);
        if props.level == 3 {
            (Some(&Item::WATER_BUCKET), Sound::ItemBucketFill)
        } else {
            (None, Sound::ItemBucketFill)
        }
    } else if block_id == BlockId::LAVA_CAULDRON {
        (Some(&Item::LAVA_BUCKET), Sound::ItemBucketFillLava)
    } else if block_id == BlockId::POWDER_SNOW_CAULDRON {
        let props = WaterCauldronLikeProperties::from_state_id(state_id);
        if props.level == 3 {
            (
                Some(&Item::POWDER_SNOW_BUCKET),
                Sound::ItemBucketFillPowderSnow,
            )
        } else {
            (None, Sound::ItemBucketFillPowderSnow)
        }
    } else {
        (None, Sound::ItemBucketFill)
    };

    let result_item = filled_item?;
    Some(exchange(
        args,
        gamemode,
        Block::CAULDRON.default_state.id,
        sound,
        result_item,
        CustomStatistic::UseCauldron,
    ))
}

/// The uses of a water cauldron: add a potion, fill a bottle, wash a shulker box, a banner or a
/// dyed item.
fn use_on_water(
    args: &mut UseWithItemArgs<'_>,
    item_id: u16,
    gamemode: GameMode,
) -> Option<BlockActionResult> {
    let state_id = args.world.get_block_state_id(args.position);
    let props = WaterCauldronLikeProperties::from_state_id(state_id);
    if props.level == 0 {
        return None;
    }
    if item_id == Item::POTION.id && props.level < 3 {
        let new_state_id = level_state(&Block::WATER_CAULDRON, props.level + 1);
        return Some(exchange(
            args,
            gamemode,
            new_state_id,
            Sound::ItemBottleEmpty,
            &Item::GLASS_BOTTLE,
            CustomStatistic::UseCauldron,
        ));
    }

    if item_id == Item::GLASS_BOTTLE.id {
        lower_water_level(args.world, args.position, props.level);
        args.world.play_sound(
            Sound::ItemBottleFill,
            SoundCategory::Blocks,
            &args.position.to_f64(),
        );
        args.item_stack.decrement_unless_creative(gamemode, 1);
        give_stack_or_drop(
            args.player,
            args.world,
            crate::item::items::glass_bottle::water_bottle(),
        );
        args.player.increment_stat(
            StatisticCategory::Custom,
            CustomStatistic::UseCauldron as i32,
            1,
        );
        return Some(BlockActionResult::Success);
    }

    if args
        .item_stack
        .item
        .has_tag(&pumpkin_data::tag::Item::MINECRAFT_SHULKER_BOXES)
        && args.item_stack.item.id != Item::SHULKER_BOX.id
    {
        lower_water_level(args.world, args.position, props.level);
        args.item_stack.item = &Item::SHULKER_BOX;
        args.player.increment_stat(
            StatisticCategory::Custom,
            CustomStatistic::CleanShulkerBox as i32,
            1,
        );
        return Some(BlockActionResult::Success);
    }

    if args
        .item_stack
        .get_data_component::<pumpkin_data::data_component_impl::BannerPatternsImpl>()
        .is_some()
    {
        return Some(wash(
            args,
            props.level,
            pumpkin_data::data_component::DataComponent::BannerPatterns,
            CustomStatistic::CleanBanner,
        ));
    }

    if args
        .item_stack
        .get_data_component::<pumpkin_data::data_component_impl::DyedColorImpl>()
        .is_some()
    {
        return Some(wash(
            args,
            props.level,
            pumpkin_data::data_component::DataComponent::DyedColor,
            CustomStatistic::CleanArmor,
        ));
    }
    None
}

/// Washes `component` off the held item with one level of water and counts `stat`.
fn wash(
    args: &mut UseWithItemArgs<'_>,
    level: u8,
    component: pumpkin_data::data_component::DataComponent,
    stat: CustomStatistic,
) -> BlockActionResult {
    lower_water_level(args.world, args.position, level);
    args.item_stack.remove_data_component(component);
    args.player
        .increment_stat(StatisticCategory::Custom, stat as i32, 1);
    BlockActionResult::Success
}

impl BlockBehaviour for CauldronBlock {
    fn use_with_item(&self, mut args: UseWithItemArgs<'_>) -> BlockActionResult {
        let item_id = args.item_stack.item.id;
        let block_id = args.block.id;
        let gamemode = args.player.gamemode.load();

        if block_id == BlockId::CAULDRON
            && let Some(result) = fill_empty(&mut args, item_id, gamemode)
        {
            return result;
        }
        if item_id == Item::BUCKET.id
            && let Some(result) = take_with_bucket(&mut args, block_id, gamemode)
        {
            return result;
        }
        if block_id == BlockId::WATER_CAULDRON
            && let Some(result) = use_on_water(&mut args, item_id, gamemode)
        {
            return result;
        }
        BlockActionResult::PassToDefaultBlockAction
    }

    fn get_comparator_output(&self, args: GetComparatorOutputArgs<'_>) -> Option<u8> {
        match args.block.id {
            BlockId::WATER_CAULDRON | BlockId::POWDER_SNOW_CAULDRON => {
                let state_id = args.world.get_block_state_id(args.position);
                let props = WaterCauldronLikeProperties::from_state_id(state_id);
                Some(props.level)
            }
            BlockId::LAVA_CAULDRON => Some(3),
            _ => Some(0),
        }
    }

    fn is_pathfindable(&self, _state: &BlockState, _computation_type: PathComputationType) -> bool {
        false
    }
}
