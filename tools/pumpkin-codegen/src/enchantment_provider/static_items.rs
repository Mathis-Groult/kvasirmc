//! The fixed part of the generated enchantment provider file.

use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn imports_and_enchantment_provider_kind_tokens() -> TokenStream {
    quote! {
        use rand::RngExt;
        use crate::enchantment::Enchantment;
        use crate::item::Item;
        use crate::item_stack::ItemStack;

        #[derive(Clone, Copy, PartialEq)]
        pub enum EnchantmentProviderKind {
            Single {
                enchantment: &'static Enchantment,
                level: i32,
            },
            ByCostWithDifficulty {
                enchantments: &'static [&'static Enchantment],
                min_cost: i32,
                max_cost_span: i32,
            },
            ByCost {
                enchantments: &'static [&'static Enchantment],
                cost: i32,
            },
        }

        impl core::fmt::Debug for EnchantmentProviderKind {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                match self {
                    Self::Single { enchantment, level } => f
                        .debug_struct("Single")
                        .field("enchantment", &enchantment.registry_key)
                        .field("level", level)
                        .finish(),
                    Self::ByCostWithDifficulty {
                        enchantments,
                        min_cost,
                        max_cost_span,
                    } => {
                        let keys: Vec<&str> = enchantments.iter().map(|e| e.registry_key).collect();
                        f.debug_struct("ByCostWithDifficulty")
                            .field("enchantments", &keys)
                            .field("min_cost", min_cost)
                            .field("max_cost_span", max_cost_span)
                            .finish()
                    }
                    Self::ByCost {
                        enchantments,
                        cost,
                    } => {
                        let keys: Vec<&str> = enchantments.iter().map(|e| e.registry_key).collect();
                        f.debug_struct("ByCost")
                            .field("enchantments", &keys)
                            .field("cost", cost)
                            .finish()
                    }
                }
            }
        }

        #[derive(Clone, Copy, PartialEq)]
        pub struct EnchantmentProvider {
            pub name: &'static str,
            pub kind: EnchantmentProviderKind,
        }

        impl core::fmt::Debug for EnchantmentProvider {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.debug_struct("EnchantmentProvider")
                    .field("name", &self.name)
                    .field("kind", &self.kind)
                    .finish()
            }
        }
    }
}

pub(super) fn name_tokens() -> TokenStream {
    quote! {
        #[must_use]
        pub const fn name(&self) -> &'static str {
            self.name
        }

        #[must_use]
        pub const fn kind(&self) -> &EnchantmentProviderKind {
            &self.kind
        }
    }
}

pub(super) fn select_enchantments_tokens() -> TokenStream {
    quote! {
        /// Evaluates the provider for the given item and difficulty multiplier, returning
        /// the selected enchantments and their levels.
        #[must_use]
        pub fn select_enchantments(
            &self,
            item: &'static Item,
            special_multiplier: f32,
        ) -> Vec<(&'static Enchantment, i32)> {
            match &self.kind {
                EnchantmentProviderKind::Single { enchantment, level } => {
                    vec![(*enchantment, *level)]
                }
                EnchantmentProviderKind::ByCostWithDifficulty {
                    enchantments,
                    min_cost,
                    max_cost_span,
                } => {
                    let cost = *min_cost + (special_multiplier * *max_cost_span as f32).round() as i32;
                    Self::select_by_cost(item, enchantments, cost)
                }
                EnchantmentProviderKind::ByCost {
                    enchantments,
                    cost,
                } => Self::select_by_cost(item, enchantments, *cost),
            }
        }

        /// Evaluates and applies the enchantments directly to an `ItemStack`.
        pub fn apply_to_stack(&self, stack: &mut ItemStack, special_multiplier: f32) {
            let enchantments = self.select_enchantments(stack.item, special_multiplier);
            for (enchantment, level) in enchantments {
                stack.enchant(enchantment, level);
            }
        }
    }
}

pub(super) fn select_by_cost_tokens() -> TokenStream {
    quote! {
        /// Selects enchantments by cost with conflict resolution, matching vanilla
        /// `EnchantmentHelper.enchantItem`.
        #[must_use]
        pub fn select_by_cost(
            item: &'static Item,
            possible_enchantments: &[&'static Enchantment],
            initial_cost: i32,
        ) -> Vec<(&'static Enchantment, i32)> {
            let candidates: Vec<&'static Enchantment> = possible_enchantments
                .iter()
                .copied()
                .filter(|e| e.can_enchant(item))
                .collect();

            if candidates.is_empty() || initial_cost <= 0 {
                return Vec::new();
            }

            let mut cost = initial_cost;
            let mut applied: Vec<(&'static Enchantment, i32)> = Vec::new();
            let mut rng = rand::rng();

            loop {
                let available: Vec<&'static Enchantment> = candidates
                    .iter()
                    .copied()
                    .filter(|cand| {
                        !applied.iter().any(|(app, _)| app.id == cand.id)
                            && applied.iter().all(|(app, _)| cand.are_compatible(app))
                    })
                    .collect();

                if available.is_empty() {
                    break;
                }

                let total_weight: f32 = available.iter().map(|e| e.weight as f32).sum();
                if total_weight <= 0.0 {
                    break;
                }

                let mut roll = rng.random_range(0.0..total_weight);
                let mut selected: Option<&'static Enchantment> = None;
                for e in &available {
                    roll -= e.weight as f32;
                    if roll <= 0.0 {
                        selected = Some(e);
                        break;
                    }
                }
                let Some(&fallback) = available.last() else {
                    break;
                };
                let selected = selected.unwrap_or(fallback);

                let mut level = 1;
                for lvl in (1..=selected.max_level).rev() {
                    if cost >= selected.min_cost.calculate(lvl) {
                        level = lvl;
                        break;
                    }
                }

                applied.push((selected, level.clamp(1, selected.max_level)));

                cost /= 2;
                if cost < 1 {
                    break;
                }
            }

            applied
        }
    }
}

pub(super) fn tests_tokens() -> TokenStream {
    quote! {
        #[cfg(test)]
        mod tests {
            use super::*;

            #[test]
            fn all_providers_count() {
                assert!(EnchantmentProvider::ALL.len() >= 7);
            }

            #[test]
            fn lookup_by_name() {
                assert_eq!(
                    EnchantmentProvider::from_name("minecraft:mob_spawn_equipment"),
                    Some(&EnchantmentProvider::MOB_SPAWN_EQUIPMENT)
                );
                assert_eq!(
                    EnchantmentProvider::from_name("mob_spawn_equipment"),
                    Some(&EnchantmentProvider::MOB_SPAWN_EQUIPMENT)
                );
                assert_eq!(
                    EnchantmentProvider::from_name("enderman_loot_drop"),
                    Some(&EnchantmentProvider::ENDERMAN_LOOT_DROP)
                );
                assert_eq!(
                    EnchantmentProvider::from_name("raid/vindicator"),
                    Some(&EnchantmentProvider::RAID_VINDICATOR)
                );
                assert_eq!(
                    EnchantmentProvider::from_name("minecraft:raid/pillager_post_wave_5"),
                    Some(&EnchantmentProvider::RAID_PILLAGER_POST_WAVE_5)
                );
            }

            #[test]
            fn raw_json_retrieval() {
                assert!(EnchantmentProvider::get_raw_json("mob_spawn_equipment").is_some());
                assert!(EnchantmentProvider::get_raw_json("minecraft:enderman_loot_drop").is_some());
                assert!(EnchantmentProvider::get_raw_json("nonexistent").is_none());
            }

            #[test]
            fn single_enchantment_provider() {
                let encs = EnchantmentProvider::ENDERMAN_LOOT_DROP.select_enchantments(&Item::DIAMOND_SWORD, 0.0);
                assert_eq!(encs.len(), 1);
                assert_eq!(encs[0].0.registry_key, "silk_touch");
                assert_eq!(encs[0].1, 1);
            }

            #[test]
            fn mob_spawn_equipment_application() {
                let mut stack = ItemStack::new(1, &Item::DIAMOND_SWORD);
                EnchantmentProvider::MOB_SPAWN_EQUIPMENT.apply_to_stack(&mut stack, 1.0);
                assert!(stack.has_enchantments());
            }
        }
    }
}
