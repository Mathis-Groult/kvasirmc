//! The fixed part of the generated villager file.

use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn imports_and_villager_trade_item_tokens() -> TokenStream {
    quote! {
        use serde::Serialize;

        #[derive(Clone, Copy, PartialEq, Eq)]
        pub struct VillagerTradeItem {
            pub item: &'static crate::item::Item,
            pub count: i32,
        }

        #[derive(Clone, Copy, PartialEq)]
        pub struct VillagerTrade {
            pub wants: VillagerTradeItem,
            pub wants_b: Option<VillagerTradeItem>,
            pub gives: VillagerTradeItem,
            pub max_uses: i32,
            pub xp: i32,
            pub price_multiplier: f32,
            pub modifier: VillagerTradeModifier,
            pub allowed_types: &'static [VillagerType],
        }

        #[derive(Clone, Copy, PartialEq, Eq)]
        pub enum VillagerTradeModifier {
            None,
            EnchantRandomly,
            EnchantWithLevels { min: i32, max: i32 },
            ExplorationMap { destination: &'static str },
            RandomDyes,
            RandomPotion,
            SuspiciousStew,
            Potion(&'static str),
        }

        #[derive(Clone, Copy, PartialEq)]
        pub struct VillagerTradeSet {
            pub trades: &'static [VillagerTrade],
            pub amount: i32,
        }
    }
}
