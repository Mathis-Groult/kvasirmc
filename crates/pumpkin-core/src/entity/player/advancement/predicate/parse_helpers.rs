//! The condition readers `CriterionConditions::parse` uses for triggers whose JSON has no
//! predicate type of its own (record: `tests/golden/advancement_parse.txt`).

use serde_json::Value;

use super::{CriterionConditions, FloatBounds, TagOrIdMatcher};

/// The block (or tag) a trigger names, under `blocks` or else `block`.
pub(super) fn block_matcher(val: &Value) -> TagOrIdMatcher {
    val.get("blocks")
        .or_else(|| val.get("block"))
        .and_then(TagOrIdMatcher::from_value)
        .unwrap_or_default()
}

/// A text field, or an empty one.
pub(super) fn text_or_empty(val: &Value, key: &str) -> String {
    val.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// The entity type (or tag) a predicate names: `minecraft:entity_type`, else `type`.
fn type_matcher(val: &Value) -> Option<TagOrIdMatcher> {
    val.get("minecraft:entity_type")
        .or_else(|| val.get("type"))
        .and_then(TagOrIdMatcher::from_value)
}

/// The entity type of an entity predicate that spells its key `minecraft:entity_type` or
/// `entity_type`, as text.
fn entity_type_name(predicate: &Value) -> Option<&str> {
    predicate
        .get("minecraft:entity_type")
        .or_else(|| predicate.get("entity_type"))
        .and_then(|v| v.as_str())
}

/// The lower bound of `distance.<axis>.min`, with no upper bound.
pub(super) fn distance_min(val: &Value, axis: &str) -> Option<FloatBounds> {
    val.get("distance")
        .and_then(|d| d.get(axis))
        .and_then(|a| a.get("min"))
        .and_then(Value::as_f64)
        .map(|min| FloatBounds {
            min: Some(min),
            max: None,
        })
}

/// `player.predicate.<key>` where the key is `minecraft:<key>` or plain.
fn player_predicate_part<'a>(val: &'a Value, key: &str) -> Option<&'a Value> {
    let predicate = val.get("player").and_then(|p| p.get("predicate"))?;
    predicate
        .get(format!("minecraft:{key}"))
        .or_else(|| predicate.get(key))
}

/// The vehicle type the player is riding.
pub(super) fn vehicle_type(val: &Value) -> Option<TagOrIdMatcher> {
    player_predicate_part(val, "vehicle").and_then(type_matcher)
}

/// The cause entity of a fall after an explosion.
pub(super) fn cause_entity(val: &Value) -> Option<String> {
    val.get("cause")
        .and_then(|c| c.get("predicate"))
        .and_then(entity_type_name)
        .map(ToString::to_string)
}

pub(super) fn using_item(val: &Value) -> CriterionConditions {
    let looking_at = val
        .get("player")
        .and_then(|p| p.get("predicate"))
        .and_then(|pred| pred.get("minecraft:type_specific/player"))
        .and_then(|p| p.get("looking_at"))
        .and_then(type_matcher);
    CriterionConditions::UsingItem {
        item: super::parse_item_pred(val),
        looking_at,
    }
}

pub(super) fn ride_entity_in_lava(val: &Value) -> CriterionConditions {
    let dimension = player_predicate_part(val, "location")
        .and_then(|l| l.get("dimension"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    CriterionConditions::RideEntityInLava {
        distance_horizontal: distance_min(val, "horizontal"),
        dimension,
        vehicle_type: vehicle_type(val),
    }
}

pub(super) fn channeled_lightning(val: &Value) -> CriterionConditions {
    CriterionConditions::ChanneledLightning {
        victim_type: val
            .get("victims")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|e| e.get("predicate"))
            .and_then(entity_type_name)
            .map(ToString::to_string),
    }
}

pub(super) fn lightning_strike(val: &Value) -> CriterionConditions {
    let bystander_type = val
        .get("bystander")
        .and_then(|b| b.get("predicate"))
        .and_then(entity_type_name)
        .map(ToString::to_string);
    let lightning = val.get("lightning").and_then(|l| l.get("predicate"));
    let blocks_set_on_fire = lightning
        .and_then(|p| p.get("minecraft:type_specific/lightning"))
        .and_then(|t| t.get("blocks_set_on_fire"))
        .and_then(Value::as_i64)
        .map(|v| v as i32);
    let distance_max = lightning
        .and_then(|p| p.get("minecraft:distance"))
        .and_then(|d| d.get("absolute"))
        .and_then(|a| a.get("max"))
        .and_then(Value::as_f64);
    CriterionConditions::LightningStrike {
        bystander_type,
        blocks_set_on_fire,
        distance_max,
    }
}
