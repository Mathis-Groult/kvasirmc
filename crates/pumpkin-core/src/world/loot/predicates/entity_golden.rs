//! A record of what `entity_matches` answers, made before the function was split
//! (`tests/golden/loot_entity_matches.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Every predicate below is asked of every entity in the cast; the record has one line per
//! predicate with a `1` or `0` per entity. The cast is built to differ in the things the
//! predicates read: position, motion, flags, equipment, riders, size, shearing.

use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use pumpkin_data::{
    data_component_impl::EquipmentSlot, entity::EntityType, item::Item, item_stack::ItemStack,
};
use pumpkin_util::math::vector3::Vector3;
use serde_json::Value;
use uuid::Uuid;

use super::entity_matches;
use crate::entity::ai::test_support::world;
use crate::entity::mob::magma_cube::MagmaCubeEntity;
use crate::entity::passive::sheep::SheepEntity;
use crate::entity::r#type::from_type;
use crate::entity::{Entity, EntityBase};
use crate::world::loot::LootContextParameters;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[allow(clippy::needless_raw_string_hashes)]
const PREDICATES: &[&str] = &[
    r#"{}"#,
    r#"[]"#,
    r#""x""#,
    r#"{"type":"minecraft:zombie"}"#,
    r#"{"entity_type":"zombie"}"#,
    r#"{"type":"pig"}"#,
    r##"{"type":"#minecraft:undead"}"##,
    r#"{"type":["minecraft:pig","minecraft:sheep"]}"#,
    r#"{"type":5}"#,
    r#"{"minecraft:type":"minecraft:armor_stand"}"#,
    r##"{"type":"#minecraft:skeletons"}"##,
    r#"{"distance":{"horizontal":{"max":5}}}"#,
    r#"{"distance":{"horizontal":{"min":6}}}"#,
    r#"{"distance":{"x":{"min":2,"max":4}}}"#,
    r#"{"distance":{"y":{"min":0.5}}}"#,
    r#"{"distance":{"z":{"max":3.9}}}"#,
    r#"{"distance":{"absolute":{"max":4.5}}}"#,
    r#"{"distance":{"absolute":{"min":4.6}}}"#,
    r#"{"distance":{"foo":{"min":100}}}"#,
    r#"{"distance":3}"#,
    r#"{"distance":{"x":3}}"#,
    r#"{"distance":{"x":{"min":3}}}"#,
    r#"{"distance":{"horizontal":{"min":5,"max":5}}}"#,
    r#"{"movement":{"x":{"min":0.4}}}"#,
    r#"{"movement":{"y":{"max":-0.4}}}"#,
    r#"{"movement":{"z":{"min":-0.1,"max":0.1}}}"#,
    r#"{"movement":{"horizontal":{"min":0.5}}}"#,
    r#"{"movement":{"vertical":{"min":0.5}}}"#,
    r#"{"movement":{"vertical":{"max":0.1}}}"#,
    r#"{"movement":{"fall_distance":{"min":2}}}"#,
    r#"{"movement":{"fall_distance":{"max":0}}}"#,
    r#"{"movement":{"nope":{"min":99}}}"#,
    r#"{"movement":1}"#,
    r#"{"flags":{"is_on_fire":true}}"#,
    r#"{"flags":{"is_on_fire":false}}"#,
    r#"{"flags":{"is_sneaking":true}}"#,
    r#"{"flags":{"is_sprinting":true}}"#,
    r#"{"flags":{"is_swimming":true}}"#,
    r#"{"flags":{"is_swimming":false}}"#,
    r#"{"flags":{"is_baby":true}}"#,
    r#"{"flags":{"is_baby":false}}"#,
    r#"{"flags":{"is_on_ground":true}}"#,
    r#"{"flags":{"is_on_ground":false}}"#,
    r#"{"flags":{"unknown":false}}"#,
    r#"{"flags":{"is_baby":"yes"}}"#,
    r#"{"flags":{"minecraft:is_sneaking":true,"is_on_ground":false}}"#,
    r#"{"flags":1}"#,
    r#"{"equipment":{"head":{"items":"minecraft:diamond_helmet"}}}"#,
    r#"{"equipment":{"mainhand":{"items":"minecraft:diamond_sword"}}}"#,
    r#"{"equipment":{"mainhand":{"items":"minecraft:air"}}}"#,
    r#"{"equipment":{"offhand":{"items":"minecraft:air"}}}"#,
    r#"{"equipment":{"chest":{"items":"minecraft:air"}}}"#,
    r#"{"equipment":{"legs":{"items":"minecraft:air"}}}"#,
    r#"{"equipment":{"feet":{"items":"minecraft:air"}}}"#,
    r#"{"equipment":{"body":{"items":"minecraft:air"}}}"#,
    r#"{"equipment":{"weapon":{"items":"minecraft:diamond_sword"}}}"#,
    r#"{"equipment":{"armor.head":{"count":{"min":1}}}}"#,
    r#"{"equipment":{"helmet":{"count":1}}}"#,
    r#"{"equipment":{"nowhere":{"items":"minecraft:air"}}}"#,
    r##"{"equipment":{"minecraft:head":{"items":"#minecraft:head_armor"}}}"##,
    r#"{"equipment":{"head":5}}"#,
    r#"{"equipment":3}"#,
    r#"{"slots":{"head":{"items":"minecraft:diamond_helmet"}}}"#,
    r#"{"slots":{"mainhand":{"count":{"min":1}}}}"#,
    r#"{"slots":4}"#,
    r#"{"location":{}}"#,
    r#"{"location":{"block":{"blocks":"minecraft:air"}}}"#,
    r#"{"location":{"biomes":"minecraft:plains"}}"#,
    r#"{"location":5}"#,
    r#"{"stepping_on":{}}"#,
    r#"{"stepping_on":{"block":{"blocks":"minecraft:air"}}}"#,
    r#"{"stepping_on":2}"#,
    r#"{"vehicle":{"type":"minecraft:pig"}}"#,
    r#"{"vehicle":{"type":"minecraft:zombie"}}"#,
    r#"{"vehicle":{}}"#,
    r#"{"passenger":{"type":"minecraft:skeleton"}}"#,
    r#"{"passenger":{"type":"minecraft:pig"}}"#,
    r#"{"passenger":{}}"#,
    r#"{"targeted_entity":{}}"#,
    r#"{"targeted_entity":{"type":"minecraft:pig"}}"#,
    r#"{"team":"red"}"#,
    r#"{"team":"minecraft:red"}"#,
    r#"{"team":3}"#,
    r#"{"nbt":{"Invulnerable":false}}"#,
    r#"{"nbt":{"Invulnerable":0}}"#,
    r#"{"nbt":{"id":"minecraft:zombie"}}"#,
    r#"{"nbt":{"id":"minecraft:pig"}}"#,
    r#"{"nbt":{"OnGround":true}}"#,
    r#"{"nbt":{"OnGround":false}}"#,
    r#"{"nbt":{"Fire":2}}"#,
    r#"{"nbt":{"Fire":-1}}"#,
    r#"{"nbt":{"Sheared":true}}"#,
    r#"{"nbt":{"Sheared":false}}"#,
    r#"{"nbt":{"Size":0}}"#,
    r#"{"nbt":{"Size":2}}"#,
    r#"{"nbt":{"Motion":[0.5]}}"#,
    r#"{"nbt":null}"#,
    r#"{"nbt":{"Nope":[1,2]}}"#,
    r#"{"nbt":{"CustomName":"x"}}"#,
    r#"{"components":{"minecraft:custom_name":"x"}}"#,
    r#"{"components":{"minecraft:nonsense":1}}"#,
    r#"{"components":{"minecraft:sheep/color":0}}"#,
    r#"{"components":{"minecraft:sheep/color":1}}"#,
    r#"{"components":{}}"#,
    r#"{"components":3}"#,
    r#"{"components":{"minecraft:sheep/color":null}}"#,
    r#"{"type_specific":{"type":"minecraft:slime","size":1}}"#,
    r#"{"type_specific":{"type":"minecraft:slime","size":{"min":2}}}"#,
    r#"{"type_specific":{"type":"minecraft:cube_mob","size":{"max":1}}}"#,
    r#"{"type_specific":{"type":"minecraft:slime"}}"#,
    r#"{"type_specific":{"type":"minecraft:sheep","sheared":true}}"#,
    r#"{"type_specific":{"type":"minecraft:sheep","sheared":false}}"#,
    r#"{"type_specific":{"type":"minecraft:sheep"}}"#,
    r#"{"type_specific":{"type":"minecraft:raider","is_captain":true}}"#,
    r#"{"type_specific":{"type":"minecraft:raider","is_captain":false,"has_raid":false}}"#,
    r#"{"type_specific":{"type":"minecraft:raider","has_raid":true}}"#,
    r#"{"type_specific":{"type":"minecraft:fishing_hook","in_open_water":true}}"#,
    r#"{"type_specific":{"type":"minecraft:player","gamemode":"survival"}}"#,
    r#"{"type_specific":{"type":"minecraft:player","level":{"min":1}}}"#,
    r#"{"type_specific":{"type":"minecraft:player","looking_at":{}}}"#,
    r#"{"type_specific":{"type":"minecraft:lightning","blocks_set_on_fire":0}}"#,
    r#"{"type_specific":{"type":"minecraft:lightning","blocks_set_on_fire":1}}"#,
    r#"{"type_specific":{"type":"minecraft:lightning","blocks_set_on_fire":{"min":1}}}"#,
    r#"{"type_specific":{"type":"minecraft:other"}}"#,
    r#"{"type_specific":{}}"#,
    r#"{"type_specific":7}"#,
    r#"{"sub_predicate":{"type":"minecraft:slime","size":1}}"#,
    r#"{"sub_predicate":{"type":"slime","size":3}}"#,
    r#"{"type_specific/slime":{"size":1}}"#,
    r#"{"sub_predicate/sheep":{"sheared":true}}"#,
    r#"{"minecraft:type_specific/slime":{"size":3}}"#,
    r#"{"type_specific/other":{}}"#,
    r#"{"type_specific/slime":3}"#,
    r#"{"sub_predicate/raider":{"is_captain":false}}"#,
    r#"{"type_specific/":{}}"#,
    r#"{"sub_predicate/lightning":{"blocks_set_on_fire":0}}"#,
    r#"{"unknown_key":true}"#,
    r#"{"type":"minecraft:zombie","flags":{"is_sneaking":true}}"#,
    r#"{"type":"minecraft:zombie","flags":{"is_sneaking":false}}"#,
    r#"{"nbt":{"Age":0}}"#,
    r#"{"nbt":{"Age":1}}"#,
    r#"{"nbt":{"PortalCooldown":0}}"#,
    r#"{"nbt":{"fall_distance":0.0}}"#,
    r#"{"nbt":{"fall_distance":2.5}}"#,
    r#"{"nbt":{"Brain":{"memories":{}}}}"#,
    r#"{"nbt":{"Motion":[0.0]}}"#,
    r#"{"nbt":{"Motion":[0.0,0.0,0.0]}}"#,
    r#"{"nbt":{"Motion":[1.0]}}"#,
    r#"{"nbt":{"Color":0}}"#,
    r#"{"nbt":{"Health":1.0}}"#,
    r#"{"nbt":{"Air":300}}"#,
    r#"{"nbt":{"Invulnerable":false,"Sheared":true}}"#,
    r#"{"movement":{"fall_distance":{"min":2,"max":3}}}"#,
];

fn put(entity: &dyn EntityBase, slot: &EquipmentSlot, stack: ItemStack) {
    if let Some(living) = entity.get_living_entity()
        && let Ok(mut equipment) = living.entity_equipment.lock()
    {
        equipment.put(slot, stack);
    }
}

#[allow(clippy::too_many_lines)]
#[tokio::test(flavor = "multi_thread")]
async fn entity_matches_answers_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let world = world(&folder);
    let at = |x: f64, y: f64, z: f64| Vector3::new(x, y, z);
    let make =
        |kind: &'static EntityType, n: u128, pos| from_type(kind, pos, &world, Uuid::from_u128(n));

    let zombie = make(&EntityType::ZOMBIE, 1, at(3.0, 64.0, 4.0));
    zombie.get_entity().sneaking.store(true, Ordering::Relaxed);
    zombie.get_entity().sprinting.store(true, Ordering::Relaxed);
    zombie.get_entity().on_ground.store(true, Ordering::Relaxed);
    zombie.get_entity().fire_ticks.store(40, Ordering::Relaxed);
    zombie.get_entity().velocity.store(at(0.5, -0.5, 0.0));
    put(
        &*zombie,
        &EquipmentSlot::HEAD,
        ItemStack::new(1, &Item::DIAMOND_HELMET),
    );
    put(
        &*zombie,
        &EquipmentSlot::MAIN_HAND,
        ItemStack::new(1, &Item::DIAMOND_SWORD),
    );

    if let Some(living) = zombie.get_living_entity() {
        living.fall_distance.store(2.5);
    }
    let baby_pig = make(&EntityType::PIG, 2, at(0.0, 64.0, 0.0));
    baby_pig.get_entity().age.store(-100, Ordering::Relaxed);
    baby_pig
        .get_entity()
        .swimming
        .store(true, Ordering::Relaxed);
    baby_pig.get_entity().velocity.store(at(0.0, 0.6, 0.0));
    put(
        &*baby_pig,
        &EquipmentSlot::CHEST,
        ItemStack::new(1, &Item::DIAMOND_HELMET),
    );

    let sheep = SheepEntity::new(Entity::new(
        world.clone(),
        at(-2.0, 60.0, 1.0),
        &EntityType::SHEEP,
    ));
    let shorn = SheepEntity::new(Entity::new(
        world.clone(),
        at(1.0, 70.0, 1.0),
        &EntityType::SHEEP,
    ));
    shorn.set_sheared(true);
    shorn.set_color(1);
    let small = MagmaCubeEntity::new(Entity::new(
        world.clone(),
        at(9.0, 64.0, 0.0),
        &EntityType::MAGMA_CUBE,
    ));
    small.slime.set_size(1, true);
    let big = MagmaCubeEntity::new(Entity::new(
        world.clone(),
        at(0.0, 64.0, 12.0),
        &EntityType::MAGMA_CUBE,
    ));
    big.slime.set_size(3, true);
    let stand = make(&EntityType::ARMOR_STAND, 3, at(0.0, 64.0, 0.0));
    let steed = make(&EntityType::PIG, 4, at(0.0, 65.0, 0.0));
    let rider = make(&EntityType::SKELETON, 5, at(0.0, 66.0, 0.0));
    if let (Ok(mut passengers), Ok(mut vehicle)) = (
        steed.get_entity().passengers.lock(),
        rider.get_entity().vehicle.lock(),
    ) {
        passengers.push(rider.clone());
        *vehicle = Some(steed.clone());
    }
    let pillager = make(&EntityType::PILLAGER, 9, at(0.0, 64.0, 0.0));
    let lightning = make(&EntityType::LIGHTNING_BOLT, 6, at(0.0, 64.0, 0.0));
    let bobber = make(&EntityType::FISHING_BOBBER, 8, at(0.0, 64.0, 0.0));

    let cast: Vec<(&str, Arc<dyn EntityBase>)> = vec![
        ("zombie", zombie),
        ("baby_pig", baby_pig),
        ("sheep", sheep),
        ("shorn_sheep", shorn),
        ("small_cube", small),
        ("big_cube", big),
        ("stand", stand),
        ("steed", steed),
        ("rider", rider),
        ("pillager", pillager),
        ("lightning", lightning),
        ("bobber", bobber),
    ];
    let params = LootContextParameters {
        world: Some(&world),
        position: Some(at(0.0, 64.0, 0.0)),
        ..Default::default()
    };

    let mut now = String::new();
    writeln!(
        now,
        "cast: {}",
        cast.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(" ")
    )?;
    for text in PREDICATES {
        let predicate: Value = serde_json::from_str(text)?;
        let bits: String = cast
            .iter()
            .map(|(_, entity)| {
                if entity_matches(&**entity, &predicate, &params) {
                    '1'
                } else {
                    '0'
                }
            })
            .collect();
        writeln!(now, "{bits} {text}")?;
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/loot_entity_matches.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &now)?;
    } else {
        assert_eq!(std::fs::read_to_string(&path)?, now);
    }
    Ok(())
}
