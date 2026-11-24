//! Characterisation of `Parser::condition`: one line per input document with the parsed condition
//! (Debug text) or `None`. Written by the code before the function was split; restructuring must
//! not change it. Rewrite it on purpose with
//! `KVASIR_BLESS=1 cargo test -p pumpkin-util --lib condition_golden`.

#![allow(clippy::too_many_lines)]

use std::fmt::Write;

use serde_json::{Value, json};

use super::parse_condition_with_predicates;

type Cases = &'static [(&'static str, &'static str)];

/// Named predicate definitions the resolver knows.
fn predicates(name: &str) -> Option<Value> {
    match name {
        "known" | "minecraft:known" => Some(json!({"condition": "killed_by_player"})),
        "bad" => Some(json!({"condition": "no_such_condition"})),
        "loop" => Some(json!("loop")),
        "ref_loop" => Some(json!({"condition": "reference", "name": "ref_loop"})),
        "arr" => Some(json!(["known", {"condition": "survives_explosion"}])),
        "arr_bad" => Some(json!(["known", "bad"])),
        "chain" => Some(json!("known")),
        _ => None,
    }
}

fn dispatch_cases() -> Cases {
    &[
        ("empty_object", "{}"),
        ("null", "null"),
        ("number", "5"),
        ("bool", "true"),
        ("string_unknown", r#""nothing""#),
        ("string_known", r#""known""#),
        ("string_known_prefixed", r#""minecraft:known""#),
        ("string_chain", r#""chain""#),
        ("string_bad", r#""bad""#),
        ("string_loop", r#""loop""#),
        ("string_arr", r#""arr""#),
        ("string_arr_bad", r#""arr_bad""#),
        ("array_empty", "[]"),
        ("array_one", r#"[{"condition":"killed_by_player"}]"#),
        ("array_two", r#"[{"condition":"killed_by_player"},"known"]"#),
        (
            "array_bad_element",
            r#"[{"condition":"killed_by_player"},{"condition":"nope"}]"#,
        ),
        ("array_string_unknown", r#"["nothing"]"#),
        (
            "array_nested",
            r#"[[{"condition":"survives_explosion"}],[]]"#,
        ),
        (
            "array_nested_one",
            r#"[[{"condition":"survives_explosion"}]]"#,
        ),
        ("kind_condition", r#"{"condition":"killed_by_player"}"#),
        ("kind_type", r#"{"type":"killed_by_player"}"#),
        ("kind_function", r#"{"function":"killed_by_player"}"#),
        (
            "kind_type_beats_condition",
            r#"{"type":"killed_by_player","condition":"survives_explosion"}"#,
        ),
        (
            "kind_condition_beats_function",
            r#"{"condition":"killed_by_player","function":"survives_explosion"}"#,
        ),
        (
            "kind_type_not_string",
            r#"{"type":5,"condition":"survives_explosion"}"#,
        ),
        (
            "kind_prefixed",
            r#"{"condition":"minecraft:killed_by_player"}"#,
        ),
        (
            "kind_double_prefix",
            r#"{"condition":"minecraft:minecraft:killed_by_player"}"#,
        ),
        (
            "kind_other_namespace",
            r#"{"condition":"kvasir:killed_by_player"}"#,
        ),
        ("kind_upper", r#"{"condition":"Killed_By_Player"}"#),
        (
            "kind_trailing_space",
            r#"{"condition":"killed_by_player "}"#,
        ),
        ("kind_unknown", r#"{"condition":"nope"}"#),
        ("kind_unknown_prefixed", r#"{"condition":"minecraft:nope"}"#),
        ("kind_only_prefix", r#"{"condition":"minecraft:"}"#),
        ("kind_empty", r#"{"condition":""}"#),
        ("kind_null", r#"{"condition":null}"#),
        ("kind_silk_touch_not_here", r#"{"condition":"silk_touch"}"#),
        ("kind_all_of_dashed", r#"{"condition":"all-of","terms":[]}"#),
        ("kind_alias_block", r#"{"condition":"match_block2"}"#),
        (
            "kind_entity_properties_suffix",
            r#"{"condition":"entity_properties_"}"#,
        ),
    ]
}

fn simple_cases() -> Cases {
    &[
        ("explosion", r#"{"condition":"survives_explosion"}"#),
        (
            "explosion_extra_fields",
            r#"{"condition":"survives_explosion","chance":0.5,"terms":[1]}"#,
        ),
        ("killed", r#"{"condition":"killed_by_player"}"#),
        (
            "killed_inverse_flag",
            r#"{"condition":"killed_by_player","inverse":true}"#,
        ),
        ("chance", r#"{"condition":"random_chance","chance":0.5}"#),
        ("chance_int", r#"{"condition":"random_chance","chance":1}"#),
        (
            "chance_tenth",
            r#"{"condition":"random_chance","chance":0.1}"#,
        ),
        ("chance_missing", r#"{"condition":"random_chance"}"#),
        (
            "chance_string",
            r#"{"condition":"random_chance","chance":"0.5"}"#,
        ),
        (
            "chance_object",
            r#"{"condition":"random_chance","chance":{"type":"constant","value":0.5}}"#,
        ),
        (
            "chance_null",
            r#"{"condition":"random_chance","chance":null}"#,
        ),
        (
            "chance_huge",
            r#"{"condition":"random_chance","chance":1e40}"#,
        ),
        (
            "chance_negative",
            r#"{"condition":"random_chance","chance":-0.25}"#,
        ),
    ]
}

fn bonus_cases() -> Cases {
    &[
        (
            "bonus_full",
            r#"{"condition":"random_chance_with_enchanted_bonus","enchantment":"minecraft:fortune","unenchanted_chance":0.1,"enchanted_chance":0.3}"#,
        ),
        (
            "bonus_empty",
            r#"{"condition":"random_chance_with_enchanted_bonus"}"#,
        ),
        (
            "bonus_unenchanted_only",
            r#"{"condition":"random_chance_with_enchanted_bonus","unenchanted_chance":0.2}"#,
        ),
        (
            "bonus_unenchanted_string",
            r#"{"condition":"random_chance_with_enchanted_bonus","unenchanted_chance":"0.2"}"#,
        ),
        (
            "bonus_enchanted_only",
            r#"{"condition":"random_chance_with_enchanted_bonus","enchanted_chance":0.4}"#,
        ),
        (
            "bonus_enchanted_string",
            r#"{"condition":"random_chance_with_enchanted_bonus","unenchanted_chance":0.2,"enchanted_chance":"0.4"}"#,
        ),
        (
            "bonus_enchanted_object",
            r#"{"condition":"random_chance_with_enchanted_bonus","unenchanted_chance":0.2,"enchanted_chance":{"base":0.5,"per_level_above_first":0.125}}"#,
        ),
        (
            "bonus_enchanted_object_base_only",
            r#"{"condition":"random_chance_with_enchanted_bonus","unenchanted_chance":0.2,"enchanted_chance":{"base":0.5}}"#,
        ),
        (
            "bonus_enchanted_object_per_level_only",
            r#"{"condition":"random_chance_with_enchanted_bonus","unenchanted_chance":0.2,"enchanted_chance":{"per_level_above_first":0.125}}"#,
        ),
        (
            "bonus_enchanted_object_empty",
            r#"{"condition":"random_chance_with_enchanted_bonus","unenchanted_chance":0.2,"enchanted_chance":{}}"#,
        ),
        (
            "bonus_enchanted_object_base_string",
            r#"{"condition":"random_chance_with_enchanted_bonus","unenchanted_chance":0.2,"enchanted_chance":{"base":"0.5","per_level_above_first":"1"}}"#,
        ),
        (
            "bonus_enchanted_per_level_on_number",
            r#"{"condition":"random_chance_with_enchanted_bonus","unenchanted_chance":0.2,"enchanted_chance":0.75,"per_level_above_first":0.5}"#,
        ),
        (
            "bonus_enchanted_array",
            r#"{"condition":"random_chance_with_enchanted_bonus","unenchanted_chance":0.2,"enchanted_chance":[0.5]}"#,
        ),
        (
            "bonus_enchantment_named",
            r#"{"condition":"random_chance_with_enchanted_bonus","enchantment":"minecraft:sharpness","unenchanted_chance":0.5,"enchanted_chance":1}"#,
        ),
        (
            "bonus_enchantment_not_string",
            r#"{"condition":"random_chance_with_enchanted_bonus","enchantment":7,"unenchanted_chance":0.5}"#,
        ),
        (
            "looting_full",
            r#"{"condition":"random_chance_with_looting","chance":0.25,"looting_multiplier":0.0625}"#,
        ),
        (
            "looting_empty",
            r#"{"condition":"random_chance_with_looting"}"#,
        ),
        (
            "looting_chance_only",
            r#"{"condition":"random_chance_with_looting","chance":0.25}"#,
        ),
        (
            "looting_multiplier_only",
            r#"{"condition":"random_chance_with_looting","looting_multiplier":0.5}"#,
        ),
        (
            "looting_strings",
            r#"{"condition":"random_chance_with_looting","chance":"1","looting_multiplier":"2"}"#,
        ),
        (
            "table_full",
            r#"{"condition":"table_bonus","enchantment":"minecraft:looting","chances":[0.1,0.25,0.5]}"#,
        ),
        ("table_empty", r#"{"condition":"table_bonus"}"#),
        (
            "table_enchantment_not_string",
            r#"{"condition":"table_bonus","enchantment":3,"chances":[0.5]}"#,
        ),
        (
            "table_chances_empty",
            r#"{"condition":"table_bonus","chances":[]}"#,
        ),
        (
            "table_chances_mixed",
            r#"{"condition":"table_bonus","chances":[0.5,"x",null,1,[2],0.75]}"#,
        ),
        (
            "table_chances_not_array",
            r#"{"condition":"table_bonus","chances":0.5}"#,
        ),
        (
            "table_chances_object",
            r#"{"condition":"table_bonus","chances":{"a":0.5}}"#,
        ),
    ]
}

fn composite_cases() -> Cases {
    &[
        (
            "all_two",
            r#"{"condition":"all_of","terms":[{"condition":"killed_by_player"},"known"]}"#,
        ),
        (
            "all_one",
            r#"{"condition":"all_of","terms":[{"condition":"killed_by_player"}]}"#,
        ),
        ("all_empty", r#"{"condition":"all_of","terms":[]}"#),
        ("all_missing_terms", r#"{"condition":"all_of"}"#),
        ("all_terms_object", r#"{"condition":"all_of","terms":{}}"#),
        (
            "all_terms_string",
            r#"{"condition":"all_of","terms":"known"}"#,
        ),
        (
            "all_bad_term",
            r#"{"condition":"all_of","terms":[{"condition":"killed_by_player"},{"condition":"nope"}]}"#,
        ),
        (
            "all_nested",
            r#"{"condition":"all_of","terms":[{"condition":"all_of","terms":[{"condition":"killed_by_player"},{"condition":"survives_explosion"}]},{"condition":"survives_explosion"}]}"#,
        ),
        (
            "all_term_empty_all",
            r#"{"condition":"all_of","terms":[{"condition":"all_of","terms":[]},{"condition":"killed_by_player"}]}"#,
        ),
        (
            "any_two",
            r#"{"condition":"any_of","terms":[{"condition":"killed_by_player"},"known"]}"#,
        ),
        (
            "any_one",
            r#"{"condition":"any_of","terms":[{"condition":"killed_by_player"}]}"#,
        ),
        ("any_empty", r#"{"condition":"any_of","terms":[]}"#),
        ("any_missing_terms", r#"{"condition":"any_of"}"#),
        ("any_terms_object", r#"{"condition":"any_of","terms":{}}"#),
        (
            "any_terms_string",
            r#"{"condition":"any_of","terms":"known"}"#,
        ),
        (
            "any_bad_term",
            r#"{"condition":"any_of","terms":[{"condition":"killed_by_player"},{"condition":"nope"}]}"#,
        ),
        (
            "any_nested",
            r#"{"condition":"any_of","terms":[{"condition":"any_of","terms":[{"condition":"killed_by_player"}]},{"condition":"all_of","terms":[{"condition":"survives_explosion"}]}]}"#,
        ),
        (
            "inverted",
            r#"{"condition":"inverted","term":{"condition":"killed_by_player"}}"#,
        ),
        (
            "inverted_string_term",
            r#"{"condition":"inverted","term":"known"}"#,
        ),
        (
            "inverted_array_term",
            r#"{"condition":"inverted","term":[{"condition":"killed_by_player"},{"condition":"survives_explosion"}]}"#,
        ),
        ("inverted_missing_term", r#"{"condition":"inverted"}"#),
        (
            "inverted_bad_term",
            r#"{"condition":"inverted","term":{"condition":"nope"}}"#,
        ),
        (
            "inverted_terms_plural",
            r#"{"condition":"inverted","terms":[{"condition":"killed_by_player"}]}"#,
        ),
        (
            "inverted_twice",
            r#"{"condition":"inverted","term":{"condition":"inverted","term":{"condition":"survives_explosion"}}}"#,
        ),
    ]
}

fn entity_cases() -> Cases {
    &[
        (
            "entity_this",
            r#"{"condition":"entity_properties","entity":"this","predicate":{"type":"minecraft:zombie"}}"#,
        ),
        (
            "entity_attacker",
            r#"{"condition":"entity_properties","entity":"attacker","predicate":{"type":"minecraft:zombie"}}"#,
        ),
        (
            "entity_killer",
            r#"{"condition":"entity_properties","entity":"killer","predicate":{"type":"minecraft:zombie"}}"#,
        ),
        (
            "entity_direct_attacker",
            r#"{"condition":"entity_properties","entity":"direct_attacker","predicate":{}}"#,
        ),
        (
            "entity_direct_killer",
            r#"{"condition":"entity_properties","entity":"direct_killer","predicate":{"type":"minecraft:arrow"}}"#,
        ),
        (
            "entity_missing",
            r#"{"condition":"entity_properties","predicate":{}}"#,
        ),
        (
            "entity_unknown",
            r#"{"condition":"entity_properties","entity":"killer_entity","predicate":{}}"#,
        ),
        (
            "entity_not_string",
            r#"{"condition":"entity_properties","entity":1,"predicate":{}}"#,
        ),
        (
            "entity_predicate_missing",
            r#"{"condition":"entity_properties","entity":"this"}"#,
        ),
        (
            "entity_predicate_not_object",
            r#"{"condition":"entity_properties","entity":"this","predicate":[]}"#,
        ),
        (
            "entity_this_empty_predicate",
            r#"{"condition":"entity_properties","entity":"this","predicate":{}}"#,
        ),
        (
            "entity_on_fire",
            r#"{"condition":"entity_properties","entity":"this","predicate":{"flags":{"is_on_fire":true}}}"#,
        ),
        (
            "entity_on_fire_prefixed",
            r#"{"condition":"entity_properties","entity":"this","predicate":{"minecraft:flags":{"is_on_fire":true}}}"#,
        ),
        (
            "entity_on_fire_false",
            r#"{"condition":"entity_properties","entity":"this","predicate":{"flags":{"is_on_fire":false}}}"#,
        ),
        (
            "entity_on_fire_and_more",
            r#"{"condition":"entity_properties","entity":"this","predicate":{"flags":{"is_on_fire":true,"is_baby":false},"type":"minecraft:cow"}}"#,
        ),
        (
            "entity_on_fire_both_keys",
            r#"{"condition":"entity_properties","entity":"this","predicate":{"flags":{"is_on_fire":true},"minecraft:flags":{"is_on_fire":true}}}"#,
        ),
        (
            "entity_on_fire_wrong_target",
            r#"{"condition":"entity_properties","entity":"attacker","predicate":{"flags":{"is_on_fire":true}}}"#,
        ),
        (
            "entity_weapon_enchantment",
            r##"{"condition":"entity_properties","entity":"direct_attacker","predicate":{"equipment":{"mainhand":{"predicates":{"minecraft:enchantments":[{"enchantments":"#minecraft:sword_enchants"}]}}}}}"##,
        ),
        (
            "entity_weapon_enchantment_prefixed",
            r##"{"condition":"entity_properties","entity":"direct_killer","predicate":{"minecraft:equipment":{"mainhand":{"predicates":{"minecraft:enchantments":[{"enchantments":"#minecraft:fire"}]}}}}}"##,
        ),
        (
            "entity_weapon_enchantment_no_hash",
            r#"{"condition":"entity_properties","entity":"direct_attacker","predicate":{"equipment":{"mainhand":{"predicates":{"minecraft:enchantments":[{"enchantments":"minecraft:fire"}]}}}}}"#,
        ),
        (
            "entity_weapon_enchantment_wrong_target",
            r##"{"condition":"entity_properties","entity":"this","predicate":{"equipment":{"mainhand":{"predicates":{"minecraft:enchantments":[{"enchantments":"#minecraft:fire"}]}}}}}"##,
        ),
        (
            "entity_weapon_enchantment_and_type",
            r##"{"condition":"entity_properties","entity":"direct_attacker","predicate":{"type":"minecraft:arrow","equipment":{"mainhand":{"predicates":{"minecraft:enchantments":[{"enchantments":"#minecraft:fire"}]}}}}}"##,
        ),
    ]
}

fn world_cases() -> Cases {
    &[
        (
            "damage_no_predicate",
            r#"{"condition":"damage_source_properties"}"#,
        ),
        (
            "damage_empty_predicate",
            r#"{"condition":"damage_source_properties","predicate":{}}"#,
        ),
        (
            "damage_source_entity",
            r#"{"condition":"damage_source_properties","predicate":{"source_entity":{"type":"minecraft:player"}}}"#,
        ),
        (
            "damage_both_entities",
            r#"{"condition":"damage_source_properties","predicate":{"source_entity":{"type":"minecraft:player"},"direct_entity":{"type":"minecraft:arrow"}}}"#,
        ),
        (
            "damage_tags",
            r##"{"condition":"damage_source_properties","predicate":{"tags":[{"id":"#minecraft:is_fire","expected":false},{"id":"minecraft:is_fall"},{"expected":true},{"id":5}]}}"##,
        ),
        (
            "damage_tags_not_array",
            r#"{"condition":"damage_source_properties","predicate":{"tags":{"id":"minecraft:is_fire"}}}"#,
        ),
        (
            "damage_flags",
            r#"{"condition":"damage_source_properties","predicate":{"is_projectile":true,"is_explosion":false,"is_fire":true,"is_lightning":false,"is_fall":true}}"#,
        ),
        (
            "damage_flags_wrong_type",
            r#"{"condition":"damage_source_properties","predicate":{"is_projectile":"true","is_fire":1}}"#,
        ),
        (
            "damage_everything",
            r#"{"condition":"damage_source_properties","predicate":{"direct_entity":{},"tags":[{"id":"minecraft:is_fire"}],"is_lightning":true}}"#,
        ),
        (
            "location_full",
            r#"{"condition":"location_check","offsetX":1,"offsetY":-2,"offsetZ":3,"predicate":{"biomes":"minecraft:plains"}}"#,
        ),
        ("location_empty", r#"{"condition":"location_check"}"#),
        (
            "location_offsets_only_x",
            r#"{"condition":"location_check","offsetX":7}"#,
        ),
        (
            "location_offsets_only_y",
            r#"{"condition":"location_check","offsetY":8}"#,
        ),
        (
            "location_offsets_only_z",
            r#"{"condition":"location_check","offsetZ":9}"#,
        ),
        (
            "location_offset_string",
            r#"{"condition":"location_check","offsetX":"1"}"#,
        ),
        (
            "location_offset_float",
            r#"{"condition":"location_check","offsetY":1.5}"#,
        ),
        (
            "location_offset_too_big",
            r#"{"condition":"location_check","offsetZ":4294967296}"#,
        ),
        (
            "location_offset_null",
            r#"{"condition":"location_check","offsetX":null}"#,
        ),
        (
            "location_offset_bad_z_only",
            r#"{"condition":"location_check","offsetX":1,"offsetY":2,"offsetZ":"x"}"#,
        ),
        (
            "location_predicate_only",
            r#"{"condition":"location_check","predicate":{"block":{"blocks":"minecraft:stone"}}}"#,
        ),
        (
            "location_snake_offsets",
            r#"{"condition":"location_check","offset_x":1}"#,
        ),
    ]
}

fn predicate_cases() -> Cases {
    &[
        (
            "block_predicate",
            r#"{"condition":"match_block","predicate":{"blocks":"minecraft:stone"}}"#,
        ),
        (
            "block_no_predicate",
            r#"{"condition":"match_block","block":"minecraft:stone"}"#,
        ),
        (
            "block_state_predicate",
            r#"{"condition":"block_state_property","predicate":{"properties":{"age":"3"}}}"#,
        ),
        (
            "block_state_whole_value",
            r#"{"condition":"block_state_property","block":"minecraft:wheat","properties":{"age":"7"}}"#,
        ),
        (
            "block_predicate_null",
            r#"{"condition":"match_block","predicate":null}"#,
        ),
        ("block_empty", r#"{"condition":"match_block"}"#),
        (
            "tool_predicate",
            r#"{"condition":"match_tool","predicate":{"items":"minecraft:shears"}}"#,
        ),
        ("tool_empty", r#"{"condition":"match_tool"}"#),
        (
            "tool_whole_value_ignored",
            r#"{"condition":"match_tool","items":"minecraft:shears"}"#,
        ),
        (
            "tool_predicate_null",
            r#"{"condition":"match_tool","predicate":null}"#,
        ),
        (
            "weather_both",
            r#"{"condition":"weather_check","raining":true,"thundering":false}"#,
        ),
        ("weather_empty", r#"{"condition":"weather_check"}"#),
        (
            "weather_raining_only",
            r#"{"condition":"weather_check","raining":false}"#,
        ),
        (
            "weather_thundering_only",
            r#"{"condition":"weather_check","thundering":true}"#,
        ),
        (
            "weather_wrong_types",
            r#"{"condition":"weather_check","raining":"yes","thundering":1}"#,
        ),
        (
            "time_full",
            r#"{"condition":"time_check","value":{"min":0,"max":12000},"period":24000}"#,
        ),
        ("time_empty", r#"{"condition":"time_check"}"#),
        ("time_value_only", r#"{"condition":"time_check","value":5}"#),
        (
            "time_period_only",
            r#"{"condition":"time_check","period":100}"#,
        ),
        (
            "time_period_negative",
            r#"{"condition":"time_check","value":1,"period":-5}"#,
        ),
        (
            "time_period_float",
            r#"{"condition":"time_check","value":1,"period":1.5}"#,
        ),
        (
            "time_period_string",
            r#"{"condition":"time_check","value":1,"period":"5"}"#,
        ),
        (
            "time_value_null",
            r#"{"condition":"time_check","value":null,"period":3}"#,
        ),
    ]
}

fn value_cases() -> Cases {
    &[
        (
            "value_constant",
            r#"{"condition":"value_check","value":5,"range":{"min":1,"max":9}}"#,
        ),
        (
            "value_constant_object",
            r#"{"condition":"value_check","value":{"value":2.5},"range":3}"#,
        ),
        (
            "value_uniform",
            r#"{"condition":"value_check","value":{"type":"minecraft:uniform","min":1,"max":3},"range":1}"#,
        ),
        (
            "value_uniform_untyped",
            r#"{"condition":"value_check","value":{"min":1,"max":3},"range":1}"#,
        ),
        (
            "value_uniform_min_object",
            r#"{"condition":"value_check","value":{"type":"uniform","min":{"value":2},"max":4},"range":1}"#,
        ),
        (
            "value_uniform_missing_max",
            r#"{"condition":"value_check","value":{"type":"uniform","min":1},"range":1}"#,
        ),
        (
            "value_uniform_missing_min",
            r#"{"condition":"value_check","value":{"type":"uniform","max":1},"range":1}"#,
        ),
        (
            "value_binomial",
            r#"{"condition":"value_check","value":{"type":"minecraft:binomial","n":10,"p":0.5},"range":2}"#,
        ),
        (
            "value_binomial_float_n",
            r#"{"condition":"value_check","value":{"type":"binomial","n":7.9,"p":{"value":0.25}},"range":2}"#,
        ),
        (
            "value_binomial_missing_p",
            r#"{"condition":"value_check","value":{"type":"binomial","n":10},"range":2}"#,
        ),
        (
            "value_binomial_missing_n",
            r#"{"condition":"value_check","value":{"type":"binomial","p":0.5},"range":2}"#,
        ),
        (
            "value_unknown_provider",
            r#"{"condition":"value_check","value":{"type":"minecraft:score","name":"x"},"range":2}"#,
        ),
        (
            "value_missing",
            r#"{"condition":"value_check","range":{"min":1}}"#,
        ),
        (
            "value_string",
            r#"{"condition":"value_check","value":"5","range":1}"#,
        ),
        ("value_no_range", r#"{"condition":"value_check","value":5}"#),
        (
            "value_range_null",
            r#"{"condition":"value_check","value":5,"range":null}"#,
        ),
        ("value_empty", r#"{"condition":"value_check"}"#),
    ]
}

fn reference_cases() -> Cases {
    &[
        ("ref_known", r#"{"condition":"reference","name":"known"}"#),
        (
            "ref_known_prefixed",
            r#"{"condition":"reference","name":"minecraft:known"}"#,
        ),
        (
            "ref_unknown",
            r#"{"condition":"reference","name":"mystery"}"#,
        ),
        ("ref_bad", r#"{"condition":"reference","name":"bad"}"#),
        ("ref_chain", r#"{"condition":"reference","name":"chain"}"#),
        ("ref_loop", r#"{"condition":"reference","name":"loop"}"#),
        (
            "ref_self_loop",
            r#"{"condition":"reference","name":"ref_loop"}"#,
        ),
        ("ref_array", r#"{"condition":"reference","name":"arr"}"#),
        (
            "ref_array_bad",
            r#"{"condition":"reference","name":"arr_bad"}"#,
        ),
        ("ref_missing_name", r#"{"condition":"reference"}"#),
        (
            "ref_name_not_string",
            r#"{"condition":"reference","name":5}"#,
        ),
        (
            "ref_name_key_only",
            r#"{"condition":"reference","predicate":"known"}"#,
        ),
        (
            "ref_inside_inverted",
            r#"{"condition":"inverted","term":{"condition":"reference","name":"mystery"}}"#,
        ),
        (
            "ref_inside_all",
            r#"{"condition":"all_of","terms":[{"condition":"reference","name":"known"},{"condition":"reference","name":"mystery"}]}"#,
        ),
    ]
}

fn render_case(label: &str, text: &str) -> String {
    let result = serde_json::from_str::<Value>(text).map_or_else(
        |_| "invalid json".to_owned(),
        |value| format!("{:?}", parse_condition_with_predicates(&value, &predicates)),
    );
    format!("{label} | {text} => {result}\n")
}

/// Documents nested `levels` deep around a leaf; the depth limit decides if they parse.
fn depth_section(out: &mut String) {
    for levels in 126..130 {
        let mut doc = json!({"condition": "survives_explosion"});
        for _ in 0..levels {
            doc = json!({"condition": "inverted", "term": doc});
        }
        let result = parse_condition_with_predicates(&doc, &predicates);
        let text = format!("{result:?}");
        let _ = writeln!(
            out,
            "inverted x{levels} => {} (inverted {}x)",
            if result.is_some() { "Some" } else { "None" },
            text.matches("Inverted(").count()
        );
    }
    for levels in 126..130 {
        let mut doc = json!({"condition": "survives_explosion"});
        for _ in 0..levels {
            doc = json!([doc]);
        }
        let result = parse_condition_with_predicates(&doc, &predicates);
        let _ = writeln!(
            out,
            "array x{levels} => {}",
            if result.is_some() { "Some" } else { "None" }
        );
    }
    for levels in 126..130 {
        let mut doc = json!("known");
        for _ in 0..levels {
            doc = json!({"condition": "all_of", "terms": [doc]});
        }
        let result = parse_condition_with_predicates(&doc, &predicates);
        let _ = writeln!(
            out,
            "all_of x{levels} => {}",
            if result.is_some() { "Some" } else { "None" }
        );
    }
    for levels in 126..130 {
        let mut doc = json!({"condition": "killed_by_player"});
        for _ in 0..levels {
            doc = json!({"condition": "any_of", "terms": [doc]});
        }
        let result = parse_condition_with_predicates(&doc, &predicates);
        let text = format!("{result:?}");
        let _ = writeln!(
            out,
            "any_of x{levels} => {} (any_of {}x)",
            if result.is_some() { "Some" } else { "None" },
            text.matches("AnyOf(").count()
        );
    }
}

/// A reference leaf `levels` deep: resolving it costs one more level, so the limit shows in the
/// result (a resolved condition, or the unresolved `Reference` once the limit is hit).
fn reference_depth_section(out: &mut String) {
    for levels in 124..130 {
        let mut doc = json!({"condition": "reference", "name": "known"});
        for _ in 0..levels {
            doc = json!({"condition": "all_of", "terms": [doc]});
        }
        let _ = writeln!(
            out,
            "reference x{levels} => {:?}",
            parse_condition_with_predicates(&doc, &predicates)
        );
    }
}

fn render() -> String {
    let mut out = String::new();
    for cases in [
        dispatch_cases(),
        simple_cases(),
        bonus_cases(),
        composite_cases(),
        entity_cases(),
        world_cases(),
        predicate_cases(),
        value_cases(),
        reference_cases(),
    ] {
        for (label, text) in cases {
            out.push_str(&render_case(label, text));
        }
    }
    depth_section(&mut out);
    reference_depth_section(&mut out);
    out
}

#[test]
fn conditions_parse_as_before() -> Result<(), std::io::Error> {
    let now = render();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/loot_condition_parse.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    let golden = std::fs::read_to_string(&path)?;
    let differing: Vec<(&str, &str)> = golden
        .lines()
        .zip(now.lines())
        .filter(|(a, b)| a != b)
        .collect();
    assert_eq!(
        golden.lines().count(),
        now.lines().count(),
        "number of cases"
    );
    assert!(
        differing.is_empty(),
        "{} lines differ, first: {:?}",
        differing.len(),
        differing.first()
    );
    Ok(())
}
