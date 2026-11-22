//! Which behaviour `DispenserBlock::dispense` runs for a stack: the item class decides, the first
//! match wins (the order of the vanilla `DispenseItemBehavior` registry as far as it is ported).

use pumpkin_data::Block;
use pumpkin_data::entity::entity_from_egg;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::tag::{self, Taggable};
use pumpkin_data::world::WorldEvent;

use super::{DispenseContext, DispenserBlock, is_water_bottle};
use crate::block::entities::dispenser::DispenserBlockEntity;
use crate::item::ItemMetadata;
use crate::item::items::boat::BoatItem;
use crate::item::items::bucket::FilledBucketItem;
use crate::item::items::minecart::MinecartItem;

impl DispenserBlock {
    /// Routes the stack to its behaviour: projectiles, vehicles and primed blocks first, then the rest.
    // Still missing some specific dispenser behavior that you can find here:
    // https://minecraft.wiki/w/Dispenser#Usage
    pub(super) fn route(
        ctx: &DispenseContext<'_>,
        dispenser: &DispenserBlockEntity,
        item: &mut ItemStack,
    ) {
        let arrows = [
            Item::ARROW.id,
            Item::TIPPED_ARROW.id,
            Item::SPECTRAL_ARROW.id,
        ];
        let boats = BoatItem::ids();

        if arrows.contains(&item.item.id) {
            // Arrows
            Self::fire_arrow(ctx, item);
        } else if boats.contains(&item.item.id) {
            // Boats
            if !Self::dispense_boat(ctx, item) {
                Self::drop_item(ctx, item);
            }
        } else if MinecartItem::ids().contains(&item.item.id) {
            // Minecarts
            if !Self::dispense_minecart(ctx, item) {
                Self::drop_item(ctx, item);
            }
        } else if item.item.id == Item::ARMOR_STAND.id {
            // Armor stands
            if !Self::dispense_armor_stand(ctx, item) {
                Self::drop_item(ctx, item);
            }
        } else if item.item.id == Item::TNT.id {
            // TNT
            Self::dispense_tnt(ctx, item);
        } else if item.item.id == Item::SNOWBALL.id {
            Self::dispense_snowball(ctx, item);
        } else if item.item.id == Item::EGG.id {
            Self::dispense_egg(ctx, item);
        } else if item.item.id == Item::SPLASH_POTION.id {
            Self::dispense_splash_potion(ctx, item);
        } else if item.item.id == Item::LINGERING_POTION.id {
            Self::dispense_lingering_potion(ctx, item);
        } else if item.item.id == Item::FIRE_CHARGE.id {
            Self::dispense_fire_charge(ctx, item);
        } else if item.item.id == Item::WIND_CHARGE.id {
            Self::dispense_wind_charge(ctx, item);
        } else if item.item.id == Item::FIREWORK_ROCKET.id {
            Self::dispense_firework_rocket(ctx, item);
        } else {
            Self::route_tools(ctx, dispenser, item);
        }
    }

    /// Buckets, fire, honeycombs, eggs, shears and bottles; hands anything else on.
    fn route_tools(
        ctx: &DispenseContext<'_>,
        dispenser: &DispenserBlockEntity,
        item: &mut ItemStack,
    ) {
        if item.item.id == Item::BUCKET.id {
            // Empty buckets pick up the fluid in front of the dispenser
            Self::dispense_empty_bucket(ctx, dispenser, item);
        } else if FilledBucketItem::ids().contains(&item.item.id) {
            // Filled buckets place their fluid in front of the dispenser
            Self::dispense_filled_bucket(ctx, item);
        } else if item.item.id == Item::FLINT_AND_STEEL.id {
            // Flint and steel light fires and prime TNT
            Self::dispense_flint_and_steel(ctx, item);
        } else if item.item.id == Item::HONEYCOMB.id {
            // Honeycombs wax copper blocks
            Self::dispense_honeycomb(ctx, item);
        } else if entity_from_egg(item.item.id).is_some() {
            // Spawn eggs
            Self::dispense_spawn_egg(ctx, item);
        } else if item.item.id == Item::SHEARS.id {
            // Shears harvest full beehives and shear sheep
            Self::dispense_shears(ctx, item);
        } else if item.item.id == Item::GLASS_BOTTLE.id {
            // Glass bottles fill from water and full beehives
            if !Self::dispense_glass_bottle(ctx, dispenser, item) {
                Self::drop_item_with_sound(ctx, item, WorldEvent::SoundDispenserFail);
            }
        } else if is_water_bottle(item) {
            // Water bottles convert dirt-likes into mud
            if !Self::dispense_water_bottle(ctx, item) {
                Self::drop_item(ctx, item);
            }
        } else if item.item.id == Item::GLOWSTONE.id {
            match Self::dispense_glowstone(ctx, item) {
                Some(true) => Self::play_dispense_effects(ctx, WorldEvent::SoundDispenserDispense),
                Some(false) => Self::play_dispense_effects(ctx, WorldEvent::SoundDispenserFail),
                None => Self::drop_item(ctx, item),
            }
        } else {
            Self::route_placed(ctx, item);
        }
    }

    /// Skulls, pumpkins, shulker boxes, equipment, experience bottles and the default drop.
    fn route_placed(ctx: &DispenseContext<'_>, item: &mut ItemStack) {
        if item.item.id == Item::WITHER_SKELETON_SKULL.id {
            // Placed only when it completes a wither, otherwise worn as a helmet
            Self::dispense_mob_head(ctx, item, &Block::WITHER_SKELETON_SKULL);
        } else if item.item.id == Item::CARVED_PUMPKIN.id {
            // Placed only when it completes a golem, otherwise worn as a helmet
            Self::dispense_mob_head(ctx, item, &Block::CARVED_PUMPKIN);
        } else if Block::from_item_id(item.item.id)
            .is_some_and(|block| block.has_tag(&tag::Block::MINECRAFT_SHULKER_BOXES))
        {
            // Shulker boxes place themselves
            if !Self::dispense_shulker_box(ctx, item) {
                Self::play_dispense_effects(ctx, WorldEvent::SoundDispenserFail);
            }
        } else if Self::dispense_equipment(ctx, item) {
            // Armor, elytra, heads, saddles, horse/wolf armor and llama carpets
            Self::play_dispense_effects(ctx, WorldEvent::SoundDispenserDispense);
        } else if item.item.id == Item::EXPERIENCE_BOTTLE.id {
            // Experience bottles (bottles o' enchanting) are thrown like splash potions
            Self::dispense_experience_bottle(ctx, item);
        } else {
            // TODO: Bone meal, chests onto llamas, brushes onto armadillos
            // Default / Drop
            Self::drop_item(ctx, item);
        }
    }
}
