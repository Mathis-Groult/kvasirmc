//! Rolling the offers of a villager's trade set (`VillagerEntity::add_trades`).

use pumpkin_data::item_stack::ItemStack;
use pumpkin_protocol::java::client::play::MerchantOffer;

use super::{
    VillagerEntity, VillagerProfession, apply_potion, apply_random_dye, apply_random_stew_effect,
    enchant_trade_item, enchanted_book_offer_items,
};
use crate::data::datapack::trade_loader::{
    DynamicTradeModifier, DynamicVillagerTrade, DynamicVillagerTradeSet,
};
use crate::entity::EntityBase;

/// The three stacks of an offer, which a trade modifier rewrites.
struct OfferStacks {
    base_cost_a: ItemStack,
    output: ItemStack,
    cost_b: Option<ItemStack>,
}

impl VillagerEntity {
    /// The datapack's trade set for `profession` at `level`, else the built-in one.
    fn resolve_trade_set(
        &self,
        profession: VillagerProfession,
        level: i32,
    ) -> Option<DynamicVillagerTradeSet> {
        self.get_entity()
            .world
            .load()
            .server
            .upgrade()
            .and_then(|server| {
                server
                    .datapack_manager
                    .get_villager_trade_set(profession.to_name(), level)
            })
            .or_else(|| {
                profession
                    .trade_set(level)
                    .map(DynamicVillagerTradeSet::from)
            })
    }

    /// Draws trades from the set, at random and without repeats, until `amount` offers were built.
    pub(super) fn roll_trade_offers(
        &self,
        profession: VillagerProfession,
        level: i32,
    ) -> Vec<MerchantOffer> {
        use rand::{RngExt, SeedableRng, rngs::StdRng};

        let villager_type = self
            .villager_data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .type_enum();
        let mut new_offers = Vec::new();
        let Some(trade_set) = self.resolve_trade_set(profession, level) else {
            return new_offers;
        };

        let mut rng = StdRng::from_rng(&mut rand::rng());
        let mut remaining_trades = trade_set.trades.clone();
        let mut added = 0;
        while added < trade_set.amount && !remaining_trades.is_empty() {
            let index = rng.random_range(0..remaining_trades.len());
            let trade = remaining_trades.remove(index);
            if !trade.allowed_types.is_empty() && !trade.allowed_types.contains(&villager_type) {
                continue;
            }
            if let Some(offer) = self.build_offer(&mut rng, &trade) {
                new_offers.push(offer);
                added += 1;
            }
        }
        new_offers
    }

    /// One offer from `trade`, or `None` when its modifier cannot produce an item.
    fn build_offer(
        &self,
        rng: &mut rand::rngs::StdRng,
        trade: &DynamicVillagerTrade,
    ) -> Option<MerchantOffer> {
        use pumpkin_protocol::codec::item_stack_seralizer::ItemStackSerializer;
        use std::borrow::Cow;

        let mut stacks = OfferStacks {
            base_cost_a: ItemStack::new(trade.wants.count as u8, trade.wants.item),
            output: ItemStack::new(trade.gives.count as u8, trade.gives.item),
            cost_b: trade
                .wants_b
                .as_ref()
                .map(|b| ItemStack::new(b.count as u8, b.item)),
        };
        self.apply_trade_modifier(rng, trade, &mut stacks)?;
        Some(MerchantOffer {
            base_cost_a: ItemStackSerializer(Cow::Owned(stacks.base_cost_a)),
            output: ItemStackSerializer(Cow::Owned(stacks.output)),
            cost_b: stacks
                .cost_b
                .map(|stack| ItemStackSerializer(Cow::Owned(stack))),
            reward_exp: true,
            uses: 0,
            max_uses: trade.max_uses,
            xp: trade.xp,
            special_price: 0,
            price_multiplier: trade.price_multiplier,
            demand: 0,
        })
    }

    /// Applies the trade's modifier to the stacks; `None` skips the trade.
    fn apply_trade_modifier(
        &self,
        rng: &mut rand::rngs::StdRng,
        trade: &DynamicVillagerTrade,
        stacks: &mut OfferStacks,
    ) -> Option<()> {
        use rand::seq::IndexedRandom;

        match &trade.modifier {
            DynamicTradeModifier::None => {}
            DynamicTradeModifier::EnchantRandomly => {
                let items = enchanted_book_offer_items(rng)?;
                (stacks.base_cost_a, stacks.output, stacks.cost_b) = items;
            }
            DynamicTradeModifier::EnchantWithLevels { min, max } => {
                let (enchanted, additional_cost) =
                    enchant_trade_item(rng, trade.gives.item, *min, *max)?;
                stacks.output = enchanted;
                let count = i32::from(stacks.base_cost_a.item_count)
                    .saturating_add(additional_cost)
                    .clamp(0, i32::from(stacks.base_cost_a.get_max_stack_size()));
                if count == 0 {
                    return None;
                }
                stacks.base_cost_a.set_count(count as u8);
            }
            DynamicTradeModifier::ExplorationMap { destination } => {
                stacks.output = self.create_explorer_map(destination, &stacks.output)?;
            }
            DynamicTradeModifier::RandomDyes => apply_random_dye(rng, &mut stacks.output),
            DynamicTradeModifier::RandomPotion => {
                let potion_name = pumpkin_data::tag::Potion::MINECRAFT_TRADEABLE
                    .0
                    .choose(rng)?;
                apply_potion(&mut stacks.output, potion_name);
            }
            DynamicTradeModifier::SuspiciousStew => {
                apply_random_stew_effect(rng, &mut stacks.output);
            }
            DynamicTradeModifier::Potion(potion) => apply_potion(&mut stacks.output, potion),
        }
        Some(())
    }
}
