//! A record of what `CriterionConditions::parse` (the datapack path: a trigger id and the
//! condition JSON as text) makes of every trigger id, made before the function was split
//! (`tests/golden/advancement_parse.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Every trigger is given every document of a corpus that holds each key path some arm reads
//! (both spellings where an arm accepts two, once alone and once together, text where a number
//! is read, and no JSON at all). A trigger line lists, for each distinct result, the documents
//! that gave it, so an arm that ignores a key or reads the wrong one shows as a changed group.

use std::fmt::Write;

use crate::entity::player::advancement::predicate::CriterionConditions;

const TRIGGERS: &[&str] = &[
    "minecraft:inventory_changed",
    "minecraft:location",
    "minecraft:player_killed_entity",
    "minecraft:killed_by_arrow",
    "minecraft:changed_dimension",
    "minecraft:slept_in_bed",
    "minecraft:consume_item",
    "minecraft:placed_block",
    "minecraft:enter_block",
    "minecraft:slide_down_block",
    "minecraft:fishing_rod_hooked",
    "minecraft:filled_bucket",
    "minecraft:using_item",
    "minecraft:used_totem",
    "minecraft:entity_hurt_player",
    "minecraft:entity_killed_player",
    "minecraft:player_hurt_entity",
    "minecraft:bred_animals",
    "minecraft:used_ender_eye",
    "minecraft:item_used_on_block",
    "minecraft:target_hit",
    "minecraft:cured_zombie_villager",
    "minecraft:villager_trade",
    "minecraft:allay_drop_item_on_block",
    "minecraft:thrown_item_picked_up_by_player",
    "minecraft:tame_animal",
    "minecraft:construct_beacon",
    "minecraft:recipe_crafted",
    "minecraft:recipe_unlocked",
    "minecraft:player_generates_container_loot",
    "minecraft:enchanted_item",
    "minecraft:brewed_potion",
    "minecraft:item_durability_changed",
    "minecraft:levitation",
    "minecraft:fall_from_height",
    "minecraft:fall_after_explosion",
    "minecraft:nether_travel",
    "minecraft:ride_entity_in_lava",
    "minecraft:shot_crossbow",
    "minecraft:avoid_vibration",
    "minecraft:bee_nest_destroyed",
    "minecraft:channeled_lightning",
    "minecraft:lightning_strike",
    "minecraft:kill_mob_near_sculk_catalyst",
    "minecraft:hero_of_the_village",
    "minecraft:player_interacted_with_entity",
    "minecraft:thrown_item_picked_up_by_entity",
    "minecraft:player_sheared_equipment",
    "minecraft:started_riding",
    "minecraft:summoned_entity",
    "minecraft:crafter_recipe_crafted",
    "minecraft:effects_changed",
    "minecraft:spear_mobs",
    "minecraft:impossible",
    "minecraft:tick",
    "minecraft:no_such_trigger",
    "custom:pack/trigger",
    "",
    "MINECRAFT:TICK",
];

const DOCUMENTS: &[(&str, &str)] = &[
    ("empty", r"{}"),
    ("bad", r"{not json"),
    ("null", r"null"),
    ("array", r"[1]"),
    ("string", r#""text""#),
    ("blocks", r#"{"blocks":"minecraft:stone"}"#),
    ("block", r#"{"block":"minecraft:dirt"}"#),
    (
        "blocks_and_block",
        r#"{"blocks":"minecraft:stone","block":"minecraft:dirt"}"#,
    ),
    ("blocks_tag", r##"{"blocks":"#minecraft:logs"}"##),
    (
        "blocks_list",
        r#"{"blocks":["minecraft:stone","minecraft:dirt"]}"#,
    ),
    (
        "bees",
        r#"{"blocks":"minecraft:bee_nest","num_bees_inside":3}"#,
    ),
    (
        "bees_text",
        r#"{"block":"minecraft:beehive","num_bees_inside":"many"}"#,
    ),
    ("bees_big", r#"{"num_bees_inside":3000000000}"#),
    (
        "item_top",
        r#"{"items":"minecraft:diamond","count":{"min":2,"max":9}}"#,
    ),
    ("item_inner", r#"{"item":{"items":"minecraft:stick"}}"#),
    (
        "item_both",
        r#"{"items":"minecraft:diamond","item":{"items":"minecraft:stick","count":{"min":4}}}"#,
    ),
    ("entity_top", r#"{"type":"minecraft:zombie"}"#),
    (
        "entity_predicate",
        r#"{"predicate":{"type":"minecraft:pig"}}"#,
    ),
    (
        "entity_nested",
        r#"{"entity":{"predicate":{"type":"minecraft:cow"}}}"#,
    ),
    ("entity_plain", r#"{"entity":{"type":"minecraft:sheep"}}"#),
    (
        "entity_text",
        r#"{"entity":"minecraft:sheep","type":"minecraft:bat"}"#,
    ),
    (
        "entity_both",
        r#"{"predicate":{"type":"minecraft:pig"},"entity":{"type":"minecraft:sheep"},"type":"minecraft:bat"}"#,
    ),
    (
        "interaction",
        r#"{"entity":{"type":"minecraft:sheep"},"item":{"items":"minecraft:shears"},"player":{"type":"minecraft:player"}}"#,
    ),
    (
        "interaction_nested",
        r#"{"entity":{"predicate":{"type":"minecraft:cow"}},"item":{"item":{"items":"minecraft:bucket"}},"player":{"predicate":{"type":"minecraft:player"}}}"#,
    ),
    ("dist_y", r#"{"distance":{"y":{"min":5.5}}}"#),
    ("dist_y_int", r#"{"distance":{"y":{"min":-3}}}"#),
    ("dist_y_max", r#"{"distance":{"y":{"max":9.0}}}"#),
    ("dist_y_both", r#"{"distance":{"y":{"min":2.5,"max":9.0}}}"#),
    ("dist_y_text", r#"{"distance":{"y":{"min":"far"}}}"#),
    ("dist_h", r#"{"distance":{"horizontal":{"min":7.25}}}"#),
    (
        "dist_hy",
        r#"{"distance":{"horizontal":{"min":7.25},"y":{"min":1.5}}}"#,
    ),
    (
        "cause_prefixed",
        r#"{"cause":{"predicate":{"minecraft:entity_type":"minecraft:creeper"}}}"#,
    ),
    (
        "cause_short",
        r#"{"cause":{"predicate":{"entity_type":"minecraft:ghast"}}}"#,
    ),
    (
        "cause_both",
        r#"{"cause":{"predicate":{"minecraft:entity_type":"minecraft:creeper","entity_type":"minecraft:ghast"}},"distance":{"y":{"min":4.0}}}"#,
    ),
    (
        "vehicle_prefixed",
        r#"{"player":{"predicate":{"minecraft:vehicle":{"minecraft:entity_type":"minecraft:strider"}}}}"#,
    ),
    (
        "vehicle_short",
        r#"{"player":{"predicate":{"vehicle":{"type":"minecraft:boat"}}}}"#,
    ),
    (
        "vehicle_both",
        r#"{"player":{"predicate":{"minecraft:vehicle":{"type":"minecraft:strider"},"vehicle":{"type":"minecraft:boat"}}}}"#,
    ),
    (
        "vehicle_tag",
        r##"{"player":{"predicate":{"minecraft:vehicle":{"minecraft:entity_type":"#minecraft:boats"}}}}"##,
    ),
    (
        "dimension_prefixed",
        r#"{"player":{"predicate":{"minecraft:location":{"dimension":"minecraft:the_nether"}}}}"#,
    ),
    (
        "dimension_short",
        r#"{"player":{"predicate":{"location":{"dimension":"minecraft:overworld"}}}}"#,
    ),
    (
        "lava_all",
        r#"{"distance":{"horizontal":{"min":50.0}},"player":{"predicate":{"minecraft:location":{"dimension":"minecraft:the_nether"},"minecraft:vehicle":{"minecraft:entity_type":"minecraft:strider"}}}}"#,
    ),
    (
        "looking_prefixed",
        r#"{"items":"minecraft:bow","player":{"predicate":{"minecraft:type_specific/player":{"looking_at":{"minecraft:entity_type":"minecraft:enderman"}}}}}"#,
    ),
    (
        "looking_short",
        r#"{"player":{"predicate":{"minecraft:type_specific/player":{"looking_at":{"type":"minecraft:creeper"}}}}}"#,
    ),
    (
        "looking_both",
        r#"{"player":{"predicate":{"minecraft:type_specific/player":{"looking_at":{"minecraft:entity_type":"minecraft:enderman","type":"minecraft:creeper"}}}}}"#,
    ),
    (
        "victims_prefixed",
        r#"{"victims":[{"predicate":{"minecraft:entity_type":"minecraft:cow"}},{"predicate":{"minecraft:entity_type":"minecraft:pig"}}]}"#,
    ),
    (
        "victims_short",
        r#"{"victims":[{"predicate":{"entity_type":"minecraft:pig"}}]}"#,
    ),
    (
        "victims_both",
        r#"{"victims":[{"predicate":{"minecraft:entity_type":"minecraft:cow","entity_type":"minecraft:pig"}}]}"#,
    ),
    ("victims_empty", r#"{"victims":[]}"#),
    (
        "bystander_prefixed",
        r#"{"bystander":{"predicate":{"minecraft:entity_type":"minecraft:villager"}}}"#,
    ),
    (
        "bystander_short",
        r#"{"bystander":{"predicate":{"entity_type":"minecraft:witch"}}}"#,
    ),
    (
        "lightning_fire",
        r#"{"lightning":{"predicate":{"minecraft:type_specific/lightning":{"blocks_set_on_fire":4}}}}"#,
    ),
    (
        "lightning_distance",
        r#"{"lightning":{"predicate":{"minecraft:distance":{"absolute":{"max":12.5}}}}}"#,
    ),
    (
        "lightning_all",
        r#"{"bystander":{"predicate":{"minecraft:entity_type":"minecraft:villager"}},"lightning":{"predicate":{"minecraft:type_specific/lightning":{"blocks_set_on_fire":4},"minecraft:distance":{"absolute":{"max":12.5}}}}}"#,
    ),
    ("loot", r#"{"loot_tables":"minecraft:chests/ancient_city"}"#),
    ("loot_number", r#"{"loot_tables":7}"#),
    ("recipes", r#"{"recipes":"minecraft:r_plural"}"#),
    ("recipe", r#"{"recipe":"minecraft:r_singular"}"#),
    (
        "recipes_and_recipe",
        r#"{"recipes":"minecraft:r_plural","recipe":"minecraft:r_singular"}"#,
    ),
    ("recipe_id", r#"{"recipe_id":"minecraft:stick"}"#),
    ("count", r#"{"count":7}"#),
    ("count_text", r#"{"count":"seven"}"#),
    ("count_big", r#"{"count":5000000000}"#),
    (
        "damage",
        r##"{"damage":{"blocked":true,"type":{"tags":[{"id":"#minecraft:is_projectile","expected":true}]}}}"##,
    ),
    (
        "location_biome",
        r#"{"biomes":"minecraft:plains","dimension":"minecraft:the_end"}"#,
    ),
    (
        "location_nested",
        r#"{"location":{"biomes":"minecraft:desert"},"player":{"location":{"dimension":"minecraft:overworld"}}}"#,
    ),
    (
        "dimension_pair",
        r#"{"from":"minecraft:overworld","to":"minecraft:the_nether"}"#,
    ),
    (
        "inventory",
        r#"{"items":[{"items":"minecraft:diamond","count":{"min":1}}],"slots":{"empty":{"min":3},"full":{"max":2},"occupied":{"min":1}}}"#,
    ),
    ("beacon", r#"{"level":{"min":4}}"#),
    (
        "bred",
        r#"{"child":{"type":"minecraft:cow"},"parent":{"type":"minecraft:cow"},"partner":{"type":"minecraft:pig"}}"#,
    ),
    (
        "target",
        r#"{"signal_strength":{"min":15},"projectile":{"type":"minecraft:arrow"},"shooter":{"type":"minecraft:player"}}"#,
    ),
    (
        "used_on_block",
        r#"{"location":{"type":"minecraft:all_of","terms":[{"type":"minecraft:location_check","predicate":{"block":{"blocks":"minecraft:copper_bulb","state":{"lit":"true"}}}},{"type":"minecraft:match_tool","predicate":{"items":"minecraft:redstone_torch"}}]}}"#,
    ),
    (
        "placed",
        r#"{"location":[{"condition":"minecraft:location_check","predicate":{"block":{"blocks":"minecraft:stone"}}},{"condition":"minecraft:match_tool","predicate":{"items":"minecraft:diamond_pickaxe"}}]}"#,
    ),
    (
        "sink",
        r#"{"blocks":"minecraft:stone","block":"minecraft:dirt","items":"minecraft:diamond","item":{"items":"minecraft:stick"},"entity":{"type":"minecraft:cow"},"player":{"predicate":{"minecraft:vehicle":{"minecraft:entity_type":"minecraft:boat"},"minecraft:location":{"dimension":"minecraft:the_end"}}},"distance":{"y":{"min":1.5},"horizontal":{"min":2.5}},"count":3,"recipes":"minecraft:r","loot_tables":"minecraft:l","num_bees_inside":2,"victims":[{"predicate":{"minecraft:entity_type":"minecraft:cow"}}],"bystander":{"predicate":{"minecraft:entity_type":"minecraft:pig"}},"cause":{"predicate":{"minecraft:entity_type":"minecraft:ghast"}}}"#,
    ),
];

fn record() -> Result<String, std::fmt::Error> {
    let mut out = String::new();
    for trigger in TRIGGERS {
        let mut groups: Vec<(String, Vec<&str>)> = Vec::new();
        for (name, json) in DOCUMENTS {
            let parsed = format!("{:?}", CriterionConditions::parse(trigger, json));
            match groups.iter_mut().find(|(text, _)| *text == parsed) {
                Some((_, names)) => names.push(name),
                None => groups.push((parsed, vec![name])),
            }
        }
        writeln!(out, "{trigger:?} groups={}", groups.len())?;
        for (text, names) in groups.iter().map(|group| (&group.0, &group.1)) {
            writeln!(out, "  [{}] => {text}", names.join(" "))?;
        }
    }
    Ok(out)
}

#[test]
fn criterion_conditions_parse_the_same() -> Result<(), Box<dyn std::error::Error>> {
    let now = record()?;
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/advancement_parse.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &now)?;
    } else {
        assert_eq!(std::fs::read_to_string(&path)?, now);
    }
    Ok(())
}
