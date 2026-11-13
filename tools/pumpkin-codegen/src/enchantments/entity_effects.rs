//! Token streams of the enchantment entity effects (one function per effect type).

use super::{
    parse_block_state, parse_entity_effect, parse_float_provider, parse_level_based_value,
    parse_particle, parse_position_source, parse_replace_disk_predicate, parse_status_effect_list,
    parse_vec3, parse_velocity_source,
};
use heck::{ToPascalCase, ToShoutySnakeCase};
use proc_macro2::TokenStream;
use pumpkin_util::text::TextComponent;
use pumpkin_util::text::TextContent::Translate;
use quote::{format_ident, quote};
use serde::Deserialize;
use std::{collections::BTreeMap, fs};

pub(super) fn entity_effect_ignite(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> TokenStream {
    let dur = parse_level_based_value(obj.get("duration").unwrap_or(&serde_json::Value::Null));
    quote! { EnchantmentEntityEffect::Ignite { duration: #dur } }
}

pub(super) fn entity_effect_damage_entity(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> TokenStream {
    let min_damage = parse_level_based_value(
        obj.get("min_damage")
            .or_else(|| obj.get("damage"))
            .unwrap_or(&serde_json::Value::Null),
    );
    let max_damage = parse_level_based_value(
        obj.get("max_damage")
            .or_else(|| obj.get("damage"))
            .unwrap_or(&serde_json::Value::Null),
    );
    let damage_type = obj.get("damage_type").and_then(|s| s.as_str());
    let damage_type_token = match damage_type {
        Some(dt) => {
            let dt_raw = dt.strip_prefix("minecraft:").unwrap_or(dt);
            let dt_ident = format_ident!("{}", dt_raw.to_shouty_snake_case());
            quote! { Some(&crate::damage::DamageType::#dt_ident) }
        }
        None => quote! { None },
    };
    quote! {
        EnchantmentEntityEffect::DamageEntity {
            min_damage: #min_damage,
            max_damage: #max_damage,
            damage_type: #damage_type_token,
        }
    }
}

pub(super) fn entity_effect_change_item_damage(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> TokenStream {
    let amount = parse_level_based_value(obj.get("amount").unwrap_or(&serde_json::Value::Null));
    quote! {
        EnchantmentEntityEffect::ChangeItemDamage {
            amount: #amount,
        }
    }
}

pub(super) fn entity_effect_play_sound(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> TokenStream {
    let sound = obj.get("sound").and_then(|s| s.as_str()).unwrap_or("");
    quote! { EnchantmentEntityEffect::PlaySound { sound: #sound } }
}

pub(super) fn entity_effect_replace_block(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> TokenStream {
    let offset_x = obj
        .get("offset")
        .and_then(|v| v.get(0))
        .and_then(|n| n.as_i64())
        .unwrap_or(0) as i32;
    let offset_y = obj
        .get("offset")
        .and_then(|v| v.get(1))
        .and_then(|n| n.as_i64())
        .unwrap_or(0) as i32;
    let offset_z = obj
        .get("offset")
        .and_then(|v| v.get(2))
        .and_then(|n| n.as_i64())
        .unwrap_or(0) as i32;
    let event = obj.get("trigger_game_event").and_then(|s| s.as_str());
    let event_token = match event {
        Some(ev) => {
            let ev_raw = ev.strip_prefix("minecraft:").unwrap_or(ev);
            let ev_ident = format_ident!("{}", ev_raw.to_pascal_case());
            quote! { Some(crate::game_event::GameEvent::#ev_ident) }
        }
        None => quote! { None },
    };
    quote! {
        EnchantmentEntityEffect::ReplaceBlock {
            offset_x: #offset_x,
            offset_y: #offset_y,
            offset_z: #offset_z,
            trigger_game_event: #event_token,
        }
    }
}

pub(super) fn entity_effect_set_block_properties(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> TokenStream {
    let offset_x = obj
        .get("offset")
        .and_then(|v| v.get(0))
        .and_then(|n| n.as_i64())
        .unwrap_or(0) as i32;
    let offset_y = obj
        .get("offset")
        .and_then(|v| v.get(1))
        .and_then(|n| n.as_i64())
        .unwrap_or(0) as i32;
    let offset_z = obj
        .get("offset")
        .and_then(|v| v.get(2))
        .and_then(|n| n.as_i64())
        .unwrap_or(0) as i32;
    let event = obj.get("trigger_game_event").and_then(|s| s.as_str());
    let event_token = match event {
        Some(ev) => {
            let ev_raw = ev.strip_prefix("minecraft:").unwrap_or(ev);
            let ev_ident = format_ident!("{}", ev_raw.to_pascal_case());
            quote! { Some(crate::game_event::GameEvent::#ev_ident) }
        }
        None => quote! { None },
    };
    let properties_obj = obj.get("properties").and_then(|p| p.as_object());
    let properties_tokens: Vec<_> = properties_obj
        .map(|o| {
            o.iter()
                .map(|(k, v)| {
                    let val_str = v.as_str().unwrap_or("");
                    quote! { (#k, #val_str) }
                })
                .collect()
        })
        .unwrap_or_default();
    quote! {
        EnchantmentEntityEffect::SetBlockProperties {
            properties: &[#(#properties_tokens),*],
            offset_x: #offset_x,
            offset_y: #offset_y,
            offset_z: #offset_z,
            trigger_game_event: #event_token,
        }
    }
}

pub(super) fn entity_effect_replace_disk(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> TokenStream {
    let radius = parse_level_based_value(obj.get("radius").unwrap_or(&serde_json::Value::Null));
    let height = parse_level_based_value(obj.get("height").unwrap_or(&serde_json::Value::Null));
    let offset_x = obj
        .get("offset")
        .and_then(|v| v.get(0))
        .and_then(|n| n.as_i64())
        .unwrap_or(0) as i32;
    let offset_y = obj
        .get("offset")
        .and_then(|v| v.get(1))
        .and_then(|n| n.as_i64())
        .unwrap_or(0) as i32;
    let offset_z = obj
        .get("offset")
        .and_then(|v| v.get(2))
        .and_then(|n| n.as_i64())
        .unwrap_or(0) as i32;
    let event = obj.get("trigger_game_event").and_then(|s| s.as_str());
    let event_token = match event {
        Some(ev) => {
            let ev_raw = ev.strip_prefix("minecraft:").unwrap_or(ev);
            let ev_ident = format_ident!("{}", ev_raw.to_pascal_case());
            quote! { Some(crate::game_event::GameEvent::#ev_ident) }
        }
        None => quote! { None },
    };
    let block_state_token = obj.get("block_state").map_or_else(
        || quote! { crate::block::Block::FROSTED_ICE.default_state },
        parse_block_state,
    );
    let predicate_token = obj
        .get("predicate")
        .map_or_else(|| quote! { None }, parse_replace_disk_predicate);
    quote! {
        EnchantmentEntityEffect::ReplaceDisk {
            radius: #radius,
            height: #height,
            offset_x: #offset_x,
            offset_y: #offset_y,
            offset_z: #offset_z,
            predicate: #predicate_token,
            block_state: #block_state_token,
            trigger_game_event: #event_token,
        }
    }
}

pub(super) fn entity_effect_summon_entity(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> TokenStream {
    let entity_val = obj.get("entity");
    let entity_token = match entity_val {
        Some(serde_json::Value::String(s)) => {
            let raw = s.strip_prefix("minecraft:").unwrap_or(s);
            let ident = format_ident!("{}", raw.to_shouty_snake_case());
            quote! { &[&crate::entity::EntityType::#ident] }
        }
        Some(serde_json::Value::Array(arr)) => {
            let idents: Vec<TokenStream> = arr
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| {
                    let raw = s.strip_prefix("minecraft:").unwrap_or(s);
                    let ident = format_ident!("{}", raw.to_shouty_snake_case());
                    quote! { &crate::entity::EntityType::#ident }
                })
                .collect();
            quote! { &[#(#idents),*] }
        }
        _ => quote! { &[] },
    };
    let join_team = obj
        .get("join_team")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);
    quote! {
        EnchantmentEntityEffect::SummonEntity {
            entity_types: #entity_token,
            join_team: #join_team,
        }
    }
}

pub(super) fn entity_effect_spawn_particles(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> TokenStream {
    let particle_val = obj.get("particle").unwrap_or(&serde_json::Value::Null);
    let particle_token = parse_particle(particle_val);
    let (h_pos_type, h_pos_off, h_pos_scale) =
        parse_position_source(obj.get("horizontal_position"));
    let (v_pos_type, v_pos_off, v_pos_scale) = parse_position_source(obj.get("vertical_position"));
    let (h_vel_scale, h_vel_base) = parse_velocity_source(obj.get("horizontal_velocity"));
    let (v_vel_scale, v_vel_base) = parse_velocity_source(obj.get("vertical_velocity"));
    let speed_token = parse_float_provider(obj.get("speed").unwrap_or(&serde_json::Value::Null));

    quote! {
        EnchantmentEntityEffect::SpawnParticles {
            particle: #particle_token,
            horizontal_position: PositionSource {
                source_type: #h_pos_type,
                offset: #h_pos_off,
                scale: #h_pos_scale,
            },
            vertical_position: PositionSource {
                source_type: #v_pos_type,
                offset: #v_pos_off,
                scale: #v_pos_scale,
            },
            horizontal_velocity: VelocitySource {
                movement_scale: #h_vel_scale,
                base: #h_vel_base,
            },
            vertical_velocity: VelocitySource {
                movement_scale: #v_vel_scale,
                base: #v_vel_base,
            },
            speed: #speed_token,
        }
    }
}

pub(super) fn entity_effect_run_function(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> TokenStream {
    let function = obj.get("function").and_then(|s| s.as_str()).unwrap_or("");
    quote! {
        EnchantmentEntityEffect::RunFunction {
            function: #function,
        }
    }
}

pub(super) fn entity_effect_apply_exhaustion(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> TokenStream {
    let amount = parse_level_based_value(obj.get("amount").unwrap_or(&serde_json::Value::Null));
    quote! {
        EnchantmentEntityEffect::ApplyExhaustion {
            amount: #amount,
        }
    }
}

pub(super) fn entity_effect_apply_impulse(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> TokenStream {
    let (dir_x, dir_y, dir_z) = parse_vec3(obj.get("direction"));
    let (scale_x, scale_y, scale_z) = parse_vec3(obj.get("coordinate_scale"));
    let mag = parse_level_based_value(obj.get("magnitude").unwrap_or(&serde_json::Value::Null));
    quote! {
        EnchantmentEntityEffect::ApplyImpulse {
            direction: pumpkin_util::math::vector3::Vector3::new(#dir_x, #dir_y, #dir_z),
            coordinate_scale: pumpkin_util::math::vector3::Vector3::new(#scale_x, #scale_y, #scale_z),
            magnitude: #mag,
        }
    }
}

pub(super) fn entity_effect_apply_mob_effect(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> TokenStream {
    let to_apply = parse_status_effect_list(obj.get("to_apply"));
    let min_duration =
        parse_level_based_value(obj.get("min_duration").unwrap_or(&serde_json::Value::Null));
    let max_duration =
        parse_level_based_value(obj.get("max_duration").unwrap_or(&serde_json::Value::Null));
    let min_amplifier =
        parse_level_based_value(obj.get("min_amplifier").unwrap_or(&serde_json::Value::Null));
    let max_amplifier =
        parse_level_based_value(obj.get("max_amplifier").unwrap_or(&serde_json::Value::Null));
    quote! {
        EnchantmentEntityEffect::ApplyMobEffect {
            to_apply: #to_apply,
            min_duration: #min_duration,
            max_duration: #max_duration,
            min_amplifier: #min_amplifier,
            max_amplifier: #max_amplifier,
        }
    }
}

pub(super) fn entity_effect_explode(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> TokenStream {
    let attribute_to_user = obj
        .get("attribute_to_user")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);
    let damage_type = obj.get("damage_type").and_then(|s| s.as_str());
    let damage_type_token = match damage_type {
        Some(dt) => {
            let dt_raw = dt.strip_prefix("minecraft:").unwrap_or(dt);
            let dt_ident = format_ident!("{}", dt_raw.to_shouty_snake_case());
            quote! { Some(&crate::damage::DamageType::#dt_ident) }
        }
        None => quote! { None },
    };
    let knockback_multiplier = obj.get("knockback_multiplier").map(parse_level_based_value);
    let knockback_token = match knockback_multiplier {
        Some(kb) => quote! { Some(#kb) },
        None => quote! { None },
    };
    let immune_blocks = obj.get("immune_blocks").and_then(|s| s.as_str());
    let immune_blocks_token = match immune_blocks {
        Some(ib) => quote! { Some(#ib) },
        None => quote! { None },
    };
    let offset_x = obj
        .get("offset")
        .and_then(|v| v.get(0))
        .and_then(|n| n.as_f64())
        .unwrap_or(0.0);
    let offset_y = obj
        .get("offset")
        .and_then(|v| v.get(1))
        .and_then(|n| n.as_f64())
        .unwrap_or(0.0);
    let offset_z = obj
        .get("offset")
        .and_then(|v| v.get(2))
        .and_then(|n| n.as_f64())
        .unwrap_or(0.0);
    let radius = parse_level_based_value(obj.get("radius").unwrap_or(&serde_json::Value::Null));
    let create_fire = obj
        .get("create_fire")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);
    let block_interaction = obj
        .get("block_interaction")
        .and_then(|s| s.as_str())
        .unwrap_or("none");
    let (small_particle_token, large_particle_token, sound_token) =
        explode_particle_and_sound_tokens(obj);

    quote! {
        EnchantmentEntityEffect::Explode {
            attribute_to_user: #attribute_to_user,
            damage_type: #damage_type_token,
            knockback_multiplier: #knockback_token,
            immune_blocks: #immune_blocks_token,
            offset_x: #offset_x,
            offset_y: #offset_y,
            offset_z: #offset_z,
            radius: #radius,
            create_fire: #create_fire,
            block_interaction: #block_interaction,
            small_particle: #small_particle_token,
            large_particle: #large_particle_token,
            sound: #sound_token,
        }
    }
}

/// The particle and sound tokens of an `explode` effect.
fn explode_particle_and_sound_tokens(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> (TokenStream, TokenStream, TokenStream) {
    let small_particle = obj.get("small_particle").and_then(|p| {
        p.get("type")
            .and_then(|t| t.as_str())
            .or_else(|| p.as_str())
    });
    let small_particle_token = match small_particle {
        Some(p) => {
            let p_raw = p.strip_prefix("minecraft:").unwrap_or(p);
            let p_ident = format_ident!("{}", p_raw.to_pascal_case());
            quote! { Some(crate::particle::Particle::#p_ident) }
        }
        None => quote! { None },
    };
    let large_particle = obj.get("large_particle").and_then(|p| {
        p.get("type")
            .and_then(|t| t.as_str())
            .or_else(|| p.as_str())
    });
    let large_particle_token = match large_particle {
        Some(p) => {
            let p_raw = p.strip_prefix("minecraft:").unwrap_or(p);
            let p_ident = format_ident!("{}", p_raw.to_pascal_case());
            quote! { Some(crate::particle::Particle::#p_ident) }
        }
        None => quote! { None },
    };
    let sound = obj.get("sound").and_then(|s| s.as_str());
    let sound_token = match sound {
        Some(s) => {
            let s_raw = s.strip_prefix("minecraft:").unwrap_or(s);
            let s_ident = format_ident!("{}", s_raw.to_pascal_case());
            quote! { Some(crate::sound::Sound::#s_ident) }
        }
        None => quote! { None },
    };
    (small_particle_token, large_particle_token, sound_token)
}

pub(super) fn entity_effect_all_of(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> TokenStream {
    let inner: Vec<TokenStream> = obj
        .get("effects")
        .and_then(|arr| arr.as_array())
        .map(|arr| arr.iter().map(parse_entity_effect).collect())
        .unwrap_or_default();
    quote! { EnchantmentEntityEffect::AllOf(&[#(#inner),*]) }
}
