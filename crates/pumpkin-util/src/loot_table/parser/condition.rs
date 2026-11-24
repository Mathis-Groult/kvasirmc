//! Arms of `Parser::condition` that read fields of one condition document.

use serde_json::Value;

use super::{Condition, Parser, all, provider};

fn float(value: &Value, key: &str) -> f32 {
    value.get(key).and_then(Value::as_f64).unwrap_or(0.0) as f32
}

fn string_or(value: &Value, key: &str, default: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or(default)
        .to_owned()
}

pub(super) fn random_chance(value: &Value) -> Condition {
    Condition::RandomChance {
        chance: float(value, "chance"),
    }
}

pub(super) fn random_chance_with_enchanted_bonus(value: &Value) -> Condition {
    let unenchanted_chance = float(value, "unenchanted_chance");
    let chance = value.get("enchanted_chance");
    let enchanted_chance_base = chance
        .and_then(|chance| {
            chance
                .as_f64()
                .or_else(|| chance.get("base").and_then(Value::as_f64))
        })
        .map_or(unenchanted_chance, |number| number as f32);
    let enchanted_chance_per_level_above_first = chance
        .and_then(|chance| chance.get("per_level_above_first"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as f32;
    Condition::RandomChanceWithEnchantedBonus {
        enchantment: string_or(value, "enchantment", "minecraft:looting"),
        unenchanted_chance,
        enchanted_chance_base,
        enchanted_chance_per_level_above_first,
    }
}

pub(super) fn random_chance_with_looting(value: &Value) -> Condition {
    Condition::RandomChanceWithLooting {
        chance: float(value, "chance"),
        looting_multiplier: float(value, "looting_multiplier"),
    }
}

pub(super) fn table_bonus(value: &Value) -> Condition {
    Condition::TableBonus {
        enchantment: string_or(value, "enchantment", "minecraft:fortune"),
        chances: value
            .get("chances")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_f64)
                    .map(|value| value as f32)
                    .collect()
            })
            .unwrap_or_default(),
    }
}

pub(super) fn match_tool(value: &Value) -> Condition {
    Condition::MatchTool(
        value
            .get("predicate")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    )
}

pub(super) fn weather_check(value: &Value) -> Condition {
    Condition::WeatherCheck {
        raining: value.get("raining").and_then(Value::as_bool),
        thundering: value.get("thundering").and_then(Value::as_bool),
    }
}

pub(super) fn time_check(value: &Value) -> Condition {
    Condition::TimeCheck {
        value: value.get("value").cloned().unwrap_or(Value::Null),
        period: value.get("period").and_then(Value::as_u64),
    }
}

pub(super) fn value_check(value: &Value) -> Option<Condition> {
    Some(Condition::ValueCheck {
        value: value.get("value").and_then(provider)?,
        range: value.get("range").cloned().unwrap_or(Value::Null),
    })
}

impl<R: Fn(&str) -> Option<Value>> Parser<'_, R> {
    /// The `terms` of `all_of` and `any_of`, each parsed one level deeper.
    pub(super) fn terms(&self, value: &Value, depth: usize) -> Option<Vec<Condition>> {
        value
            .get("terms")?
            .as_array()?
            .iter()
            .map(|term| self.condition(term, depth + 1))
            .collect()
    }

    pub(super) fn all_of(&self, value: &Value, depth: usize) -> Option<Condition> {
        Some(all(self.terms(value, depth)?))
    }

    pub(super) fn any_of(&self, value: &Value, depth: usize) -> Option<Condition> {
        Some(Condition::AnyOf(self.terms(value, depth)?))
    }

    pub(super) fn inverted(&self, value: &Value, depth: usize) -> Option<Condition> {
        Some(Condition::Inverted(Box::new(
            self.condition(value.get("term")?, depth + 1)?,
        )))
    }

    /// A named predicate, or `Reference` when the resolver cannot expand it.
    pub(super) fn reference(&self, value: &Value, depth: usize) -> Option<Condition> {
        let name = value.get("name").and_then(Value::as_str)?;
        Some(
            (self.predicates)(name)
                .and_then(|value| self.condition(&value, depth + 1))
                .unwrap_or_else(|| Condition::Reference(name.to_owned())),
        )
    }
}
