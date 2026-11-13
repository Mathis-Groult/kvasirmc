//! Damage, tool, weapon and attack components of an item.

use super::{
    Item, ItemComponents, Modifier, Operation, ToolRule, float_literal, kinetic_condition_tokens,
    optional_sound_tokens,
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
    /// Emits the `damage` component, if any.
    pub(super) fn push_damage(&self, tokens: &mut TokenStream) {
        if let Some(d) = self.damage {
            let damage_lit = LitInt::new(&d.to_string(), Span::call_site());
            tokens.extend(quote! {
                (Damage, &DamageImpl {
                    damage: #damage_lit,
                }),
            });
        }
    }

    /// Emits the `max_damage` component, if any.
    pub(super) fn push_max_damage(&self, tokens: &mut TokenStream) {
        if let Some(md) = self.max_damage {
            let max_damage_lit = LitInt::new(&md.to_string(), Span::call_site());
            tokens.extend(quote! {
                (MaxDamage, &MaxDamageImpl {
                    max_damage: #max_damage_lit,
                }),
            });
        }
    }

    /// Emits the `attribute_modifiers` component, if any.
    pub(super) fn push_attribute_modifiers(&self, tokens: &mut TokenStream) {
        if let Some(modifiers) = &self.attribute_modifiers {
            let modifier_code = modifiers.iter().map(|modifier| {
                let r#type = format_ident!(
                    "{}",
                    modifier
                        .r#type
                        .strip_prefix("minecraft:")
                        .unwrap()
                        .to_uppercase()
                );
                let id = LitStr::new(&modifier.id, Span::call_site());
                let amount = modifier.amount;
                let operation = Ident::new(&format!("{:?}", modifier.operation), Span::call_site());
                let slot = modifier.slot.to_tokens();

                quote! {
                    Modifier {
                        r#type: &Attributes::#r#type,
                        id: #id,
                        amount: #amount,
                        operation: Operation::#operation,
                        slot: #slot,
                    }
                }
            });
            tokens.extend(quote! {
                (AttributeModifiers, &AttributeModifiersImpl {
                    attribute_modifiers: Cow::Borrowed(&[#(#modifier_code),*])
                }),
            });
        }
    }

    /// Emits the `tool` component, if any.
    pub(super) fn push_tool(&self, tokens: &mut TokenStream) {
        if let Some(tool) = &self.tool {
            let rules_code = tool.rules.iter().map(|rule| {
                let block_array;

                if let RegistryEntryList::Single(t) = &rule.blocks {
                    if let TagType::Item(str) = t {
                        let ident = format_ident!(
                            "{}",
                            str.strip_prefix("minecraft:").unwrap().to_uppercase()
                        );
                        block_array = quote! {
                            IDs(Cow::Borrowed(&[&Block::#ident]))
                        }
                    } else if let TagType::Tag(str) = t {
                        block_array = quote! {
                            Tag(Cow::Borrowed(#str))
                        }
                    } else {
                        unreachable!();
                    }
                } else if let RegistryEntryList::Many(t) = &rule.blocks {
                    let mut array = vec![];
                    for i in t {
                        let TagType::Item(str) = i else {
                            unreachable!();
                        };
                        let ident = format_ident!(
                            "{}",
                            str.strip_prefix("minecraft:").unwrap().to_uppercase()
                        );
                        array.push(quote! {
                            &Block::#ident
                        });
                    }
                    block_array = quote! {
                        IDs(Cow::Borrowed(&[#(#array),*]))
                    }
                } else {
                    unreachable!();
                }
                let speed = if let Some(speed) = rule.speed {
                    quote! { Some(#speed) }
                } else {
                    quote! { None }
                };
                let correct_for_drops = if let Some(correct_for_drops) = rule.correct_for_drops {
                    let correct_for_drops = LitBool::new(correct_for_drops, Span::call_site());
                    quote! { Some(#correct_for_drops) }
                } else {
                    quote! { None }
                };
                quote! {
                    ToolRule {
                        blocks: #block_array,
                        speed: #speed,
                        correct_for_drops: #correct_for_drops
                    }
                }
            });
            let damage_per_block = {
                let speed = LitInt::new(&tool.damage_per_block.to_string(), Span::call_site());
                quote! { #speed }
            };
            let default_mining_speed = {
                let speed = LitFloat::new(
                    &format!("{:.1}", tool.default_mining_speed),
                    Span::call_site(),
                );
                quote! { #speed }
            };
            let can_destroy_blocks_in_creative =
                LitBool::new(tool.can_destroy_blocks_in_creative, Span::call_site());
            tokens.extend(quote! { (Tool, &ToolImpl {
                rules: Cow::Borrowed(&[#(#rules_code),*]),
                default_mining_speed: #default_mining_speed,
                damage_per_block: #damage_per_block,
                can_destroy_blocks_in_creative: #can_destroy_blocks_in_creative
            }), });
        }
    }

    /// Emits the `weapon` component, if any.
    pub(super) fn push_weapon(&self, tokens: &mut TokenStream) {
        if let Some(weapon) = &self.weapon {
            let damage = LitInt::new(
                &weapon.item_damage_per_attack.to_string(),
                Span::call_site(),
            );
            let disable_blocking_for_seconds = float_literal(weapon.disable_blocking_for_seconds);
            tokens.extend(quote! {
                (Weapon, &WeaponImpl {
                    item_damage_per_attack: #damage,
                    disable_blocking_for_seconds: #disable_blocking_for_seconds,
                }),
            });
        }
    }

    /// Emits the `damage_resistant` component, if any.
    pub(super) fn push_damage_resistant(&self, tokens: &mut TokenStream) {
        if let Some(damage_resistant) = &self.damage_resistant {
            let res_type_variant = damage_resistant_variant(damage_resistant.types.as_str());
            let res_type_ident = format_ident!("{}", res_type_variant);
            tokens.extend(quote! { (DamageResistant, &DamageResistantImpl {
                res_type: DamageResistantType::#res_type_ident,
            }), });
        }
    }

    /// Emits the `attack_range` component, if any.
    pub(super) fn push_attack_range(&self, tokens: &mut TokenStream) {
        if let Some(attack_range) = &self.attack_range {
            let min_reach = float_literal(attack_range.min_reach);
            let max_reach = float_literal(attack_range.max_reach);
            let min_creative_reach = float_literal(attack_range.min_creative_reach);
            let max_creative_reach = float_literal(attack_range.max_creative_reach);
            let hitbox_margin = float_literal(attack_range.hitbox_margin);
            let mob_factor = float_literal(attack_range.mob_factor);
            tokens.extend(quote! {
                (AttackRange, &AttackRangeImpl {
                    min_reach: #min_reach,
                    max_reach: #max_reach,
                    min_creative_reach: #min_creative_reach,
                    max_creative_reach: #max_creative_reach,
                    hitbox_margin: #hitbox_margin,
                    mob_factor: #mob_factor,
                }),
            });
        }
    }

    /// Emits the `damage_type` component, if any.
    pub(super) fn push_damage_type(&self, tokens: &mut TokenStream) {
        if let Some(damage_type) = &self.damage_type {
            let damage_type = format_ident!(
                "{}",
                damage_type
                    .strip_prefix("minecraft:")
                    .unwrap_or(damage_type)
                    .to_shouty_snake_case()
            );
            tokens.extend(quote! {
                (DamageType, &DamageTypeImpl {
                    damage_type: crate::damage::DamageType::#damage_type,
                }),
            });
        }
    }

    /// Emits the `interact_animation` component, if any.
    pub(super) fn push_interact_animation(&self, tokens: &mut TokenStream) {
        if let Some(interact) = &self.interact_animation {
            let anim_type = match interact.r#type.as_str() {
                "whack" => quote! { SwingAnimationType::Whack },
                "stab" => quote! { SwingAnimationType::Stab },
                "none" => quote! { SwingAnimationType::None },
                _ => quote! { SwingAnimationType::Whack },
            };
            let duration = interact.duration;
            tokens.extend(quote! {
                (
                    InteractAnimation,
                    &InteractAnimationImpl {
                        animation_type: #anim_type,
                        duration: #duration,
                    },
                ),
            });
        }
    }

    /// Emits the `kinetic_weapon` component, if any.
    pub(super) fn push_kinetic_weapon(&self, tokens: &mut TokenStream) {
        if let Some(kinetic_weapon) = &self.kinetic_weapon {
            let contact_cooldown_ticks = LitInt::new(
                &kinetic_weapon.contact_cooldown_ticks.to_string(),
                Span::call_site(),
            );
            let delay_ticks =
                LitInt::new(&kinetic_weapon.delay_ticks.to_string(), Span::call_site());
            let dismount_conditions =
                kinetic_condition_tokens(kinetic_weapon.dismount_conditions.as_ref());
            let knockback_conditions =
                kinetic_condition_tokens(kinetic_weapon.knockback_conditions.as_ref());
            let damage_conditions =
                kinetic_condition_tokens(kinetic_weapon.damage_conditions.as_ref());
            let forward_movement = float_literal(kinetic_weapon.forward_movement);
            let damage_multiplier = float_literal(kinetic_weapon.damage_multiplier);
            let sound = optional_sound_tokens(kinetic_weapon.sound.as_deref());
            let hit_sound = optional_sound_tokens(kinetic_weapon.hit_sound.as_deref());
            tokens.extend(quote! {
                (KineticWeapon, &KineticWeaponImpl {
                    contact_cooldown_ticks: #contact_cooldown_ticks,
                    delay_ticks: #delay_ticks,
                    dismount_conditions: #dismount_conditions,
                    knockback_conditions: #knockback_conditions,
                    damage_conditions: #damage_conditions,
                    forward_movement: #forward_movement,
                    damage_multiplier: #damage_multiplier,
                    sound: #sound,
                    hit_sound: #hit_sound,
                }),
            });
        }
    }

    /// Emits the `minimum_attack_charge` component, if any.
    pub(super) fn push_minimum_attack_charge(&self, tokens: &mut TokenStream) {
        if let Some(charge) = self.minimum_attack_charge {
            let charge = float_literal(charge);
            tokens.extend(
                quote! { (MinimumAttackCharge, &MinimumAttackChargeImpl { charge: #charge }), },
            );
        }
    }

    /// Emits the `piercing_weapon` component, if any.
    pub(super) fn push_piercing_weapon(&self, tokens: &mut TokenStream) {
        if let Some(piercing_weapon) = &self.piercing_weapon {
            let deals_knockback = LitBool::new(piercing_weapon.deals_knockback, Span::call_site());
            let dismounts = LitBool::new(piercing_weapon.dismounts, Span::call_site());
            let sound = optional_sound_tokens(piercing_weapon.sound.as_deref());
            let hit_sound = optional_sound_tokens(piercing_weapon.hit_sound.as_deref());
            tokens.extend(quote! {
                (PiercingWeapon, &PiercingWeaponImpl {
                    deals_knockback: #deals_knockback,
                    dismounts: #dismounts,
                    sound: #sound,
                    hit_sound: #hit_sound,
                }),
            });
        }
    }

    /// Emits the `swing_animation` component, if any.
    pub(super) fn push_swing_animation(&self, tokens: &mut TokenStream) {
        if let Some(swing) = &self.swing_animation {
            let anim_type = match swing.r#type.as_str() {
                "whack" => quote! { SwingAnimationType::Whack },
                "stab" => quote! { SwingAnimationType::Stab },
                "none" => quote! { SwingAnimationType::None },
                _ => quote! { SwingAnimationType::Whack },
            };
            let duration = swing.duration;
            tokens.extend(quote! {
                (
                    AttackAnimation,
                    &SwingAnimationImpl {
                        animation_type: #anim_type,
                        duration: #duration,
                    },
                ),
            });
        }
    }
}

/// The `DamageResistantType` variant named by a damage type tag (canonical or shorthand).
fn damage_resistant_variant(types: &str) -> &'static str {
    match types {
        // Common canonical and shorthand forms mapped to enum variant names
        "#minecraft:always_hurts_ender_dragons"
        | "minecraft:always_hurts_ender_dragons"
        | "always_hurts_ender_dragons" => "AlwaysHurtsEnderDragons",
        "#minecraft:always_kills_armor_stands"
        | "minecraft:always_kills_armor_stands"
        | "always_kills_armor_stands" => "AlwaysKillsArmorStands",
        "#minecraft:always_most_significant_fall"
        | "minecraft:always_most_significant_fall"
        | "always_most_significant_fall" => "AlwaysMostSignificantFall",
        "#minecraft:always_triggers_silverfish"
        | "minecraft:always_triggers_silverfish"
        | "always_triggers_silverfish" => "AlwaysTriggersSilverfish",
        "#minecraft:avoids_guardian_thorns"
        | "minecraft:avoids_guardian_thorns"
        | "avoids_guardian_thorns" => "AvoidsGuardianThorns",
        "#minecraft:burns_armor_stands" | "minecraft:burns_armor_stands" | "burns_armor_stands" => {
            "BurnsArmorStands"
        }
        "#minecraft:burn_from_stepping" | "minecraft:burn_from_stepping" | "burn_from_stepping" => {
            "BurnFromStepping"
        }
        "#minecraft:bypasses_armor" | "minecraft:bypasses_armor" | "bypasses_armor" => {
            "BypassesArmor"
        }
        "#minecraft:bypasses_cooldown" | "minecraft:bypasses_cooldown" | "bypasses_cooldown" => {
            "BypassesCooldown"
        }
        "#minecraft:bypasses_effects" | "minecraft:bypasses_effects" | "bypasses_effects" => {
            "BypassesEffects"
        }
        "#minecraft:bypasses_enchantments"
        | "minecraft:bypasses_enchantments"
        | "bypasses_enchantments" => "BypassesEnchantments",
        "#minecraft:bypasses_invulnerability"
        | "minecraft:bypasses_invulnerability"
        | "bypasses_invulnerability" => "BypassesInvulnerability",
        "#minecraft:bypasses_resistance"
        | "minecraft:bypasses_resistance"
        | "bypasses_resistance" => "BypassesResistance",
        "#minecraft:bypasses_shield" | "minecraft:bypasses_shield" | "bypasses_shield" => {
            "BypassesShield"
        }
        "#minecraft:bypasses_wolf_armor"
        | "minecraft:bypasses_wolf_armor"
        | "bypasses_wolf_armor" => "BypassesWolfArmor",
        "#minecraft:can_break_armor_stand"
        | "minecraft:can_break_armor_stand"
        | "can_break_armor_stand" => "CanBreakArmorStands",
        "#minecraft:damages_helmet" | "minecraft:damages_helmet" | "damages_helmet" => {
            "DamagesHelmet"
        }
        "#minecraft:ignites_armor_stands"
        | "minecraft:ignites_armor_stands"
        | "ignites_armor_stands" => "IgnitesArmorStands",
        "#minecraft:is_drowning" | "minecraft:is_drowning" | "is_drowning" => "Drowning",
        "#minecraft:is_explosion" | "minecraft:is_explosion" | "is_explosion" | "explosion" => {
            "Explosion"
        }
        "#minecraft:is_fall" | "minecraft:is_fall" | "is_fall" | "fall" => "Fall",
        "#minecraft:is_fire" | "minecraft:is_fire" | "is_fire" | "fire" | "in_fire"
        | "minecraft:in_fire" => "Fire",
        "#minecraft:is_freezing" | "minecraft:is_freezing" | "is_freezing" => "Freezing",
        "#minecraft:is_lightning" | "minecraft:is_lightning" | "is_lightning" => "Lightning",
        "#minecraft:is_player_attack" | "minecraft:is_player_attack" | "is_player_attack" => {
            "PlayerAttack"
        }
        "#minecraft:is_projectile" | "minecraft:is_projectile" | "is_projectile" => "Projectile",
        "#minecraft:mace_smash" | "minecraft:mace_smash" | "mace_smash" => "MaceSmash",
        "#minecraft:no_anger" | "minecraft:no_anger" | "no_anger" => "NoAnger",
        "#minecraft:no_impact" | "minecraft:no_impact" | "no_impact" => "NoImpact",
        "#minecraft:no_knockback" | "minecraft:no_knockback" | "no_knockback" => "NoKnockback",
        "#minecraft:panic_causes" | "minecraft:panic_causes" | "panic_causes" => "PanicCauses",
        "#minecraft:panic_environmental_causes"
        | "minecraft:panic_environmental_causes"
        | "panic_environmental_causes" => "PanicEnvironmentalCauses",
        "#minecraft:witch_resistant_to" | "minecraft:witch_resistant_to" | "witch_resistant_to" => {
            "WitchResistantTo"
        }
        "#minecraft:wither_immune_to" | "minecraft:wither_immune_to" | "wither_immune_to" => {
            "WitherImmuneTo"
        }
        _ => "Generic",
    }
}
