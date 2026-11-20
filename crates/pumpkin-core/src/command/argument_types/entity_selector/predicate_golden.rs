//! A record of which entities each selector predicate accepts
//! (`tests/golden/entity_selector_predicates.txt`; rewrite it on purpose with `KVASIR_BLESS=1`),
//! made before `EntitySelectorPredicate::test` was split.
//!
//! The four harness players and five mobs are set up through commands (teams, scores, tags,
//! game modes, levels, rotations, names, advancements, an item) plus a removed (killed) zombie. Per selector:
//! the parse result, then which of the nine entities pass all its predicates. Selectors with a
//! distance or a box also record the `Distance` (around the origin) and `BoundingBox` predicates
//! built from them, which `find_entities` would add.

use std::collections::HashMap;
use std::fmt::Write;
use std::sync::Arc;

use pumpkin_data::entity::EntityType;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;

use super::{EntitySelectorPredicate, parser::EntitySelectorParser};
use crate::command::commands::script_golden::{self, TestResult};
use crate::command::string_reader::StringReader;
use crate::entity::ai::test_support;
use crate::entity::living::LivingEntity;
use crate::entity::{Entity, EntityBase};
use crate::world::World;

const SETUP: &[&str] = &[
    "team add red",
    "team add blue",
    "team join red GoldA GoldC",
    "team join blue GoldB",
    "team join red @e[type=minecraft:cow]",
    "scoreboard objectives add kills dummy",
    "scoreboard objectives add deaths dummy",
    "scoreboard players set GoldA kills 5",
    "scoreboard players set GoldB kills 12",
    "scoreboard players set GoldB deaths 3",
    "scoreboard players set GoldC deaths -2",
    "scoreboard players set @e[type=minecraft:cow] kills 7",
    "tag GoldA add alpha",
    "tag GoldB add alpha",
    "tag GoldB add beta",
    "tag @e[type=minecraft:cow] add beta",
    "gamemode creative GoldB",
    "gamemode spectator GoldD",
    "gamemode adventure GoldC",
    "xp set GoldA 3 levels",
    "xp set GoldB 30 levels",
    "tp GoldA 0.5 64 0.5 0 0",
    "tp GoldB 3.5 64 4.5 90 -30",
    "tp GoldC 10.5 70 -5.5 -170 60",
    "tp GoldD -20.5 64 30.5 179 -90",
    "advancement grant GoldB only minecraft:story/root",
    "give GoldA minecraft:stone 3",
    "data merge entity @e[type=minecraft:zombie,limit=1] {CustomName:\"Bob\"}",
];

const SELECTORS: &[&str] = &[
    "@e",
    "@e[gamemode=creative]",
    "@e[gamemode=!creative]",
    "@e[gamemode=spectator]",
    "@e[gamemode=survival]",
    "@e[level=3]",
    "@e[level=..10]",
    "@e[level=10..]",
    "@e[level=0]",
    "@e[x_rotation=-45..-15]",
    "@e[x_rotation=60]",
    "@e[x_rotation=..0]",
    "@e[y_rotation=80..100]",
    "@e[y_rotation=170..-170]",
    "@e[y_rotation=-180..-160]",
    "@e[y_rotation=..0]",
    "@e[y_rotation=0..]",
    "@e[y_rotation=350..10]",
    "@e[x_rotation=-30]",
    "@e[x_rotation=-30..-30]",
    "@e[x_rotation=-29.9..]",
    "@e[x_rotation=..-30.1]",
    "@e[x_rotation=20..60]",
    "@e[x_rotation=-90..-89]",
    "@e[y_rotation=90]",
    "@e[y_rotation=-90..90]",
    "@e[y_rotation=170..190]",
    "@e[y_rotation=-200..-160]",
    "@e[y_rotation=179..181]",
    "@e[y_rotation=-170..-90]",
    "@e[y_rotation=-1..1]",
    "@e[y_rotation=359..361]",
    "@e[y_rotation=90..450]",
    // The parser feeds x_rotation to yaw and y_rotation to pitch (vanilla: the reverse), so the
    // yaw wrap-around cases go through x_rotation here.
    "@e[x_rotation=170..190]",
    "@e[x_rotation=-200..-160]",
    "@e[x_rotation=179..181]",
    "@e[x_rotation=-170..-90]",
    "@e[x_rotation=350..370]",
    "@e[x_rotation=-1..1]",
    "@e[x_rotation=45]",
    "@e[x_rotation=90..450]",
    "@e[x_rotation=0..]",
    "@e[x_rotation=..-90]",
    "@e[distance=..5]",
    "@e[distance=5..]",
    "@e[distance=4..6]",
    "@e[distance=0]",
    "@e[x=0,y=60,z=0,dx=5,dy=10,dz=5]",
    "@e[x=-30,y=0,z=-30,dx=60,dy=100,dz=60]",
    "@e[dx=-3,dy=-3,dz=-3]",
    "@e[type=minecraft:zombie]",
    "@e[type=!minecraft:zombie]",
    "@e[type=minecraft:player]",
    "@e[type=!minecraft:player,type=!minecraft:cow]",
    "@e[name=GoldB]",
    "@e[name=!GoldB]",
    "@e[name=Bob]",
    "@e[name=zombie]",
    "@e[name=cow]",
    "@e[name=minecraft:cow]",
    "@e[tag=alpha]",
    "@e[tag=!alpha]",
    "@e[tag=alpha,tag=beta]",
    "@e[tag=]",
    "@e[tag=!]",
    "@e[team=red]",
    "@e[team=!red]",
    "@e[team=blue]",
    "@e[team=green]",
    "@e[team=!blue,team=!red]",
    "@e[scores={kills=5}]",
    "@e[scores={kills=6..}]",
    "@e[scores={kills=..5}]",
    "@e[scores={kills=1..,deaths=1..}]",
    "@e[scores={deaths=..0}]",
    "@e[scores={missing=0}]",
    "@e[scores={missing=1}]",
    "@e[advancements={minecraft:story/root=true}]",
    "@e[advancements={minecraft:story/root=false}]",
    "@e[advancements={story/root=true}]",
    "@e[advancements={minecraft:no/such=true}]",
    "@e[advancements={minecraft:no/such=false}]",
    "@e[nbt={Tags:[\"beta\"]}]",
    "@e[nbt=!{Tags:[\"beta\"]}]",
    "@e[nbt={Tags:[\"alpha\",\"beta\"]}]",
    "@e[nbt={Inventory:[{id:\"minecraft:stone\"}]}]",
    "@e[nbt={Inventory:[{id:\"minecraft:stone\",count:2}]}]",
    "@e[nbt={NoSuchKey:1b}]",
    "@e[nbt={}]",
    "@e[nbt={OnGround:0b}]",
    "@e[predicate=minecraft:no_such_predicate]",
    "@e[predicate=!minecraft:no_such_predicate]",
    "@e[type=minecraft:cow,tag=beta,team=red,scores={kills=7}]",
    "@e[type=minecraft:cow,tag=beta,team=blue]",
    "@a",
    "@p",
    "@s",
];

fn spawn_mob(
    world: &Arc<World>,
    kind: &'static EntityType,
    entity_id: i32,
    pos: Vector3<f64>,
) -> Arc<LivingEntity> {
    let mut entity = Entity::new(Arc::clone(world), pos, kind);
    entity.entity_id = entity_id;
    entity.entity_uuid = uuid::Uuid::from_u128(0x0b05_5ba0_0002_0000 + entity_id as u128);
    let mob = Arc::new(LivingEntity::new(entity));
    world.spawn_entity(mob.clone());
    mob
}

fn passing(predicates: &[EntitySelectorPredicate], entities: &[(&str, &dyn EntityBase)]) -> String {
    let all = EntitySelectorPredicate::new_all_of(predicates.to_vec());
    let names: Vec<&str> = entities
        .iter()
        .filter(|(_, entity)| all.test(*entity))
        .map(|(name, _)| *name)
        .collect();
    names.join(",")
}

/// Every entity's rotation, then per selector which entities pass its predicates.
fn selector_lines(entities: &[(&str, &dyn EntityBase)], now: &mut String) -> TestResult {
    let mut rotation_text = String::new();
    for (name, entity) in entities {
        let base = entity.get_entity();
        write!(
            rotation_text,
            " {name}={}/{}",
            base.yaw.load(),
            base.pitch.load()
        )?;
    }
    writeln!(now, "rotations{rotation_text}")?;
    for selector in SELECTORS {
        let mut reader = StringReader::new(*selector);
        let parsed = EntitySelectorParser::new(&mut reader, true).parse_and_consume();
        let selector_text = match parsed {
            Err(error) => format!("error {error:?}"),
            Ok(parsed) => {
                let mut text = format!("[{}]", passing(&parsed.predicates, entities));
                if let Some(distance) = parsed.distance {
                    let around =
                        EntitySelectorPredicate::Distance(distance, Vector3::new(0.5, 64.0, 0.5));
                    write!(text, " distance [{}]", passing(&[around], entities))?;
                }
                if let Some(bounding_box) = parsed.bounding_box {
                    let inside = EntitySelectorPredicate::BoundingBox(bounding_box);
                    write!(text, " box [{}]", passing(&[inside], entities))?;
                }
                text
            }
        };
        writeln!(now, "{selector} -> {selector_text}")?;
    }
    Ok(())
}

/// Hand-built advancement predicates.
fn advancement_lines(entities: &[(&str, &dyn EntityBase)], now: &mut String) -> TestResult {
    // Hand-built, so the predicate is checked apart from the parser.
    for (id, done) in [
        ("minecraft:story/root", true),
        ("minecraft:story/root", false),
        ("story/root", true),
        ("story/root", false),
        ("story/mine_stone", true),
        ("story/mine_stone", false),
        ("minecraft:story/mine_stone", true),
        ("minecraft:story/mine_stone", false),
        ("minecraft:no/such", true),
        ("minecraft:no/such", false),
    ] {
        let wanted = EntitySelectorPredicate::Advancements(HashMap::from([(id.to_string(), done)]));
        writeln!(
            now,
            "advancements {id}={done} -> [{}]",
            passing(&[wanted], entities)
        )?;
    }
    let both = EntitySelectorPredicate::Advancements(HashMap::from([
        ("minecraft:story/root".to_string(), true),
        ("minecraft:story/mine_stone".to_string(), false),
    ]));
    writeln!(
        now,
        "advancements root=true,mine_stone=false -> [{}]",
        passing(&[both], entities)
    )?;
    let empty = EntitySelectorPredicate::Advancements(HashMap::new());
    writeln!(
        now,
        "advancements {{}} -> [{}]",
        passing(&[empty], entities)
    )?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn selector_predicates_accept_the_same_entities() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    for cx in -3..3 {
        for cz in -3..3 {
            world.level.loaded_chunks.insert(
                Vector2::new(cx, cz),
                pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
            );
        }
    }
    let zombie = spawn_mob(
        &world,
        &EntityType::ZOMBIE,
        72_101,
        Vector3::new(2.5, 64.0, 2.5),
    );
    let cow = spawn_mob(
        &world,
        &EntityType::COW,
        72_102,
        Vector3::new(-4.5, 66.0, 1.5),
    );
    let skeleton = spawn_mob(
        &world,
        &EntityType::SKELETON,
        72_103,
        Vector3::new(12.5, 64.0, 12.5),
    );
    let dead = spawn_mob(
        &world,
        &EntityType::ZOMBIE,
        72_104,
        Vector3::new(0.5, 64.0, 5.5),
    );
    let pig = spawn_mob(
        &world,
        &EntityType::PIG,
        72_105,
        Vector3::new(40.5, 64.0, 40.5),
    );
    let mut online = script_golden::join(&world).await?;
    let source_player = online[0].player.clone();

    let mut now = String::new();
    for step in SETUP {
        let result = script_golden::run(&server, &source_player, step);
        writeln!(now, "{step} -> {result}")?;
    }
    let rotations: [(&LivingEntity, f32, f32); 9] = [
        (&online[0].player.living_entity, 0.0, 0.0),
        (&online[1].player.living_entity, 90.0, -30.0),
        (&online[2].player.living_entity, -170.0, 60.0),
        (&online[3].player.living_entity, 179.0, -90.0),
        (&zombie, 45.0, 10.0),
        (&cow, -90.0, 20.0),
        (&skeleton, 359.0, -10.0),
        (&dead, 0.0, 0.0),
        (&pig, 540.0, 0.0),
    ];
    for (living, yaw, pitch) in rotations {
        living.entity.yaw.store(yaw);
        living.entity.pitch.store(pitch);
    }
    dead.entity
        .removal_reason
        .store(Some(crate::entity::RemovalReason::Killed));
    let _ = script_golden::packets_bedrock_counted(&mut online);

    let entities: Vec<(&str, &dyn EntityBase)> = vec![
        ("GoldA", &*online[0].player),
        ("GoldB", &*online[1].player),
        ("GoldC", &*online[2].player),
        ("GoldD", &*online[3].player),
        ("zombie", &*zombie),
        ("cow", &*cow),
        ("skeleton", &*skeleton),
        ("dead", &*dead),
        ("pig", &*pig),
    ];
    selector_lines(&entities, &mut now)?;
    advancement_lines(&entities, &mut now)?;

    script_golden::bless_or_compare("entity_selector_predicates.txt", &now)?;
    script_golden::finish(&server, &world, &online).await;
    Ok(())
}
