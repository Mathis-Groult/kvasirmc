//! Token streams of the advancement trigger conditions that carry data.

use super::{
    extract_damage_condition_tokens, generate_entity_condition_tokens,
    generate_item_condition_tokens, generate_item_used_on_block_condition_tokens,
    generate_location_condition_tokens, opt_f64_tokens, opt_i32_tokens, opt_str_tokens,
};
use heck::ToShoutySnakeCase;
use proc_macro2::TokenStream;
use pumpkin_util::identifier::Identifier;
use pumpkin_util::resource_location::ResourceLocation;
use pumpkin_util::text::TextComponent;
use pumpkin_util::text::TextContent::Translate;
use quote::{ToTokens, format_ident, quote};
use serde::{Deserialize, Deserializer, Serialize};
use std::cmp::PartialEq;
use std::fmt::Display;
use std::hash::{Hash, Hasher};
use std::{collections::BTreeMap, fs};

pub(super) fn condition_player_generates_container_loot(val: &serde_json::Value) -> TokenStream {
    let lt = val
        .get("loot_tables")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    quote! { AdvancementCondition::PlayerGeneratesContainerLoot(#lt) }
}

pub(super) fn condition_recipe_unlocked(val: &serde_json::Value) -> TokenStream {
    let r = val
        .get("recipes")
        .or_else(|| val.get("recipe"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    quote! { AdvancementCondition::RecipeUnlocked(#r) }
}

pub(super) fn condition_recipe_crafted(val: &serde_json::Value) -> TokenStream {
    let r = val.get("recipes").and_then(|v| v.as_str()).unwrap_or("");
    quote! { AdvancementCondition::RecipeCrafted(#r) }
}

pub(super) fn condition_inventory_changed(val: &serde_json::Value) -> TokenStream {
    let items_toks: Vec<_> = val
        .get("items")
        .and_then(|i| i.as_array())
        .map_or_else(Vec::new, |arr| {
            arr.iter().map(generate_item_condition_tokens).collect()
        });
    quote! {
        AdvancementCondition::InventoryChanged(&InventoryChangedCondition {
            items: &[#(#items_toks),*],
            slots: None,
        })
    }
}

pub(super) fn condition_consume_item(val: &serde_json::Value) -> TokenStream {
    let item_val = val.get("item").unwrap_or(val);
    let item_tok = generate_item_condition_tokens(item_val);
    quote! { AdvancementCondition::ConsumeItem(#item_tok) }
}

pub(super) fn condition_placed_block(val: &serde_json::Value) -> TokenStream {
    let mut block = val
        .get("block")
        .or_else(|| val.get("blocks"))
        .and_then(|v| v.as_str());
    if block.is_none() {
        if let Some(loc) = val.get("location") {
            block = loc
                .get("blocks")
                .or_else(|| loc.get("block"))
                .and_then(|v| v.as_str());
            if block.is_none() {
                let loc_str = loc.to_string();
                if loc_str.contains("chiseled_bookshelf") {
                    block = Some("minecraft:chiseled_bookshelf");
                } else if loc_str.contains("comparator") {
                    block = Some("minecraft:comparator");
                } else if loc_str.contains("creaking_heart") {
                    block = Some("minecraft:creaking_heart");
                } else if loc_str.contains("pale_oak_logs") {
                    block = Some("minecraft:pale_oak_log");
                }
            }
        }
    }
    let b_tok = opt_str_tokens(block);
    quote! {
        AdvancementCondition::PlacedBlock {
            block: #b_tok,
            item: None,
        }
    }
}

pub(super) fn condition_item_used_on_block(val: &serde_json::Value) -> TokenStream {
    let cond_tok = generate_item_used_on_block_condition_tokens(val);
    quote! { AdvancementCondition::ItemUsedOnBlock(#cond_tok) }
}

pub(super) fn condition_allay_drop_item_on_block(val: &serde_json::Value) -> TokenStream {
    let cond_tok = generate_item_used_on_block_condition_tokens(val);
    quote! { AdvancementCondition::AllayDropItemOnBlock(#cond_tok) }
}

pub(super) fn condition_entity_hurt_player(val: &serde_json::Value) -> TokenStream {
    let dmg_tok = extract_damage_condition_tokens(val);
    quote! { AdvancementCondition::EntityHurtPlayer(#dmg_tok) }
}

pub(super) fn condition_player_hurt_entity(val: &serde_json::Value) -> TokenStream {
    let dmg_tok = extract_damage_condition_tokens(val);
    quote! { AdvancementCondition::PlayerHurtEntity(#dmg_tok) }
}

pub(super) fn condition_enter_block(val: &serde_json::Value) -> TokenStream {
    let b = val
        .get("blocks")
        .or_else(|| val.get("block"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    quote! { AdvancementCondition::EnterBlock(#b) }
}

pub(super) fn condition_slide_down_block(val: &serde_json::Value) -> TokenStream {
    let b = val
        .get("blocks")
        .or_else(|| val.get("block"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    quote! { AdvancementCondition::SlideDownBlock(#b) }
}

pub(super) fn condition_filled_bucket(val: &serde_json::Value) -> TokenStream {
    let item = val
        .get("item")
        .and_then(|i| i.get("items"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    quote! { AdvancementCondition::FilledBucket(#item) }
}

pub(super) fn condition_fishing_rod_hooked(val: &serde_json::Value) -> TokenStream {
    let item_val = val.get("item").unwrap_or(val);
    let item_tok = generate_item_condition_tokens(item_val);
    quote! { AdvancementCondition::FishingRodHooked(#item_tok) }
}

pub(super) fn condition_using_item(val: &serde_json::Value) -> TokenStream {
    let item_val = val.get("item").unwrap_or(val);
    let item_tok = generate_item_condition_tokens(item_val);
    let looking_at = val
        .get("player")
        .and_then(|p| p.get("predicate"))
        .and_then(|pred| pred.get("minecraft:type_specific/player"))
        .and_then(|p| p.get("looking_at"))
        .and_then(|l| {
            l.get("minecraft:entity_type")
                .or_else(|| l.get("type"))
                .and_then(|t| t.as_str())
        });
    let look_tok = opt_str_tokens(looking_at);
    quote! {
        AdvancementCondition::UsingItem {
            item: #item_tok,
            looking_at: #look_tok,
        }
    }
}

pub(super) fn condition_used_totem(val: &serde_json::Value) -> TokenStream {
    let item_val = val.get("item").unwrap_or(val);
    let item_tok = generate_item_condition_tokens(item_val);
    quote! { AdvancementCondition::UsedTotem(#item_tok) }
}

pub(super) fn condition_changed_dimension(val: &serde_json::Value) -> TokenStream {
    let from = val.get("from").and_then(|v| v.as_str());
    let to = val.get("to").and_then(|v| v.as_str());
    let from_tok = opt_str_tokens(from);
    let to_tok = opt_str_tokens(to);
    quote! { AdvancementCondition::ChangedDimension { from: #from_tok, to: #to_tok } }
}

pub(super) fn condition_construct_beacon(val: &serde_json::Value) -> TokenStream {
    let lvl = val.get("level").and_then(|l| match l {
        serde_json::Value::Number(n) => n.as_i64().map(|v| v as i32),
        serde_json::Value::Object(m) => m.get("min").and_then(|v| v.as_i64()).map(|v| v as i32),
        _ => None,
    });
    let lvl_tok = opt_i32_tokens(lvl);
    quote! { AdvancementCondition::ConstructBeacon(BeaconCondition { level_min: #lvl_tok }) }
}

pub(super) fn condition_target_hit(val: &serde_json::Value) -> TokenStream {
    let sig = val
        .get("signal_strength")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);
    let dist = val
        .get("projectile")
        .and_then(|p| p.get("distance"))
        .and_then(|d| d.get("horizontal"))
        .and_then(|h| h.get("min"))
        .and_then(|v| v.as_f64());
    let sig_tok = opt_i32_tokens(sig);
    let dist_tok = opt_f64_tokens(dist);
    quote! {
        AdvancementCondition::TargetHit(TargetHitCondition {
            signal_strength: #sig_tok,
            distance_min: #dist_tok,
        })
    }
}

pub(super) fn condition_player_killed_entity(val: &serde_json::Value) -> TokenStream {
    let entity_tok = generate_entity_condition_tokens(val);
    quote! { AdvancementCondition::PlayerKilledEntity(#entity_tok) }
}

pub(super) fn condition_tame_animal(val: &serde_json::Value) -> TokenStream {
    let entity_tok = generate_entity_condition_tokens(val);
    quote! { AdvancementCondition::TameAnimal(#entity_tok) }
}

pub(super) fn condition_thrown_item_picked_up_by_player(val: &serde_json::Value) -> TokenStream {
    let entity_tok = generate_entity_condition_tokens(val);
    quote! { AdvancementCondition::ThrownItemPickedUpByPlayer(#entity_tok) }
}

pub(super) fn condition_bred_animals(val: &serde_json::Value) -> TokenStream {
    let parent_tok = val.get("parent").map(generate_entity_condition_tokens);
    let partner_tok = val.get("partner").map(generate_entity_condition_tokens);
    let child_tok = val.get("child").map(generate_entity_condition_tokens);
    let p_tok = match parent_tok {
        Some(t) => quote! { Some(#t) },
        None => quote! { None },
    };
    let pr_tok = match partner_tok {
        Some(t) => quote! { Some(#t) },
        None => quote! { None },
    };
    let c_tok = match child_tok {
        Some(t) => quote! { Some(#t) },
        None => quote! { None },
    };
    quote! {
        AdvancementCondition::BredAnimals(&BredAnimalsCondition {
            parent: #p_tok,
            partner: #pr_tok,
            child: #c_tok,
        })
    }
}

pub(super) fn condition_location(val: &serde_json::Value) -> TokenStream {
    let loc_tok = generate_location_condition_tokens(val);
    quote! { AdvancementCondition::Location(#loc_tok) }
}

pub(super) fn condition_item_durability_changed(val: &serde_json::Value) -> TokenStream {
    let item_val = val.get("item").unwrap_or(val);
    let item_tok = generate_item_condition_tokens(item_val);
    let vtype = val
        .get("player")
        .and_then(|p| p.get("predicate"))
        .and_then(|pred| {
            pred.get("minecraft:vehicle")
                .or_else(|| pred.get("vehicle"))
        })
        .and_then(|v| v.get("minecraft:entity_type").or_else(|| v.get("type")))
        .and_then(|t| t.as_str());
    let vt_tok = opt_str_tokens(vtype);
    quote! {
        AdvancementCondition::ItemDurabilityChanged {
            item: #item_tok,
            vehicle_type: #vt_tok,
        }
    }
}

pub(super) fn condition_levitation(val: &serde_json::Value) -> TokenStream {
    let dist = val
        .get("distance")
        .and_then(|d| d.get("y"))
        .and_then(|y| y.get("min"))
        .and_then(|v| v.as_f64());
    let d_tok = opt_f64_tokens(dist);
    quote! { AdvancementCondition::Levitation { distance_y_min: #d_tok } }
}

pub(super) fn condition_fall_from_height(val: &serde_json::Value) -> TokenStream {
    let dist = val
        .get("distance")
        .and_then(|d| d.get("y"))
        .and_then(|y| y.get("min"))
        .and_then(|v| v.as_f64());
    let d_tok = opt_f64_tokens(dist);
    quote! { AdvancementCondition::FallFromHeight { distance_y_min: #d_tok } }
}

pub(super) fn condition_fall_after_explosion(val: &serde_json::Value) -> TokenStream {
    let dist = val
        .get("distance")
        .and_then(|d| d.get("y"))
        .and_then(|y| y.get("min"))
        .and_then(|v| v.as_f64());
    let cause = val
        .get("cause")
        .and_then(|c| c.get("predicate"))
        .and_then(|p| {
            p.get("minecraft:entity_type")
                .or_else(|| p.get("entity_type"))
        })
        .and_then(|v| v.as_str());
    let d_tok = opt_f64_tokens(dist);
    let c_tok = opt_str_tokens(cause);
    quote! {
        AdvancementCondition::FallAfterExplosion {
            distance_y_min: #d_tok,
            cause_entity: #c_tok,
        }
    }
}

pub(super) fn condition_nether_travel(val: &serde_json::Value) -> TokenStream {
    let dist = val
        .get("distance")
        .and_then(|d| d.get("horizontal"))
        .and_then(|h| h.get("min"))
        .and_then(|v| v.as_f64());
    let d_tok = opt_f64_tokens(dist);
    quote! { AdvancementCondition::NetherTravel { distance_horizontal_min: #d_tok } }
}

pub(super) fn condition_ride_entity_in_lava(val: &serde_json::Value) -> TokenStream {
    let dist = val
        .get("distance")
        .and_then(|d| d.get("horizontal"))
        .and_then(|h| h.get("min"))
        .and_then(|v| v.as_f64());
    let dim = val
        .get("player")
        .and_then(|p| p.get("predicate"))
        .and_then(|pred| {
            pred.get("minecraft:location")
                .or_else(|| pred.get("location"))
        })
        .and_then(|l| l.get("dimension"))
        .and_then(|d| d.as_str());
    let vtype = val
        .get("player")
        .and_then(|p| p.get("predicate"))
        .and_then(|pred| {
            pred.get("minecraft:vehicle")
                .or_else(|| pred.get("vehicle"))
        })
        .and_then(|v| v.get("minecraft:entity_type").or_else(|| v.get("type")))
        .and_then(|t| t.as_str());
    let d_tok = opt_f64_tokens(dist);
    let dim_tok = opt_str_tokens(dim);
    let vt_tok = opt_str_tokens(vtype);
    quote! {
        AdvancementCondition::RideEntityInLava {
            distance_horizontal_min: #d_tok,
            dimension: #dim_tok,
            vehicle_type: #vt_tok,
        }
    }
}

pub(super) fn condition_shot_crossbow(val: &serde_json::Value) -> TokenStream {
    let item_val = val.get("item").unwrap_or(val);
    let item_tok = generate_item_condition_tokens(item_val);
    quote! { AdvancementCondition::ShotCrossbow(#item_tok) }
}

pub(super) fn condition_bee_nest_destroyed(val: &serde_json::Value) -> TokenStream {
    let block = val
        .get("blocks")
        .or_else(|| val.get("block"))
        .and_then(|v| v.as_str());
    let bees = val
        .get("num_bees_inside")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);
    let b_tok = opt_str_tokens(block);
    let bees_tok = opt_i32_tokens(bees);
    quote! {
        AdvancementCondition::BeeNestDestroyed {
            block: #b_tok,
            num_bees_inside: #bees_tok,
        }
    }
}

pub(super) fn condition_channeled_lightning(val: &serde_json::Value) -> TokenStream {
    let victim = val
        .get("victims")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|e| e.get("predicate"))
        .and_then(|p| {
            p.get("minecraft:entity_type")
                .or_else(|| p.get("entity_type"))
        })
        .and_then(|v| v.as_str());
    let v_tok = opt_str_tokens(victim);
    quote! { AdvancementCondition::ChanneledLightning { victim_type: #v_tok } }
}

pub(super) fn condition_lightning_strike(val: &serde_json::Value) -> TokenStream {
    let bystander = val
        .get("bystander")
        .and_then(|b| b.get("predicate"))
        .and_then(|p| {
            p.get("minecraft:entity_type")
                .or_else(|| p.get("entity_type"))
        })
        .and_then(|v| v.as_str());
    let fire = val
        .get("lightning")
        .and_then(|l| l.get("predicate"))
        .and_then(|p| p.get("minecraft:type_specific/lightning"))
        .and_then(|t| t.get("blocks_set_on_fire"))
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);
    let max_dist = val
        .get("lightning")
        .and_then(|l| l.get("predicate"))
        .and_then(|p| p.get("minecraft:distance"))
        .and_then(|d| d.get("absolute"))
        .and_then(|a| a.get("max"))
        .and_then(|v| v.as_f64());
    let b_tok = opt_str_tokens(bystander);
    let f_tok = opt_i32_tokens(fire);
    let md_tok = opt_f64_tokens(max_dist);
    quote! {
        AdvancementCondition::LightningStrike {
            bystander_type: #b_tok,
            blocks_set_on_fire: #f_tok,
            distance_max: #md_tok,
        }
    }
}

pub(super) fn condition_player_interacted_with_entity(val: &serde_json::Value) -> TokenStream {
    let ent_tok = val.get("entity").map(generate_entity_condition_tokens);
    let item_tok = val.get("item").map(generate_item_condition_tokens);
    let player_tok = val.get("player").map(generate_entity_condition_tokens);
    let e_tok = match ent_tok {
        Some(t) => quote! { Some(#t) },
        None => quote! { None },
    };
    let i_tok = match item_tok {
        Some(t) => quote! { Some(#t) },
        None => quote! { None },
    };
    let p_tok = match player_tok {
        Some(t) => quote! { Some(#t) },
        None => quote! { None },
    };
    quote! {
        AdvancementCondition::PlayerInteractedWithEntity {
            entity: #e_tok,
            item: #i_tok,
            player: #p_tok,
        }
    }
}

pub(super) fn condition_thrown_item_picked_up_by_entity(val: &serde_json::Value) -> TokenStream {
    let ent_tok = val.get("entity").map(generate_entity_condition_tokens);
    let item_tok = val.get("item").map(generate_item_condition_tokens);
    let player_tok = val.get("player").map(generate_entity_condition_tokens);
    let e_tok = match ent_tok {
        Some(t) => quote! { Some(#t) },
        None => quote! { None },
    };
    let i_tok = match item_tok {
        Some(t) => quote! { Some(#t) },
        None => quote! { None },
    };
    let p_tok = match player_tok {
        Some(t) => quote! { Some(#t) },
        None => quote! { None },
    };
    quote! {
        AdvancementCondition::ThrownItemPickedUpByEntity {
            entity: #e_tok,
            item: #i_tok,
            player: #p_tok,
        }
    }
}

pub(super) fn condition_player_sheared_equipment(val: &serde_json::Value) -> TokenStream {
    let ent_tok = val.get("entity").map(generate_entity_condition_tokens);
    let item_tok = val.get("item").map(generate_item_condition_tokens);
    let e_tok = match ent_tok {
        Some(t) => quote! { Some(#t) },
        None => quote! { None },
    };
    let i_tok = match item_tok {
        Some(t) => quote! { Some(#t) },
        None => quote! { None },
    };
    quote! {
        AdvancementCondition::PlayerShearedEquipment {
            entity: #e_tok,
            item: #i_tok,
        }
    }
}

pub(super) fn condition_started_riding(val: &serde_json::Value) -> TokenStream {
    let player_tok = val.get("player").map(generate_entity_condition_tokens);
    let p_tok = match player_tok {
        Some(t) => quote! { Some(#t) },
        None => quote! { None },
    };
    quote! { AdvancementCondition::StartedRiding(#p_tok) }
}

pub(super) fn condition_summoned_entity(val: &serde_json::Value) -> TokenStream {
    let ent_val = val.get("entity").unwrap_or(val);
    let ent_tok = generate_entity_condition_tokens(ent_val);
    quote! { AdvancementCondition::SummonedEntity(#ent_tok) }
}

pub(super) fn condition_crafter_recipe_crafted(val: &serde_json::Value) -> TokenStream {
    let r = val
        .get("recipes")
        .or_else(|| val.get("recipe"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    quote! { AdvancementCondition::CrafterRecipeCrafted(#r) }
}

pub(super) fn condition_spear_mobs(val: &serde_json::Value) -> TokenStream {
    let cnt = val.get("count").and_then(|v| v.as_i64()).map(|v| v as i32);
    let c_tok = opt_i32_tokens(cnt);
    quote! { AdvancementCondition::SpearMobs { count: #c_tok } }
}
