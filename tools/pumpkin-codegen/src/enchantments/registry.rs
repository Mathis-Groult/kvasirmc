//! Reads the vanilla enchantments and turns each into its constant and lookup arms.

use super::{
    AttributeModifierSlot, Cost, Enchantment, parse_conditional_entity_effects,
    parse_conditional_value_effects, parse_optional_value_effect,
    parse_targeted_conditional_entity_effects, parse_targeted_conditional_value_effects,
};
use heck::{ToPascalCase, ToShoutySnakeCase};
use proc_macro2::TokenStream;
use pumpkin_util::text::TextComponent;
use pumpkin_util::text::TextContent::Translate;
use quote::{format_ident, quote};
use serde::Deserialize;
use std::{collections::BTreeMap, fs};

/// Reads every vanilla enchantment JSON, numbering them in file-name order.
pub(super) fn load_enchantments() -> BTreeMap<String, Enchantment> {
    let dir = std::path::Path::new("../../assets/datapack/data/minecraft/enchantment");
    let mut enchantments: BTreeMap<String, Enchantment> = BTreeMap::new();
    let mut entries: Vec<_> = fs::read_dir(dir)
        .expect("Missing enchantment directory")
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();
    entries.sort_by_key(|e| e.path());

    for (i, entry) in entries.iter().enumerate() {
        let stem = entry
            .path()
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let key = format!("minecraft:{stem}");
        let content = fs::read_to_string(entry.path()).expect("Failed to read enchantment file");
        let mut enc: Enchantment =
            serde_json::from_str(&content).expect("Failed to parse enchantment JSON");
        enc.id = i as u8;
        enchantments.insert(key, enc);
    }
    enchantments
}

/// Adds one enchantment's constant, list entry and lookup arms.
pub(super) fn push_enchantment(
    name: String,
    enchantment: Enchantment,
    variants: &mut TokenStream,
    all_variants: &mut TokenStream,
    name_to_type: &mut TokenStream,
    id_to_type: &mut TokenStream,
) {
    let id = enchantment.id;
    let raw_name = name.strip_prefix("minecraft:").unwrap();
    let format_name = format_ident!("{}", raw_name.to_shouty_snake_case());
    all_variants.extend(quote! { &Self::#format_name, });
    let anvil_cost = enchantment.anvil_cost;
    let supported_items = format_ident!(
        "{}",
        enchantment
            .supported_items
            .strip_prefix("#")
            .unwrap()
            .replace([':', '/'], "_")
            .to_uppercase()
    );
    let max_level = enchantment.max_level;
    let weight = enchantment.weight;
    let min_cost_base = enchantment.min_cost.base;
    let min_cost_per_level = enchantment.min_cost.per_level_above_first;
    let max_cost_base = enchantment.max_cost.base;
    let max_cost_per_level = enchantment.max_cost.per_level_above_first;

    let slots = enchantment.slots;
    let slots = slots.iter().map(AttributeModifierSlot::to_tokens);
    let translate = description_key(&enchantment.description);

    let effects_tokens = effects_tokens(enchantment.effects);

    let primary_items = primary_items_tokens(enchantment.primary_items.as_deref());

    if let Some(exclusive_set) = &enchantment.exclusive_set {
        let exclusive_set = format_ident!(
            "{}",
            exclusive_set
                .strip_prefix("#")
                .unwrap()
                .replace([':', '/'], "_")
                .to_uppercase()
        );
        variants.extend([quote! {
            pub const #format_name: Self = Self {
                id: #id,
                name: #name,
                registry_key: #raw_name,
                description: #translate,
                anvil_cost: #anvil_cost,
                supported_items: &ItemTag::#supported_items,
                primary_items: #primary_items,
                exclusive_set: Some(&EnchantmentTag::#exclusive_set),
                max_level: #max_level,
                slots: &[#(#slots),*],
                weight: #weight,
                min_cost: Cost {
                    base: #min_cost_base,
                    per_level_above_first: #min_cost_per_level,
                },
                max_cost: Cost {
                    base: #max_cost_base,
                    per_level_above_first: #max_cost_per_level,
                },
                effects: #effects_tokens,
            };
        }]);
    } else {
        variants.extend([quote! {
            pub const #format_name: Self = Self {
                id: #id,
                name: #name,
                description: #translate,
                registry_key: #raw_name,
                anvil_cost: #anvil_cost,
                supported_items: &ItemTag::#supported_items,
                primary_items: #primary_items,
                exclusive_set: None,
                max_level: #max_level,
                slots: &[#(#slots),*],
                weight: #weight,
                min_cost: Cost {
                    base: #min_cost_base,
                    per_level_above_first: #min_cost_per_level,
                },
                max_cost: Cost {
                    base: #max_cost_base,
                    per_level_above_first: #max_cost_per_level,
                },
                effects: #effects_tokens,
            };
        }]);
    }

    name_to_type.extend(quote! { #name | #raw_name => Some(&Self::#format_name), });
    id_to_type.extend(quote! { #id => Some(&Self::#format_name), });
}

/// The translation key of an enchantment's description.
fn description_key(description: &TextComponent) -> String {
    let Translate {
        translate,
        bedrock_translate: _,
        with: _,
    } = &*description.0.content
    else {
        panic!()
    };
    translate.to_string()
}

/// The `EnchantmentEffects` value of one enchantment.
fn effects_tokens(effects_map: serde_json::Map<String, serde_json::Value>) -> TokenStream {
    let projectile_spawned =
        parse_conditional_entity_effects(effects_map.get("minecraft:projectile_spawned"));
    let post_attack =
        parse_targeted_conditional_entity_effects(effects_map.get("minecraft:post_attack"));
    let projectile_count =
        parse_conditional_value_effects(effects_map.get("minecraft:projectile_count"));
    let projectile_spread =
        parse_conditional_value_effects(effects_map.get("minecraft:projectile_spread"));
    let projectile_piercing =
        parse_conditional_value_effects(effects_map.get("minecraft:projectile_piercing"));
    let ammo_use = parse_conditional_value_effects(effects_map.get("minecraft:ammo_use"));
    let damage = parse_conditional_value_effects(effects_map.get("minecraft:damage"));
    let knockback = parse_conditional_value_effects(effects_map.get("minecraft:knockback"));
    let armor_effectiveness =
        parse_conditional_value_effects(effects_map.get("minecraft:armor_effectiveness"));
    let damage_protection =
        parse_conditional_value_effects(effects_map.get("minecraft:damage_protection"));
    let hit_block = parse_conditional_entity_effects(effects_map.get("minecraft:hit_block"));
    let item_damage = parse_conditional_value_effects(effects_map.get("minecraft:item_damage"));
    let equipment_drops =
        parse_targeted_conditional_value_effects(effects_map.get("minecraft:equipment_drops"));
    let fishing_time_reduction =
        parse_conditional_value_effects(effects_map.get("minecraft:fishing_time_reduction"));
    let fishing_luck_bonus =
        parse_conditional_value_effects(effects_map.get("minecraft:fishing_luck_bonus"));
    let block_experience =
        parse_conditional_value_effects(effects_map.get("minecraft:block_experience"));
    let mob_experience =
        parse_conditional_value_effects(effects_map.get("minecraft:mob_experience"));
    let repair_with_xp =
        parse_conditional_value_effects(effects_map.get("minecraft:repair_with_xp"));
    let smash_damage_per_fallen_block =
        parse_conditional_value_effects(effects_map.get("minecraft:smash_damage_per_fallen_block"));
    let trident_return_acceleration =
        parse_conditional_value_effects(effects_map.get("minecraft:trident_return_acceleration"));
    let trident_spin_attack_strength =
        parse_optional_value_effect(effects_map.get("minecraft:trident_spin_attack_strength"));
    let crossbow_charge_time =
        parse_optional_value_effect(effects_map.get("minecraft:crossbow_charge_time"));
    let location_changed =
        parse_conditional_entity_effects(effects_map.get("minecraft:location_changed"));
    let prevent_armor_change = effects_map.contains_key("minecraft:prevent_armor_change");
    let prevent_equipment_drop = effects_map.contains_key("minecraft:prevent_equipment_drop");

    quote! {
        EnchantmentEffects {
            projectile_spawned: #projectile_spawned,
            post_attack: #post_attack,
            projectile_count: #projectile_count,
            projectile_spread: #projectile_spread,
            projectile_piercing: #projectile_piercing,
            ammo_use: #ammo_use,
            damage: #damage,
            knockback: #knockback,
            armor_effectiveness: #armor_effectiveness,
            damage_protection: #damage_protection,
            hit_block: #hit_block,
            item_damage: #item_damage,
            equipment_drops: #equipment_drops,
            fishing_time_reduction: #fishing_time_reduction,
            fishing_luck_bonus: #fishing_luck_bonus,
            block_experience: #block_experience,
            mob_experience: #mob_experience,
            repair_with_xp: #repair_with_xp,
            smash_damage_per_fallen_block: #smash_damage_per_fallen_block,
            trident_return_acceleration: #trident_return_acceleration,
            trident_spin_attack_strength: #trident_spin_attack_strength,
            crossbow_charge_time: #crossbow_charge_time,
            location_changed: #location_changed,
            prevent_armor_change: #prevent_armor_change,
            prevent_equipment_drop: #prevent_equipment_drop,
        }
    }
}

/// The `primary_items` tag reference of one enchantment.
fn primary_items_tokens(primary_items: Option<&str>) -> TokenStream {
    if let Some(primary) = primary_items {
        let ident = format_ident!(
            "{}",
            primary
                .strip_prefix("#")
                .unwrap()
                .replace([':', '/'], "_")
                .to_uppercase()
        );
        quote! { Some(&ItemTag::#ident) }
    } else {
        quote! { None }
    }
}
