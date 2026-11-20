mod entity_effects;
mod registry;
mod static_items;

use registry::{load_enchantments, push_enchantment};
use std::{collections::BTreeMap, fs};

use heck::{ToPascalCase, ToShoutySnakeCase};
use proc_macro2::TokenStream;
use pumpkin_util::text::TextComponent;
use pumpkin_util::text::TextContent::Translate;
use quote::{format_ident, quote};
use serde::Deserialize;

/// Raw deserialization shape for a single enchantment entry from `enchantment/*.json`.
#[derive(Deserialize)]
pub struct Enchantment {
    /// Numeric registry ID for this enchantment.
    #[serde(default)]
    pub id: u8,
    /// Anvil repair cost multiplier added when applying this enchantment.
    pub anvil_cost: u32,
    /// Tag path (prefixed with `#`) of items that support this enchantment.
    pub supported_items: String,
    /// Optional tag path (prefixed with `#`) of primary items for enchanting table.
    #[serde(default)]
    pub primary_items: Option<String>,
    /// Display name component for this enchantment (typically a translation key).
    pub description: TextComponent,
    /// Optional exclusive-set tag; enchantments in the same set are mutually incompatible.
    pub exclusive_set: Option<String>,
    /// Maximum level this enchantment can reach.
    pub max_level: i32,
    /// Equipment slots this enchantment's attribute modifiers apply to.
    pub slots: Vec<AttributeModifierSlot>,
    /// The weight of this enchantment (used for random selection).
    pub weight: i32,
    /// The minimum cost to get this enchantment.
    pub min_cost: Cost,
    /// The maximum cost to get this enchantment.
    pub max_cost: Cost,
    /// Data-driven effects map.
    #[serde(default)]
    pub effects: serde_json::Map<String, serde_json::Value>,
}

#[derive(Deserialize, Clone, Copy)]
pub struct Cost {
    pub base: i32,
    pub per_level_above_first: i32,
}

/// Equipment slot category that an enchantment's attribute modifier applies to.
#[derive(Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum AttributeModifierSlot {
    Any,
    MainHand,
    OffHand,
    Hand,
    Feet,
    Legs,
    Chest,
    Head,
    Armor,
    Body,
    Saddle,
}

impl AttributeModifierSlot {
    /// Converts this slot variant into a `TokenStream` for use in generated code.
    pub fn to_tokens(&self) -> TokenStream {
        match self {
            Self::Any => quote! { AttributeModifierSlot::Any },
            Self::MainHand => quote! { AttributeModifierSlot::MainHand },
            Self::OffHand => quote! { AttributeModifierSlot::OffHand },
            Self::Hand => quote! { AttributeModifierSlot::Hand },
            Self::Feet => quote! { AttributeModifierSlot::Feet },
            Self::Legs => quote! { AttributeModifierSlot::Legs },
            Self::Chest => quote! { AttributeModifierSlot::Chest },
            Self::Head => quote! { AttributeModifierSlot::Head },
            Self::Armor => quote! { AttributeModifierSlot::Armor },
            Self::Body => quote! { AttributeModifierSlot::Body },
            Self::Saddle => quote! { AttributeModifierSlot::Saddle },
        }
    }
}

fn parse_level_based_value(val: &serde_json::Value) -> TokenStream {
    if let Some(num) = val.as_f64() {
        let num_f32 = num as f32;
        return quote! { LevelBasedValue::Constant(#num_f32) };
    }

    if let Some(obj) = val.as_object() {
        let val_type = obj.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match val_type {
            "minecraft:linear" => {
                let base = obj.get("base").and_then(|b| b.as_f64()).unwrap_or(0.0) as f32;
                let per_level = obj
                    .get("per_level_above_first")
                    .and_then(|p| p.as_f64())
                    .unwrap_or(0.0) as f32;
                quote! {
                    LevelBasedValue::Linear {
                        base: #base,
                        per_level_above_first: #per_level,
                    }
                }
            }
            "minecraft:clamped" => {
                let inner =
                    parse_level_based_value(obj.get("value").unwrap_or(&serde_json::Value::Null));
                let min = obj.get("min").and_then(|m| m.as_f64()).unwrap_or(0.0) as f32;
                let max = obj.get("max").and_then(|m| m.as_f64()).unwrap_or(0.0) as f32;
                quote! {
                    LevelBasedValue::Clamped {
                        value: &#inner,
                        min: #min,
                        max: #max,
                    }
                }
            }
            "minecraft:fraction" => {
                let num = parse_level_based_value(
                    obj.get("numerator").unwrap_or(&serde_json::Value::Null),
                );
                let denom = parse_level_based_value(
                    obj.get("denominator").unwrap_or(&serde_json::Value::Null),
                );
                quote! {
                    LevelBasedValue::Fraction {
                        numerator: &#num,
                        denominator: &#denom,
                    }
                }
            }
            "minecraft:levels_squared" => {
                let added = obj.get("added").and_then(|a| a.as_f64()).unwrap_or(0.0) as f32;
                quote! {
                    LevelBasedValue::LevelsSquared { added: #added }
                }
            }
            "minecraft:lookup" => {
                let values: Vec<f32> = obj
                    .get("values")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_f64().map(|f| f as f32))
                            .collect()
                    })
                    .unwrap_or_default();
                let fallback = parse_level_based_value(
                    obj.get("fallback").unwrap_or(&serde_json::Value::Null),
                );
                quote! {
                    LevelBasedValue::Lookup {
                        values: &[#(#values),*],
                        fallback: &#fallback,
                    }
                }
            }
            _ => quote! { LevelBasedValue::Constant(0.0) },
        }
    } else {
        quote! { LevelBasedValue::Constant(0.0) }
    }
}

fn parse_particle(v: &serde_json::Value) -> TokenStream {
    let type_str = if let Some(s) = v.as_str() {
        s
    } else {
        v.get("type").and_then(|s| s.as_str()).unwrap_or("")
    };
    let raw = type_str.strip_prefix("minecraft:").unwrap_or(type_str);
    let ident = format_ident!("{}", raw.to_pascal_case());
    quote! { crate::particle::Particle::#ident }
}

fn parse_position_source(v: Option<&serde_json::Value>) -> (TokenStream, f32, f32) {
    let obj = v.and_then(|val| val.as_object());
    let type_str = obj
        .and_then(|o| o.get("type"))
        .and_then(|s| s.as_str())
        .unwrap_or("entity_position");
    let offset = obj
        .and_then(|o| o.get("offset"))
        .and_then(|n| n.as_f64())
        .unwrap_or(0.0) as f32;
    let scale = obj
        .and_then(|o| o.get("scale"))
        .and_then(|n| n.as_f64())
        .unwrap_or(1.0) as f32;
    let type_token = match type_str {
        "in_bounding_box" => quote! { PositionSourceType::InBoundingBox },
        _ => quote! { PositionSourceType::EntityPosition },
    };
    (type_token, offset, scale)
}

fn parse_float_provider(v: &serde_json::Value) -> TokenStream {
    if let Some(f) = v.as_f64() {
        let f = f as f32;
        quote! { pumpkin_util::math::float_provider::FloatProvider::Constant(#f) }
    } else if let Some(type_str) = v.get("type").and_then(|s| s.as_str()) {
        match type_str {
            "minecraft:constant" => {
                let val = v.get("value").and_then(|n| n.as_f64()).unwrap_or(0.0) as f32;
                quote! { pumpkin_util::math::float_provider::FloatProvider::Constant(#val) }
            }
            "minecraft:uniform" => {
                let min = v
                    .get("min_inclusive")
                    .and_then(|n| n.as_f64())
                    .unwrap_or(0.0) as f32;
                let max = v
                    .get("max_exclusive")
                    .and_then(|n| n.as_f64())
                    .unwrap_or(0.0) as f32;
                quote! {
                    pumpkin_util::math::float_provider::FloatProvider::Object(
                        pumpkin_util::math::float_provider::NormalFloatProvider::Uniform(
                            pumpkin_util::math::float_provider::UniformFloatProvider::new(#min, #max)
                        )
                    )
                }
            }
            "minecraft:trapezoid" => {
                let min = v.get("min").and_then(|n| n.as_f64()).unwrap_or(0.0) as f32;
                let max = v.get("max").and_then(|n| n.as_f64()).unwrap_or(0.0) as f32;
                let plateau = v.get("plateau").and_then(|n| n.as_f64()).unwrap_or(0.0) as f32;
                quote! {
                    pumpkin_util::math::float_provider::FloatProvider::Object(
                        pumpkin_util::math::float_provider::NormalFloatProvider::Trapezoid(
                            pumpkin_util::math::float_provider::TrapezoidFloatProvider::new(#min, #max, #plateau)
                        )
                    )
                }
            }
            _ => quote! { pumpkin_util::math::float_provider::FloatProvider::Constant(0.0) },
        }
    } else {
        quote! { pumpkin_util::math::float_provider::FloatProvider::Constant(0.0) }
    }
}

fn parse_velocity_source(v: Option<&serde_json::Value>) -> (f32, TokenStream) {
    let obj = v.and_then(|val| val.as_object());
    let movement_scale = obj
        .and_then(|o| o.get("movement_scale"))
        .and_then(|n| n.as_f64())
        .unwrap_or(0.0) as f32;
    let base = obj.and_then(|o| o.get("base"));
    let base_token = match base {
        Some(b) => parse_float_provider(b),
        None => quote! { pumpkin_util::math::float_provider::FloatProvider::Constant(0.0) },
    };
    (movement_scale, base_token)
}

fn parse_vec3(v: Option<&serde_json::Value>) -> (f64, f64, f64) {
    let arr = v.and_then(|val| val.as_array());
    if let Some(a) = arr {
        let x = a.first().and_then(|n| n.as_f64()).unwrap_or(0.0);
        let y = a.get(1).and_then(|n| n.as_f64()).unwrap_or(0.0);
        let z = a.get(2).and_then(|n| n.as_f64()).unwrap_or(0.0);
        (x, y, z)
    } else {
        (0.0, 0.0, 0.0)
    }
}

fn parse_status_effect_list(v: Option<&serde_json::Value>) -> TokenStream {
    match v {
        Some(serde_json::Value::String(s)) => {
            let raw = s.strip_prefix("minecraft:").unwrap_or(s);
            let ident = format_ident!("{}", raw.to_shouty_snake_case());
            quote! { &[&crate::effect::StatusEffect::#ident] }
        }
        Some(serde_json::Value::Array(arr)) => {
            let idents: Vec<_> = arr
                .iter()
                .filter_map(|x| x.as_str())
                .map(|s| {
                    let raw = s.strip_prefix("minecraft:").unwrap_or(s);
                    let ident = format_ident!("{}", raw.to_shouty_snake_case());
                    quote! { &crate::effect::StatusEffect::#ident }
                })
                .collect();
            quote! { &[#(#idents),*] }
        }
        _ => quote! { &[] },
    }
}

fn parse_entity_effect(val: &serde_json::Value) -> TokenStream {
    if let Some(obj) = val.as_object() {
        let eff_type = obj.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match eff_type {
            "minecraft:ignite" => entity_effects::entity_effect_ignite(obj),
            "minecraft:damage_entity" => entity_effects::entity_effect_damage_entity(obj),
            "minecraft:change_item_damage" => entity_effects::entity_effect_change_item_damage(obj),
            "minecraft:play_sound" => entity_effects::entity_effect_play_sound(obj),
            "minecraft:replace_block" => entity_effects::entity_effect_replace_block(obj),
            "minecraft:set_block_properties" => {
                entity_effects::entity_effect_set_block_properties(obj)
            }
            "minecraft:replace_disk" => entity_effects::entity_effect_replace_disk(obj),
            "minecraft:summon_entity" => entity_effects::entity_effect_summon_entity(obj),
            "minecraft:spawn_particles" => entity_effects::entity_effect_spawn_particles(obj),
            "minecraft:run_function" => entity_effects::entity_effect_run_function(obj),
            "minecraft:apply_exhaustion" => entity_effects::entity_effect_apply_exhaustion(obj),
            "minecraft:apply_impulse" => entity_effects::entity_effect_apply_impulse(obj),
            "minecraft:apply_mob_effect" => entity_effects::entity_effect_apply_mob_effect(obj),
            "minecraft:explode" => entity_effects::entity_effect_explode(obj),
            "minecraft:all_of" => entity_effects::entity_effect_all_of(obj),
            _ => quote! { EnchantmentEntityEffect::Other },
        }
    } else {
        quote! { EnchantmentEntityEffect::Other }
    }
}

fn parse_value_effect(val: &serde_json::Value) -> TokenStream {
    if let Some(obj) = val.as_object() {
        let eff_type = obj.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match eff_type {
            "minecraft:add" => {
                let v =
                    parse_level_based_value(obj.get("value").unwrap_or(&serde_json::Value::Null));
                quote! { EnchantmentValueEffect::Add(#v) }
            }
            "minecraft:multiply" => {
                let v = parse_level_based_value(
                    obj.get("factor")
                        .or_else(|| obj.get("value"))
                        .unwrap_or(&serde_json::Value::Null),
                );
                quote! { EnchantmentValueEffect::Multiply(#v) }
            }
            "minecraft:set" => {
                let v =
                    parse_level_based_value(obj.get("value").unwrap_or(&serde_json::Value::Null));
                quote! { EnchantmentValueEffect::Set(#v) }
            }
            "minecraft:remove_binomial" => {
                let v =
                    parse_level_based_value(obj.get("chance").unwrap_or(&serde_json::Value::Null));
                quote! { EnchantmentValueEffect::RemoveBinomial(#v) }
            }
            _ => quote! { EnchantmentValueEffect::Other },
        }
    } else {
        quote! { EnchantmentValueEffect::Other }
    }
}

fn parse_conditional_entity_effects(val: Option<&serde_json::Value>) -> TokenStream {
    let effects: Vec<TokenStream> = val
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|item| {
                    let eff = item.get("effect").unwrap_or(item);
                    let eff_tokens = parse_entity_effect(eff);
                    quote! { ConditionalEffect { effect: #eff_tokens } }
                })
                .collect()
        })
        .unwrap_or_default();
    quote! { &[#(#effects),*] }
}

fn parse_targeted_conditional_entity_effects(val: Option<&serde_json::Value>) -> TokenStream {
    let effects: Vec<TokenStream> = val
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|item| {
                    let eff = item.get("effect").unwrap_or(item);
                    let eff_tokens = parse_entity_effect(eff);
                    let enchanted = match item.get("enchanted").and_then(|e| e.as_str()) {
                        Some("attacker") => quote! { Some(EnchantmentTarget::Attacker) },
                        Some("damaging_entity") => {
                            quote! { Some(EnchantmentTarget::DamagingEntity) }
                        }
                        Some("victim") => quote! { Some(EnchantmentTarget::Victim) },
                        _ => quote! { None },
                    };
                    let affected = match item.get("affected").and_then(|e| e.as_str()) {
                        Some("attacker") => quote! { Some(EnchantmentTarget::Attacker) },
                        Some("damaging_entity") => {
                            quote! { Some(EnchantmentTarget::DamagingEntity) }
                        }
                        Some("victim") => quote! { Some(EnchantmentTarget::Victim) },
                        _ => quote! { None },
                    };
                    quote! {
                        TargetedConditionalEffect {
                            enchanted: #enchanted,
                            affected: #affected,
                            effect: #eff_tokens,
                        }
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    quote! { &[#(#effects),*] }
}

fn parse_conditional_value_effects(val: Option<&serde_json::Value>) -> TokenStream {
    let effects: Vec<TokenStream> = val
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|item| {
                    let eff = item.get("effect").unwrap_or(item);
                    let eff_tokens = parse_value_effect(eff);
                    quote! { ConditionalEffect { effect: #eff_tokens } }
                })
                .collect()
        })
        .unwrap_or_default();
    quote! { &[#(#effects),*] }
}

fn parse_targeted_conditional_value_effects(val: Option<&serde_json::Value>) -> TokenStream {
    let effects: Vec<TokenStream> = val
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|item| {
                    let eff = item.get("effect").unwrap_or(item);
                    let eff_tokens = parse_value_effect(eff);
                    let enchanted = match item.get("enchanted").and_then(|e| e.as_str()) {
                        Some("attacker") => quote! { Some(EnchantmentTarget::Attacker) },
                        Some("damaging_entity") => {
                            quote! { Some(EnchantmentTarget::DamagingEntity) }
                        }
                        Some("victim") => quote! { Some(EnchantmentTarget::Victim) },
                        _ => quote! { None },
                    };
                    let affected = match item.get("affected").and_then(|e| e.as_str()) {
                        Some("attacker") => quote! { Some(EnchantmentTarget::Attacker) },
                        Some("damaging_entity") => {
                            quote! { Some(EnchantmentTarget::DamagingEntity) }
                        }
                        Some("victim") => quote! { Some(EnchantmentTarget::Victim) },
                        _ => quote! { None },
                    };
                    quote! {
                        TargetedConditionalEffect {
                            enchanted: #enchanted,
                            affected: #affected,
                            effect: #eff_tokens,
                        }
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    quote! { &[#(#effects),*] }
}

fn parse_block_state(v: &serde_json::Value) -> TokenStream {
    let state_val = if let Some(state) = v.get("state") {
        state
    } else {
        v
    };
    let name = state_val
        .get("Name")
        .or_else(|| state_val.get("id"))
        .and_then(|n| n.as_str())
        .unwrap_or("minecraft:air");
    let name_stripped = name.strip_prefix("minecraft:").unwrap_or(name);
    let block_ident = format_ident!("{}", name_stripped.to_shouty_snake_case());
    quote! {
        crate::Block::#block_ident.default_state
    }
}

fn parse_replace_disk_predicate(v: &serde_json::Value) -> TokenStream {
    if v.is_null() {
        return quote! { None };
    }
    let pred_token = parse_single_replace_disk_predicate(v);
    quote! { Some(#pred_token) }
}

fn parse_single_replace_disk_predicate(v: &serde_json::Value) -> TokenStream {
    let type_str = v.get("type").and_then(|s| s.as_str()).unwrap_or("");
    match type_str {
        "minecraft:matching_block_tag" => {
            let offset_x = v
                .get("offset")
                .and_then(|a| a.get(0))
                .and_then(|n| n.as_i64())
                .unwrap_or(0) as i32;
            let offset_y = v
                .get("offset")
                .and_then(|a| a.get(1))
                .and_then(|n| n.as_i64())
                .unwrap_or(0) as i32;
            let offset_z = v
                .get("offset")
                .and_then(|a| a.get(2))
                .and_then(|n| n.as_i64())
                .unwrap_or(0) as i32;
            let tag = v.get("tag").and_then(|s| s.as_str()).unwrap_or("");
            let tag_ident = format_ident!("{}", tag.to_uppercase().replace([':', '-'], "_"));
            quote! {
                ReplaceDiskPredicate::MatchingBlockTag {
                    offset: pumpkin_util::math::vector3::Vector3::new(#offset_x, #offset_y, #offset_z),
                    tag: &crate::tag::Block::#tag_ident,
                }
            }
        }
        "minecraft:matching_blocks" => {
            let offset_x = v
                .get("offset")
                .and_then(|a| a.get(0))
                .and_then(|n| n.as_i64())
                .unwrap_or(0) as i32;
            let offset_y = v
                .get("offset")
                .and_then(|a| a.get(1))
                .and_then(|n| n.as_i64())
                .unwrap_or(0) as i32;
            let offset_z = v
                .get("offset")
                .and_then(|a| a.get(2))
                .and_then(|n| n.as_i64())
                .unwrap_or(0) as i32;
            let blocks_token = match v.get("blocks") {
                Some(serde_json::Value::String(s)) => quote! { &[#s] },
                Some(serde_json::Value::Array(arr)) => {
                    let items: Vec<&str> = arr.iter().filter_map(|x| x.as_str()).collect();
                    quote! { &[#(#items),*] }
                }
                _ => quote! { &[] },
            };
            quote! {
                ReplaceDiskPredicate::MatchingBlocks {
                    offset: pumpkin_util::math::vector3::Vector3::new(#offset_x, #offset_y, #offset_z),
                    blocks: #blocks_token,
                }
            }
        }
        "minecraft:matching_fluids" => {
            let offset_x = v
                .get("offset")
                .and_then(|a| a.get(0))
                .and_then(|n| n.as_i64())
                .unwrap_or(0) as i32;
            let offset_y = v
                .get("offset")
                .and_then(|a| a.get(1))
                .and_then(|n| n.as_i64())
                .unwrap_or(0) as i32;
            let offset_z = v
                .get("offset")
                .and_then(|a| a.get(2))
                .and_then(|n| n.as_i64())
                .unwrap_or(0) as i32;
            let fluids_token = match v.get("fluids") {
                Some(serde_json::Value::String(s)) => quote! { &[#s] },
                Some(serde_json::Value::Array(arr)) => {
                    let items: Vec<&str> = arr.iter().filter_map(|x| x.as_str()).collect();
                    quote! { &[#(#items),*] }
                }
                _ => quote! { &[] },
            };
            quote! {
                ReplaceDiskPredicate::MatchingFluids {
                    offset: pumpkin_util::math::vector3::Vector3::new(#offset_x, #offset_y, #offset_z),
                    fluids: #fluids_token,
                }
            }
        }
        "minecraft:unobstructed" => {
            quote! {
                ReplaceDiskPredicate::Unobstructed
            }
        }
        "minecraft:all_of" => {
            let preds: Vec<TokenStream> = v
                .get("predicates")
                .and_then(|arr| arr.as_array())
                .map(|arr| {
                    arr.iter()
                        .map(parse_single_replace_disk_predicate)
                        .collect()
                })
                .unwrap_or_default();
            quote! {
                ReplaceDiskPredicate::AllOf(&[#(#preds),*])
            }
        }
        _ => quote! {
            ReplaceDiskPredicate::AllOf(&[])
        },
    }
}

fn parse_optional_value_effect(val: Option<&serde_json::Value>) -> TokenStream {
    if let Some(v) = val {
        let eff = v.get("effect").unwrap_or(v);
        let eff_tokens = parse_value_effect(eff);
        quote! { Some(#eff_tokens) }
    } else {
        quote! { None }
    }
}

/// Generates the `TokenStream` for the `Enchantment` struct, `AttributeModifierSlot` enum,
/// data-driven effect types, and `from_name`/`from_id` lookup methods.
pub fn build() -> TokenStream {
    let enchantments = load_enchantments();

    let mut variants = TokenStream::new();
    let mut all_variants = TokenStream::new();
    let mut name_to_type = TokenStream::new();
    let mut id_to_type = TokenStream::new();

    for (name, enchantment) in enchantments {
        push_enchantment(
            name,
            enchantment,
            &mut variants,
            &mut all_variants,
            &mut name_to_type,
            &mut id_to_type,
        );
    }

    let imports_and_level_based_value_tokens = static_items::imports_and_level_based_value_tokens();
    let conditional_effect_tokens = static_items::conditional_effect_tokens();
    let enchantment_entity_effect_tokens = static_items::enchantment_entity_effect_tokens();
    let position_source_type_tokens = static_items::position_source_type_tokens();
    let velocity_source_tokens = static_items::velocity_source_tokens();
    let enchantment_effects_tokens = static_items::enchantment_effects_tokens();
    let cost_tokens = static_items::cost_tokens();
    let can_enchant_tokens = static_items::can_enchant_tokens();
    let modify_ammo_count_tokens = static_items::modify_ammo_count_tokens();
    let modify_damage_against_tokens = static_items::modify_damage_against_tokens();
    let tests_tokens = static_items::tests_tokens();

    quote! {
        #imports_and_level_based_value_tokens

        #conditional_effect_tokens

        #enchantment_entity_effect_tokens

        #position_source_type_tokens

        #velocity_source_tokens

        #enchantment_effects_tokens

        #cost_tokens

        impl Enchantment {
            pub const ALL: &'static [&'static Self] = &[#all_variants];

            pub fn all() -> Iter<'static, &'static Self> {
                Self::ALL.iter()
            }

            #variants

            pub fn from_name(name: &str) -> Option<&'static Self> {
                match name {
                    #name_to_type
                    _ => None
                }
            }
            pub fn from_id(id: u8) -> Option<&'static Self> {
                match id {
                    #id_to_type
                    _ => None
                }
            }

            #can_enchant_tokens

            #modify_ammo_count_tokens

            #modify_damage_against_tokens
        }

        #tests_tokens
    }
}
