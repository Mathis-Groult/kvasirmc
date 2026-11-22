//! The block entity each type name builds in its default state, split into three groups of arms
//! that `create_block_entity` tries in turn (the names are unique, so the first `Some` wins).

use std::sync::Arc;

use pumpkin_util::math::position::BlockPos;

use super::BlockEntity;

/// Containers, signs and the blocks that hold items or text.
pub(super) fn storage_and_signs(name: &str, position: BlockPos) -> Option<Arc<dyn BlockEntity>> {
    match name {
        "furnace" => Some(Arc::new(super::furnace::FurnaceBlockEntity::new(position))),
        "chest" => Some(Arc::new(super::chest::ChestBlockEntity::new(position))),
        "trapped_chest" => Some(Arc::new(
            super::trapped_chest::TrappedChestBlockEntity::new(position),
        )),
        "ender_chest" => Some(Arc::new(super::ender_chest::EnderChestBlockEntity::new(
            position,
        ))),
        "jukebox" => Some(Arc::new(super::jukebox::JukeboxBlockEntity::new(position))),
        "dispenser" => Some(Arc::new(super::dispenser::DispenserBlockEntity::new(
            position,
        ))),
        "dropper" => Some(Arc::new(super::dropper::DropperBlockEntity::new(position))),
        "sign" => Some(Arc::new(super::sign::SignBlockEntity::empty(position))),
        "hanging_sign" => Some(Arc::new(
            super::hanging_sign::HangingSignBlockEntity::empty(position),
        )),
        "mob_spawner" => Some(Arc::new(super::mob_spawner::MobSpawnerBlockEntity::new(
            position, None,
        ))),
        "creaking_heart" => Some(Arc::new(
            super::creaking_heart::CreakingHeartBlockEntity::new(position),
        )),
        "piston" => Some(Arc::new(super::piston::PistonBlockEntity::from_nbt(
            &pumpkin_nbt::compound::NbtCompound::new(),
            position,
        ))),
        "brewing_stand" => Some(Arc::new(
            super::brewing_stand::BrewingStandBlockEntity::new(position),
        )),
        "enchanting_table" => Some(Arc::new(
            super::enchanting_table::EnchantingTableBlockEntity::new(position),
        )),
        "end_portal" => Some(Arc::new(super::end_portal::EndPortalBlockEntity::new(
            position,
        ))),
        "beacon" => Some(Arc::new(super::beacon::BeaconBlockEntity::new(position))),
        "skull" => Some(Arc::new(super::skull::SkullBlockEntity::new(position))),
        _ => None,
    }
}

/// Machines and the functional blocks of the overworld and the end.
pub(super) fn machines_and_markers(name: &str, position: BlockPos) -> Option<Arc<dyn BlockEntity>> {
    use pumpkin_data::block_properties::FacingHopper;
    match name {
        "daylight_detector" => Some(Arc::new(
            super::daylight_detector::DaylightDetectorBlockEntity::new(position),
        )),
        "hopper" => Some(Arc::new(super::hopper::HopperBlockEntity::new(
            position,
            FacingHopper::Down,
        ))),
        "comparator" => Some(Arc::new(super::comparator::ComparatorBlockEntity::new(
            position,
        ))),
        "banner" => Some(Arc::new(super::banner::BannerBlockEntity::new(position))),
        "structure_block" => Some(Arc::new(
            super::structure_block::StructureBlockBlockEntity::new(position),
        )),
        "end_gateway" => Some(Arc::new(super::end_gateway::EndGatewayBlockEntity::new(
            position,
        ))),
        "command_block" => Some(Arc::new(super::command_block::CommandBlockEntity::new(
            position, true, false,
        ))),
        "shulker_box" => Some(Arc::new(super::shulker_box::ShulkerBoxBlockEntity::new(
            position,
        ))),
        "conduit" => Some(Arc::new(super::conduit::ConduitBlockEntity::new(position))),
        "barrel" => Some(Arc::new(super::barrel::BarrelBlockEntity::new(position))),
        "smoker" => Some(Arc::new(super::smoker::SmokerBlockEntity::new(position))),
        "blast_furnace" => Some(Arc::new(
            super::blasting_furnace::BlastingFurnaceBlockEntity::new(position),
        )),
        "lectern" => Some(Arc::new(super::lectern::LecternBlockEntity::new(position))),
        "bell" => Some(Arc::new(super::bell::BellBlockEntity::new(position))),
        "jigsaw" => Some(Arc::new(super::jigsaw_block::JigsawBlockEntity::new(
            position,
        ))),
        "campfire" => Some(Arc::new(super::campfire::CampfireBlockEntity::new(
            position,
        ))),
        "beehive" => Some(Arc::new(super::beehive::BeehiveBlockEntity::new(position))),
        _ => None,
    }
}

/// Sculk, trial and the newer blocks.
pub(super) fn sculk_and_misc(name: &str, position: BlockPos) -> Option<Arc<dyn BlockEntity>> {
    match name {
        "sculk_sensor" => Some(Arc::new(super::sculk_sensor::SculkSensorBlockEntity::new(
            position,
        ))),
        "calibrated_sculk_sensor" => Some(Arc::new(
            super::calibrated_sculk_sensor::CalibratedSculkSensorBlockEntity::new(position),
        )),
        "sculk_catalyst" => Some(Arc::new(
            super::sculk_catalyst::SculkCatalystBlockEntity::new(position),
        )),
        "sculk_shrieker" => Some(Arc::new(
            super::sculk_shrieker::SculkShriekerBlockEntity::new(position),
        )),
        "chiseled_bookshelf" => Some(Arc::new(
            super::chiseled_bookshelf::ChiseledBookshelfBlockEntity::new(position),
        )),
        "shelf" => Some(Arc::new(super::shelf::ShelfBlockEntity::new(position))),
        "brushable_block" => Some(Arc::new(
            super::brushable_block::BrushableBlockBlockEntity::new(position),
        )),
        "decorated_pot" => Some(Arc::new(
            super::decorated_pot::DecoratedPotBlockEntity::new(position),
        )),
        "crafter" => Some(Arc::new(super::crafter::CrafterBlockEntity::new(position))),
        "trial_spawner" => Some(Arc::new(
            super::trial_spawner::TrialSpawnerBlockEntity::new(position),
        )),
        "vault" => Some(Arc::new(super::vault::VaultBlockEntity::new(position))),
        "test_block" => Some(Arc::new(super::test_block::TestBlockBlockEntity::new(
            position,
        ))),
        "test_instance_block" => Some(Arc::new(
            super::test_instance_block::TestInstanceBlockBlockEntity::new(position),
        )),
        "copper_golem_statue" => Some(Arc::new(
            super::copper_golem_statue::CopperGolemStatueBlockEntity::new(position),
        )),
        "potent_sulfur" => Some(Arc::new(
            super::potent_sulfur::PotentSulfurBlockEntity::new(position),
        )),
        "map" => Some(Arc::new(super::map::MapBlockEntity::new(position, 0))),
        _ => None,
    }
}
