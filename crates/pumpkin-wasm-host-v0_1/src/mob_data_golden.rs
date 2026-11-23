//! Golden for `HostMob::get_mob_data`: entities of every type that has its own data record are
//! built through `from_type`, put into the plugin resource table and read back. A scene is one
//! entity with the state its record reads (colours past the dye range, levels past the byte
//! range, negative experience, ages around the baby threshold ...); every entity type also gets
//! its plain record, grouped by identical result. A resource id that is not in the table prints
//! `error`. Bless with `BLESS_MOB_DATA=1`.

use std::{collections::BTreeMap, fmt::Write as _, sync::Arc};

use pumpkin_config::{AdvancedConfiguration, BasicConfiguration, TelemetryConfig};
use pumpkin_core::{
    data::VanillaData,
    entity::{
        EntityBase,
        mob::{
            creeper::CreeperEntity, enderman::EndermanEntity, shulker::ShulkerEntity,
            slime::SlimeEntity, zombie::zombie::ZombieEntity,
        },
        passive::{
            cat::CatEntity, fox::FoxEntity, iron_golem::IronGolemEntity, sheep::SheepEntity,
            tamable::TamableAnimal, villager::VillagerEntity, wolf::WolfEntity,
        },
        r#type::from_type,
    },
    server::Server,
    world::World,
};
use pumpkin_data::{BlockDirection, BlockStateId, entity::EntityType};
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_wasm_host_common::state::PluginHostState;
use wasmtime::component::Resource;

use crate::pumpkin::plugin::world::{HostMob, Mob};

const GOLDEN: &str = include_str!("mob_data.txt");

const OWNER: uuid::Uuid = uuid::Uuid::from_u128(0x0123_4567_89ab_cdef_0011_2233_4455_6677);

async fn server(folder: &tempfile::TempDir) -> Arc<Server> {
    let basic = BasicConfiguration {
        default_level_name: folder.path().join("world").to_string_lossy().into_owned(),
        ..BasicConfiguration::default()
    };
    let telemetry = TelemetryConfig {
        enabled: false,
        ..TelemetryConfig::default()
    };
    Server::new(
        basic,
        AdvancedConfiguration::default(),
        telemetry,
        VanillaData::default(),
        Vec::new(),
    )
    .await
    .expect("a server starts on an empty folder")
}

type Scene = (String, Arc<dyn EntityBase>);

/// Builds an entity of `kind` and lets `setup` change the concrete `T` behind it.
fn spawn<T: 'static>(
    world: &Arc<World>,
    kind: &'static EntityType,
    setup: impl FnOnce(&T),
) -> Arc<dyn EntityBase> {
    let entity = from_type(
        kind,
        Vector3::new(0.5, 64.0, 0.5),
        world,
        uuid::Uuid::new_v4(),
    );
    setup(
        entity
            .cast_any()
            .downcast_ref::<T>()
            .expect("the type builds the struct the scene sets up"),
    );
    entity
}

fn sheep_scenes(world: &Arc<World>) -> Vec<Scene> {
    let mut out = Vec::new();
    for (color, sheared) in [(0, false), (5, true), (15, false), (16, true), (200, false)] {
        let entity = spawn::<SheepEntity>(world, &EntityType::SHEEP, |s| {
            s.set_color(color);
            s.set_sheared(sheared);
        });
        out.push((format!("sheep color={color} sheared={sheared}"), entity));
    }
    out
}

fn wolf_scenes(world: &Arc<World>) -> Vec<Scene> {
    let mut out = Vec::new();
    let wild = spawn::<WolfEntity>(world, &EntityType::WOLF, |_| {});
    out.push(("wolf untamed".to_string(), wild));
    let tamed = spawn::<WolfEntity>(world, &EntityType::WOLF, |w| {
        w.set_tame(true);
        w.set_owner(Some(OWNER));
        w.set_collar_color(3);
    });
    out.push(("wolf tamed owned collar=3".to_string(), tamed));
    let sitting = spawn::<WolfEntity>(world, &EntityType::WOLF, |w| {
        w.set_tame(true);
        w.set_ordered_to_sit(true);
        w.set_in_sitting_pose(true);
        w.set_collar_color(14);
    });
    out.push(("wolf tamed sitting collar=14".to_string(), sitting));
    let owner_only = spawn::<WolfEntity>(world, &EntityType::WOLF, |w| w.set_owner(Some(OWNER)));
    out.push(("wolf owner but not tamed".to_string(), owner_only));
    let sit_wild = spawn::<WolfEntity>(world, &EntityType::WOLF, |w| w.set_in_sitting_pose(true));
    out.push(("wolf sitting but not tamed".to_string(), sit_wild));
    out
}

fn cat_scenes(world: &Arc<World>) -> Vec<Scene> {
    let mut out = Vec::new();
    let wild = spawn::<CatEntity>(world, &EntityType::CAT, |_| {});
    out.push(("cat untamed".to_string(), wild));
    let tamed = spawn::<CatEntity>(world, &EntityType::CAT, |c| {
        c.set_tame(true, Some(OWNER));
        c.set_collar_color(9);
    });
    out.push(("cat tamed owned collar=9".to_string(), tamed));
    let sitting = spawn::<CatEntity>(world, &EntityType::CAT, |c| {
        c.set_tame(true, None);
        c.set_ordered_to_sit(true);
        c.set_in_sitting_pose(true);
    });
    out.push(("cat tamed sitting no owner".to_string(), sitting));
    let sit_wild = spawn::<CatEntity>(world, &EntityType::CAT, |c| c.set_in_sitting_pose(true));
    out.push(("cat sitting but not tamed".to_string(), sit_wild));
    out
}

fn set_profession(villager: &VillagerEntity, profession: i32, level: i32) {
    let mut data = villager
        .villager_data
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    data.profession = VarInt(profession);
    data.level = VarInt(level);
}

fn villager_scenes(world: &Arc<World>) -> Vec<Scene> {
    let mut out = Vec::new();
    let fresh = spawn::<VillagerEntity>(world, &EntityType::VILLAGER, |_| {});
    out.push(("villager new".to_string(), fresh));
    let cases = [
        (0, 1, 0),
        (1, 2, 10),
        (5, 3, 250),
        (9, 4, 70_000),
        (13, 5, -5),
        (14, 255, 1),
        (3, 256, 2),
        (4, 1000, i32::MAX),
        (6, -1, i32::MIN),
        (2, 0, 0),
        (99, 1, 1),
        (-3, 1, 1),
    ];
    for (profession, level, xp) in cases {
        let entity = spawn::<VillagerEntity>(world, &EntityType::VILLAGER, |v| {
            set_profession(v, profession, level);
            v.xp.store(xp, std::sync::atomic::Ordering::Relaxed);
        });
        out.push((
            format!("villager profession={profession} level={level} xp={xp}"),
            entity,
        ));
    }
    out
}

fn creeper_scenes(world: &Arc<World>) -> Vec<Scene> {
    let mut out = Vec::new();
    let plain = spawn::<CreeperEntity>(world, &EntityType::CREEPER, |_| {});
    out.push(("creeper new".to_string(), plain));
    let cases = [
        (true, 20, false, 3),
        (false, 0, true, 0),
        (false, -4, false, 255),
        (true, 30, true, 256),
        (false, 7, false, 10_000),
        (false, 1, false, -2),
    ];
    for (charged, fuse, ignited, radius) in cases {
        let entity = spawn::<CreeperEntity>(world, &EntityType::CREEPER, |c| {
            c.set_charged(charged);
            c.set_fuse(fuse);
            c.set_ignited(ignited);
            c.set_explosion_radius(radius);
        });
        out.push((
            format!("creeper charged={charged} fuse={fuse} ignited={ignited} radius={radius}"),
            entity,
        ));
    }
    out
}

fn slime_scenes(world: &Arc<World>) -> Vec<Scene> {
    let mut out = Vec::new();
    for size in [1, 2, 3, 4, 8] {
        let entity = spawn::<SlimeEntity>(world, &EntityType::SLIME, |s| s.set_size(size, false));
        out.push((format!("slime size={size}"), entity));
    }
    out
}

fn enderman_scenes(world: &Arc<World>) -> Vec<Scene> {
    let mut out = Vec::new();
    let plain = spawn::<EndermanEntity>(world, &EntityType::ENDERMAN, |_| {});
    out.push(("enderman new".to_string(), plain));
    for (block, creepy) in [(Some(1), false), (None, true), (Some(2000), true)] {
        let entity = spawn::<EndermanEntity>(world, &EntityType::ENDERMAN, |e| {
            e.set_carried_block(block.and_then(BlockStateId::new));
            e.set_creepy(creepy);
        });
        out.push((
            format!("enderman carried={block:?} creepy={creepy}"),
            entity,
        ));
    }
    out
}

fn small_mob_scenes(world: &Arc<World>) -> Vec<Scene> {
    let mut out = Vec::new();
    for created in [false, true] {
        let entity = spawn::<IronGolemEntity>(world, &EntityType::IRON_GOLEM, |g| {
            g.set_player_created(created);
        });
        out.push((format!("iron_golem player_created={created}"), entity));
    }
    for (sitting, sleeping, crouching) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (false, false, true),
        (true, true, true),
    ] {
        let entity = spawn::<FoxEntity>(world, &EntityType::FOX, |f| {
            f.set_sitting(sitting);
            f.set_sleeping(sleeping);
            f.set_crouching(crouching);
        });
        out.push((
            format!("fox sitting={sitting} sleeping={sleeping} crouching={crouching}"),
            entity,
        ));
    }
    for (baby, doors) in [(false, false), (true, false), (false, true), (true, true)] {
        let entity = spawn::<ZombieEntity>(world, &EntityType::ZOMBIE, |z| {
            z.set_baby(baby);
            z.set_can_break_doors(doors);
        });
        out.push((format!("zombie baby={baby} breaks_doors={doors}"), entity));
    }
    out
}

fn shulker_scenes(world: &Arc<World>) -> Vec<Scene> {
    let mut out = Vec::new();
    let plain = spawn::<ShulkerEntity>(world, &EntityType::SHULKER, |_| {});
    out.push(("shulker new".to_string(), plain));
    let faces = [
        BlockDirection::Down,
        BlockDirection::Up,
        BlockDirection::North,
        BlockDirection::South,
        BlockDirection::West,
        BlockDirection::East,
    ];
    for (i, face) in faces.into_iter().enumerate() {
        let color = [None, Some(0), Some(7), Some(15), Some(16), Some(1)][i];
        let peek = (i as u8) * 20;
        let entity = spawn::<ShulkerEntity>(world, &EntityType::SHULKER, |s| {
            s.set_attach_face(face);
            s.set_raw_peek(peek);
            s.set_color(color);
        });
        out.push((
            format!("shulker face={face:?} peek={peek} color={color:?}"),
            entity,
        ));
    }
    out
}

fn ageable_scenes(world: &Arc<World>) -> Vec<Scene> {
    let mut out = Vec::new();
    for kind in [&EntityType::COW, &EntityType::PIG] {
        for age in [-24_000, -1, 0, 1, 6000] {
            let entity = from_type(
                kind,
                Vector3::new(0.5, 64.0, 0.5),
                world,
                uuid::Uuid::new_v4(),
            );
            entity
                .get_living_entity()
                .expect("a cow and a pig are living")
                .entity
                .age
                .store(age, std::sync::atomic::Ordering::Relaxed);
            out.push((format!("{} age={age}", kind.resource_name), entity));
        }
    }
    out
}

fn scenes(world: &Arc<World>) -> Vec<Scene> {
    let mut all = sheep_scenes(world);
    all.extend(wolf_scenes(world));
    all.extend(cat_scenes(world));
    all.extend(villager_scenes(world));
    all.extend(creeper_scenes(world));
    all.extend(slime_scenes(world));
    all.extend(enderman_scenes(world));
    all.extend(small_mob_scenes(world));
    all.extend(shulker_scenes(world));
    all.extend(ageable_scenes(world));
    all
}

fn read(state: &mut PluginHostState, entity: Arc<dyn EntityBase>) -> String {
    let resource: Resource<Mob> = state.add(entity).expect("the table has room");
    HostMob::get_mob_data(state, resource)
        .map_or_else(|_| "error".to_string(), |data| format!("{data:?}"))
}

/// Every entity type's record before any state is set, types with the same record grouped.
fn plain_records(state: &mut PluginHostState, world: &Arc<World>) -> String {
    let mut groups: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for kind in EntityType::ALL {
        let entity = from_type(
            kind,
            Vector3::new(0.5, 64.0, 0.5),
            world,
            uuid::Uuid::new_v4(),
        );
        // A new slime rolls its size.
        if let Some(slime) = entity.cast_any().downcast_ref::<SlimeEntity>() {
            slime.set_size(2, false);
        }
        groups
            .entry(read(state, entity))
            .or_default()
            .push(kind.resource_name);
    }
    let mut out = String::new();
    for (record, names) in groups {
        writeln!(
            out,
            "{} types => {record}\n    {}",
            names.len(),
            names.join(" ")
        )
        .expect("write to a String");
    }
    out
}

async fn record() -> String {
    let folder = tempfile::tempdir().expect("temp dir");
    let server = server(&folder).await;
    let world = server.worlds.load().first().cloned().expect("a world");
    let mut state = PluginHostState::new();
    let mut out = String::from("== scenes\n");
    for (name, entity) in scenes(&world) {
        writeln!(out, "{name} => {}", read(&mut state, entity)).expect("write to a String");
    }
    out.push_str("== not in the table\n");
    let missing: Resource<Mob> = Resource::new_borrow(9999);
    let verdict = if HostMob::get_mob_data(&mut state, missing).is_err() {
        "error"
    } else {
        "answered"
    };
    writeln!(out, "unknown id => {verdict}").expect("write to a String");
    out.push_str("== plain record of every entity type\n");
    out.push_str(&plain_records(&mut state, &world));
    server.stop();
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn get_mob_data_golden() {
    let got = record().await;
    if std::env::var_os("BLESS_MOB_DATA").is_some() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/mob_data.txt");
        std::fs::write(path, &got).expect("write golden");
        return;
    }
    assert_eq!(got, GOLDEN, "get_mob_data changed what it reports");
}
