//! What `Mob::mob_write_nbt` saves for a villager, one writer per group of fields.

use std::sync::PoisonError;
use std::sync::atomic::Ordering;

use pumpkin_data::item_stack::ItemStack;
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;
use pumpkin_protocol::java::client::play::MerchantOffer;
use uuid::Uuid;

use super::{GossipType, VillagerEntity};
use crate::entity::mob::Mob;

/// A stack as the compound a trade or the inventory stores it in.
fn stack_compound(stack: &ItemStack) -> NbtCompound {
    let mut compound = NbtCompound::new();
    stack.write_item_stack(&mut compound);
    compound
}

/// One trade: its stacks and every number of the offer.
fn offer_compound(offer: &MerchantOffer) -> NbtCompound {
    let mut recipe = NbtCompound::new();
    recipe.put_compound("buy", stack_compound(offer.base_cost_a.0.as_ref()));
    recipe.put_compound("sell", stack_compound(offer.output.0.as_ref()));
    if let Some(cost_b) = &offer.cost_b {
        recipe.put_compound("buyB", stack_compound(cost_b.0.as_ref()));
    }
    recipe.put_int("uses", offer.uses);
    recipe.put_int("maxUses", offer.max_uses);
    recipe.put_bool("rewardExp", offer.reward_exp);
    recipe.put_int("xp", offer.xp);
    recipe.put_float("priceMultiplier", offer.price_multiplier);
    recipe.put_int("specialPrice", offer.special_price);
    recipe.put_int("demand", offer.demand);
    recipe
}

/// One gossip about `target`, whose UUID is saved as four ints.
fn gossip_compound(target: Uuid, gossip_type: GossipType, value: i32) -> NbtCompound {
    let (u1, u2) = target.as_u64_pair();
    let uuid_array = vec![(u1 >> 32) as i32, u1 as i32, (u2 >> 32) as i32, u2 as i32];
    let mut gossip = NbtCompound::new();
    gossip.put("Target", NbtTag::IntArray(uuid_array));
    gossip.put_string("Type", gossip_type.name().to_string());
    gossip.put_int("Value", value);
    gossip
}

impl VillagerEntity {
    pub(super) fn write_villager_data(&self, nbt: &mut NbtCompound) {
        let data = self
            .villager_data
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut villager_data_nbt = NbtCompound::new();
        villager_data_nbt.put_int("Type", data.r#type.0);
        villager_data_nbt.put_int("Profession", data.profession.0);
        villager_data_nbt.put_int("Level", data.level.0);
        nbt.put_compound("VillagerData", villager_data_nbt);
    }

    pub(super) fn write_counters(&self, nbt: &mut NbtCompound) {
        nbt.put_int("FoodLevel", self.food_level.load(Ordering::Relaxed));
        nbt.put_int("Xp", self.xp.load(Ordering::Relaxed));
        nbt.put_long(
            "LastRestock",
            self.last_restock_time.load(Ordering::Relaxed),
        );
        nbt.put_int("RestocksToday", self.restocks_today.load(Ordering::Relaxed));
        nbt.put_long(
            "LastGossipDecay",
            self.last_gossip_decay_time.load(Ordering::Relaxed),
        );

        let last_gossip_share = self.last_gossip_share_time.load(Ordering::Relaxed);
        if last_gossip_share > 0 {
            nbt.put_long("LastGossipShare", last_gossip_share);
        }

        let last_worked = self.last_worked_at_poi.load(Ordering::Relaxed);
        if last_worked > 0 {
            nbt.put_long("LastWorkedAtPoi", last_worked);
        }
    }

    pub(super) fn write_places(&self, nbt: &mut NbtCompound) {
        if let Some(pos) = self.get_job_site() {
            nbt.put_int("JobSiteX", pos.0.x);
            nbt.put_int("JobSiteY", pos.0.y);
            nbt.put_int("JobSiteZ", pos.0.z);
            nbt.put_bool(
                "JobSitePending",
                self.job_site_pending.load(Ordering::Relaxed),
            );
        }

        if let Some(pos) = self.get_home() {
            nbt.put_int("HomeX", pos.0.x);
            nbt.put_int("HomeY", pos.0.y);
            nbt.put_int("HomeZ", pos.0.z);
        }
    }

    pub(super) fn write_offers(&self, nbt: &mut NbtCompound) {
        let offers = self.offers.lock().unwrap_or_else(PoisonError::into_inner);
        if offers.is_empty() {
            return;
        }
        let recipes = offers
            .iter()
            .map(|offer| NbtTag::Compound(offer_compound(offer)))
            .collect();
        let mut offers_compound = NbtCompound::new();
        offers_compound.put("Recipes", NbtTag::List(recipes));
        nbt.put_compound("Offers", offers_compound);
    }

    pub(super) fn write_inventory(&self, nbt: &mut NbtCompound) {
        let inventory = self
            .inventory
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if inventory.is_empty() {
            return;
        }
        let inventory_list = inventory
            .iter()
            .map(|item| NbtTag::Compound(stack_compound(item)))
            .collect();
        nbt.put("Inventory", NbtTag::List(inventory_list));
    }

    pub(super) fn write_gossips(&self, nbt: &mut NbtCompound) {
        let gossips = self.gossips.lock().unwrap_or_else(PoisonError::into_inner);
        if gossips.is_empty() {
            return;
        }
        let mut gossip_list = Vec::new();
        for (uuid, entries) in gossips.iter() {
            for (gossip_type, val) in entries {
                gossip_list.push(NbtTag::Compound(gossip_compound(*uuid, *gossip_type, *val)));
            }
        }
        nbt.put("Gossips", NbtTag::List(gossip_list));
    }
}
