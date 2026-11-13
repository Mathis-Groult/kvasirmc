//! Eating, drinking, cooldown and fuel components of an item.

use super::{
    Consumable, ConsumeEffect, ItemComponents, StatusEffectInstance, StringOrStatusEffects,
    float_literal,
};
use crate::block::{Block, BlockAssets};
use crate::enchantments::AttributeModifierSlot;
use heck::{ToPascalCase, ToShoutySnakeCase};
use proc_macro2::{Span, TokenStream};
use pumpkin_nbt::{Nbt, compound::NbtCompound};
use pumpkin_util::registry::TagType;
use pumpkin_util::text::TextContent;
use pumpkin_util::{registry::RegistryEntryList, text::TextComponent};
use quote::{ToTokens, format_ident, quote};
use serde::Deserialize;
use serde_repr::Deserialize_repr;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::Cursor,
};
use syn::{Ident, LitBool, LitByteStr, LitFloat, LitInt, LitStr};

impl ItemComponents {
    /// Emits the `use_cooldown` component, if any.
    pub(super) fn push_use_cooldown(&self, tokens: &mut TokenStream) {
        if let Some(use_cooldown) = &self.use_cooldown {
            let seconds = LitFloat::new(&format!("{:.1}", use_cooldown.seconds), Span::call_site());
            let cooldown_group = if let Some(cd_group) = &use_cooldown.cooldown_group {
                quote! { Some(#cd_group) }
            } else {
                quote! { None }
            };

            tokens.extend(quote! {
                (UseCooldown, &UseCooldownImpl {
                    seconds: #seconds,
                    cooldown_group: #cooldown_group,
                }),
            });
        }
    }

    /// Emits the `food` component, if any.
    pub(super) fn push_food(&self, tokens: &mut TokenStream) {
        if let Some(food) = &self.food {
            let nutrition = LitInt::new(&food.nutrition.to_string(), Span::call_site());
            let saturation = LitFloat::new(&format!("{:.1}", food.saturation), Span::call_site());
            let can_always_eat = {
                let can = LitBool::new(food.can_always_eat, Span::call_site());
                quote! { #can }
            };
            tokens.extend(quote! { (Food, &FoodImpl {
                nutrition: #nutrition,
                saturation: #saturation,
                can_always_eat: #can_always_eat,
            }), });
        }
    }

    /// Emits the `consumable` component, if any.
    pub(super) fn push_consumable(&self, tokens: &mut TokenStream) {
        if let Some(consumable) = &self.consumable {
            let consume_seconds = LitFloat::new(
                &format!("{:.1}", consumable.consume_seconds.unwrap_or(1.6)),
                Span::call_site(),
            );
            let consume_particles = LitBool::new(
                consumable.has_consume_particles.unwrap_or(true),
                Span::call_site(),
            );

            let anim_str = consumable.animation.clone().unwrap_or("eat".to_string());
            let animation = format_ident!("{}", anim_str.to_pascal_case());

            let sound_id = consumable
                .sound
                .clone()
                .unwrap_or("minecraft:entity.generic.eat".to_string());
            let variant_name = format_ident!(
                "{}",
                sound_id
                    .strip_prefix("minecraft:")
                    .unwrap()
                    .to_pascal_case()
            );
            let effects: Vec<ConsumeEffect> =
                consumable.on_consume_effects.clone().unwrap_or(vec![]);
            let effect_tokens = consume_effect_tokens(effects);

            tokens.extend(quote! { (Consumable, &ConsumableImpl {
                consume_seconds: #consume_seconds,
                animation: ConsumeAnimation::#animation,
                sound_event: IdOr::Id(Sound::#variant_name),
                consume_particles: #consume_particles,
                effects: Cow::Borrowed(&[#effect_tokens])
            }), });
        }
    }

    /// Emits the `compostable` component, if any.
    pub(super) fn push_compostable(&self, tokens: &mut TokenStream) {
        if let Some(compostable) = &self.compostable {
            let layers = compostable
                .layers
                .strip_prefix("minecraft:")
                .unwrap_or(&compostable.layers);
            let chance = match layers {
                "compostable/low" => 0.3,
                "compostable/low_medium" => 0.5,
                "compostable/medium" => 0.65,
                "compostable/medium_high" => 0.85,
                "compostable/always_add_one" => 1.0,
                _ => panic!("Unknown compostable layers: {}", compostable.layers),
            };
            let chance_lit = LitFloat::new(&format!("{chance:?}f32"), Span::call_site());
            tokens.extend(quote! {
                (Compostable, &CompostableImpl { chance: #chance_lit }),
            });
        }
    }

    /// Emits the `cooking_fuel` component, if any.
    pub(super) fn push_cooking_fuel(&self, tokens: &mut TokenStream) {
        if let Some(fuel) = &self.cooking_fuel {
            let burn_time_tokens = match &fuel.burn_time {
                serde_json::Value::Number(n) => {
                    let val = n.as_i64().unwrap_or(0) as i32;
                    let val_lit = LitInt::new(&val.to_string(), Span::call_site());
                    quote! { crate::data_component_impl::IntProvider::Inline(#val_lit) }
                }
                serde_json::Value::String(s) => {
                    let s_lit = LitStr::new(s, Span::call_site());
                    quote! { crate::data_component_impl::IntProvider::Id(std::borrow::Cow::Borrowed(#s_lit)) }
                }
                _ => quote! { crate::data_component_impl::IntProvider::Inline(0) },
            };

            let speed_multiplier_tokens = match &fuel.speed_multiplier {
                Some(serde_json::Value::Number(n)) => {
                    let val = n.as_f64().unwrap_or(1.0) as f32;
                    let val_lit = LitFloat::new(&format!("{val:?}f32"), Span::call_site());
                    quote! { crate::data_component_impl::FloatProvider::Inline(#val_lit) }
                }
                Some(serde_json::Value::String(s)) => {
                    let s_lit = LitStr::new(s, Span::call_site());
                    quote! { crate::data_component_impl::FloatProvider::Id(std::borrow::Cow::Borrowed(#s_lit)) }
                }
                _ => {
                    quote! { crate::data_component_impl::FloatProvider::Id(std::borrow::Cow::Borrowed("minecraft:cooking/speed_default")) }
                }
            };

            tokens.extend(quote! {
                (CookingFuel, &CookingFuelImpl {
                    burn_time: #burn_time_tokens,
                    speed_multiplier: #speed_multiplier_tokens,
                }),
            });
        }
    }

    /// Emits the `ominous_bottle_amplifier` component, if any.
    pub(super) fn push_ominous_bottle_amplifier(&self, tokens: &mut TokenStream) {
        if let Some(amp) = self.ominous_bottle_amplifier {
            let amp_lit = LitInt::new(&amp.to_string(), Span::call_site());
            tokens.extend(quote! { (OminousBottleAmplifier, &OminousBottleAmplifierImpl { amplifier: #amp_lit }), });
        }
    }

    /// Emits the `potion_contents` component, if any.
    pub(super) fn push_potion_contents(&self, tokens: &mut TokenStream) {
        if self.potion_contents.is_some() {
            tokens.extend(quote! {
                (PotionContents, &PotionContentsImpl {
                    potion_id: None,
                    custom_color: None,
                    custom_effects: Vec::new(),
                    custom_name: None,
                }),
            });
        }
    }

    /// Emits the `potion_duration_scale` component, if any.
    pub(super) fn push_potion_duration_scale(&self, tokens: &mut TokenStream) {
        if let Some(scale) = self.potion_duration_scale {
            let scale_lit = LitFloat::new(&format!("{scale:?}f32"), Span::call_site());
            tokens.extend(
                quote! { (PotionDurationScale, &PotionDurationScaleImpl { scale: #scale_lit }), },
            );
        }
    }

    /// Emits the `use_effects` component, if any.
    pub(super) fn push_use_effects(&self, tokens: &mut TokenStream) {
        if let Some(use_effects) = &self.use_effects {
            let can_sprint = LitBool::new(use_effects.can_sprint, Span::call_site());
            let interact_vibrations =
                LitBool::new(use_effects.interact_vibrations, Span::call_site());
            let speed_multiplier = float_literal(use_effects.speed_multiplier);
            tokens.extend(quote! {
                (UseEffects, &UseEffectsImpl {
                    can_sprint: #can_sprint,
                    interact_vibrations: #interact_vibrations,
                    speed_multiplier: #speed_multiplier,
                }),
            });
        }
    }

    /// Emits the `use_remainder` component, if any.
    pub(super) fn push_use_remainder(&self, tokens: &mut TokenStream) {
        if self.use_remainder.is_some() {
            let remainder = self.use_remainder.as_ref().unwrap();
            let id = LitStr::new(&remainder.id, Span::call_site());
            tokens.extend(quote! {
                (UseRemainder, &UseRemainderImpl {
                    remainder: Some(Cow::Borrowed(#id)),
                }),
            });
        }
    }

    /// Emits the `villager_food` component, if any.
    pub(super) fn push_villager_food(&self, tokens: &mut TokenStream) {
        if let Some(villager_food) = &self.villager_food {
            let nutrition = LitInt::new(&villager_food.nutrition.to_string(), Span::call_site());
            tokens.extend(quote! {
                (VillagerFood, &VillagerFoodImpl {
                    nutrition: #nutrition,
                }),
            });
        }
    }
}

/// The `ConsumeEffect` values of one consumable.
fn consume_effect_tokens(effects: Vec<ConsumeEffect>) -> TokenStream {
    let mut effect_tokens = TokenStream::new();

    for effect in effects {
        match effect.r#type.as_str() {
            "minecraft:clear_all_effects" => {
                effect_tokens.extend(quote! { ConsumeEffect::ClearAllEffects, });
            }
            "minecraft:teleport_randomly" => {
                let diameter = effect.diameter.unwrap_or(16.);
                effect_tokens.extend(quote! { ConsumeEffect::TeleportRandomly(#diameter), });
            }
            "minecraft:play_sound" => {
                let sound = format_ident!(
                    "{}",
                    effect
                        .sound
                        .unwrap()
                        .strip_prefix("minecraft:")
                        .unwrap()
                        .to_pascal_case()
                );
                effect_tokens.extend(quote! { ConsumeEffect::PlaySound(IdOr::Id(Sound::#sound)), });
            }
            "minecraft:apply_effects" => {
                let probability = effect.probability.unwrap_or(1.);
                if let StringOrStatusEffects::Effects(status_effect_instances) =
                    effect.effects.unwrap()
                {
                    let mut status_tokens = TokenStream::new();

                    for status in status_effect_instances {
                        let effect_id = status.id;
                        let amplifier = status.amplifier.unwrap_or(0);
                        let duration = status.duration.unwrap_or(1);
                        let ambient = status.ambient.unwrap_or(false);
                        let show_particles = status.show_particles.unwrap_or(true);
                        let show_icon = status.show_icon.unwrap_or(true);
                        status_tokens.extend(quote! {
                            StatusEffectInstance {
                                effect_id: Cow::Borrowed(#effect_id),
                                amplifier: #amplifier,
                                duration: #duration,
                                ambient: #ambient,
                                show_particles: #show_particles,
                                show_icon: #show_icon
                            },
                        });
                    }
                    effect_tokens.extend(quote! {
                    ConsumeEffect::ApplyEffects((Cow::Borrowed(&[#status_tokens]), #probability)),
                });
                }
            }
            "minecraft:remove_effects" => {
                if let StringOrStatusEffects::String(id) = effect.effects.unwrap() {
                    let effect_id = format_ident!(
                        "{}",
                        id.strip_prefix("minecraft:")
                            .unwrap()
                            .to_pascal_case()
                            .to_uppercase()
                    );

                    effect_tokens.extend(quote! {
                    ConsumeEffect::RemoveEffects(IDSet::IDs(Cow::Borrowed(&[&StatusEffect::#effect_id]))),
                });
                }
            }
            _ => println!("Unknown CustomEffect type: {}", effect.r#type),
        }
    }
    effect_tokens
}
