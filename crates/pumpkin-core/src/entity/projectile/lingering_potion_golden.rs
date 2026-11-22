//! A record of what `LingeringPotionEntity::on_hit` does, made before it was split
//! (`tests/golden/lingering_potion_on_hit.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene is built in its own loaded area of a real server's world (stone floor at y 63,
//! blocks placed without neighbour updates). A lingering potion entity holding the scene's
//! stack is hit on a block or on a zombie by calling `on_hit` directly; fire and soul fire stand
//! on the hit block and around it. A survival player in a corner of the area is the watcher.
//!
//! After the hit the scene records: the packets the watcher was sent (short ones in hex, the
//! rest by length: Add Entity and sounds carry random bytes), the blocks around the hit, the
//! entities now in the world and every area effect cloud with its numbers, effects and stack.

use std::fmt::Write;
use std::num::NonZero;
use std::sync::Arc;

use pumpkin_data::data_component::DataComponent;
use pumpkin_data::data_component_impl::{
    DataComponentImpl, PotionContentsImpl, StatusEffectInstance,
};
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::potion::Potion;
use pumpkin_data::{Block, BlockDirection};
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::cylindrical_chunk_iterator::Cylindrical;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use super::ProjectileHit;
use super::lingering_potion::LingeringPotionEntity;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::area_effect_cloud::AreaEffectCloudEntity;
use crate::entity::living::LivingEntity;
use crate::entity::player::Player;
use crate::entity::{Entity, EntityBase};
use crate::net::java::PacketTap;
use crate::world::World;

const FLOOR_Y: i32 = 63;
/// Packet ids whose bytes are random like in vanilla: Add Entity (UUID, entity id), entity data (entity id) and sounds: recorded by id only.
const RANDOM_PACKETS: [u8; 4] = [1, 101, 118, 119];
const HIT_AT: Vec3 = (12.7, 64.9, 12.3);

type Vec3 = (f64, f64, f64);

#[derive(Clone, Copy)]
enum Contents {
    /// A stack without the potion contents component.
    Bare,
    /// The component with these fields.
    Potion {
        potion: Option<&'static Potion>,
        color: Option<i32>,
        custom: &'static [&'static str],
    },
}

#[derive(Clone, Copy)]
enum Target {
    Block,
    Zombie,
}

struct Scene {
    name: &'static str,
    contents: Contents,
    target: Target,
}

const fn scene(name: &'static str, contents: Contents, target: Target) -> Scene {
    Scene {
        name,
        contents,
        target,
    }
}

const fn potion(potion: &'static Potion) -> Contents {
    Contents::Potion {
        potion: Some(potion),
        color: None,
        custom: &[],
    }
}

const fn colored(potion: &'static Potion, color: i32) -> Contents {
    Contents::Potion {
        potion: Some(potion),
        color: Some(color),
        custom: &[],
    }
}

const fn custom(
    potion: Option<&'static Potion>,
    color: Option<i32>,
    custom: &'static [&'static str],
) -> Contents {
    Contents::Potion {
        potion,
        color,
        custom,
    }
}

fn base_scenes() -> Vec<Scene> {
    vec![
        scene("bare_stack", Contents::Bare, Target::Block),
        scene("water", potion(&Potion::WATER), Target::Block),
        scene("water_on_zombie", potion(&Potion::WATER), Target::Zombie),
        scene(
            "water_colored",
            colored(&Potion::WATER, 0x00ff_0000),
            Target::Block,
        ),
        scene("mundane", potion(&Potion::MUNDANE), Target::Block),
        scene("slowness", potion(&Potion::SLOWNESS), Target::Block),
        scene(
            "slowness_on_zombie",
            potion(&Potion::SLOWNESS),
            Target::Zombie,
        ),
        scene("harming", potion(&Potion::HARMING), Target::Block),
        scene("healing", potion(&Potion::HEALING), Target::Zombie),
        scene(
            "poison_colored",
            colored(&Potion::POISON, 0x0000_ff7f),
            Target::Block,
        ),
        scene("swiftness", potion(&Potion::SWIFTNESS), Target::Block),
    ]
}

fn custom_scenes() -> Vec<Scene> {
    vec![
        scene(
            "custom_pair",
            custom(None, None, &["minecraft:poison", "minecraft:weakness"]),
            Target::Block,
        ),
        scene(
            "custom_instant_last",
            custom(
                None,
                None,
                &["minecraft:slowness", "minecraft:instant_health"],
            ),
            Target::Block,
        ),
        scene(
            "custom_instant_damage_last",
            custom(
                None,
                None,
                &["minecraft:regeneration", "minecraft:instant_damage"],
            ),
            Target::Block,
        ),
        scene(
            "custom_colored",
            custom(None, Some(0x0012_3456), &["minecraft:poison"]),
            Target::Block,
        ),
        scene(
            "water_with_custom",
            custom(Some(&Potion::WATER), None, &["minecraft:slowness"]),
            Target::Block,
        ),
        scene(
            "potion_and_custom",
            custom(Some(&Potion::SWIFTNESS), None, &["minecraft:slowness"]),
            Target::Zombie,
        ),
        scene(
            "unknown_custom_effect",
            custom(None, None, &["minecraft:no_such_effect"]),
            Target::Block,
        ),
        scene("empty_potion", custom(None, None, &[]), Target::Block),
        scene(
            "empty_potion_colored",
            custom(None, Some(0x0000_00ff), &[]),
            Target::Block,
        ),
        scene(
            "colored_black",
            custom(None, Some(0), &["minecraft:poison"]),
            Target::Block,
        ),
    ]
}

fn stack_of(contents: Contents) -> ItemStack {
    let mut stack = ItemStack::new(1, &Item::LINGERING_POTION);
    if let Contents::Potion {
        potion,
        color,
        custom,
    } = contents
    {
        stack.patch.push((
            DataComponent::PotionContents,
            Some(
                PotionContentsImpl {
                    potion_id: potion.map(|p| i32::from(p.id)),
                    custom_color: color,
                    custom_effects: custom
                        .iter()
                        .map(|id| StatusEffectInstance {
                            effect_id: (*id).into(),
                            amplifier: 1,
                            duration: 300,
                            ambient: false,
                            show_particles: true,
                            show_icon: true,
                        })
                        .collect(),
                    custom_name: None,
                }
                .to_dyn(),
            ),
        ));
    }
    stack
}

struct Area<'a> {
    world: &'a Arc<World>,
    x: i32,
    z: i32,
}

impl Area<'_> {
    fn put(&self, block: &Block, x: i32, y: i32, z: i32) {
        self.world.set_block_state(
            &BlockPos(Vector3::new(self.x + x, y, self.z + z)),
            block.default_state.id,
            BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
        );
    }

    fn name_at(&self, x: i32, y: i32, z: i32) -> &'static str {
        let state =
            self.world
                .get_block_state_id(&BlockPos(Vector3::new(self.x + x, y, self.z + z)));
        Block::from_state_id(state).name
    }

    fn point(&self, (x, y, z): Vec3) -> Vector3<f64> {
        Vector3::new(f64::from(self.x) + x, y, f64::from(self.z) + z)
    }
}

/// Fire on the hit block, soul fire east of it, fire west, north and south, fire on the
/// diagonal and two blocks away.
const FIRES: [(&Block, i32, i32); 7] = [
    (&Block::FIRE, 12, 12),
    (&Block::SOUL_FIRE, 13, 12),
    (&Block::FIRE, 11, 12),
    (&Block::FIRE, 12, 13),
    (&Block::SOUL_FIRE, 12, 11),
    (&Block::FIRE, 13, 13),
    (&Block::FIRE, 14, 12),
];

fn fires_text(area: &Area<'_>) -> String {
    FIRES
        .iter()
        .map(|(_, x, z)| area.name_at(*x, FLOOR_Y + 1, *z))
        .collect::<Vec<_>>()
        .join(",")
}

fn sent_text(tap: &mut PacketTap) -> String {
    tap.drain()
        .iter()
        .map(|p| {
            let id = p.first().copied().unwrap_or(0);
            if RANDOM_PACKETS.contains(&id) {
                // The length follows the size of the entity id, which depends on test order.
                return id.to_string();
            }
            if p.len() > 24 {
                return format!("{id}:{}", p.len());
            }
            let hex = p.iter().fold(String::new(), |mut hex, b| {
                let _ = write!(hex, "{b:02x}");
                hex
            });
            format!("{id}:{}:{hex}", p.len())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn effects_text(cloud: &AreaEffectCloudEntity) -> String {
    cloud
        .effects
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .map(|(effect, duration, amplifier, ambient, particles, icon)| {
            format!(
                "{}:{duration}:{amplifier}:{}{}{}",
                effect.id,
                u8::from(*ambient),
                u8::from(*particles),
                u8::from(*icon)
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn cloud_text(cloud: &AreaEffectCloudEntity) -> String {
    let get = |value: &std::sync::Mutex<i32>| {
        *value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    };
    let get_f = |value: &std::sync::Mutex<f32>| {
        *value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    };
    let stack = cloud
        .item_stack
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let contents = stack
        .get_data_component::<PotionContentsImpl>()
        .map_or_else(
            || "none".to_owned(),
            |pc| {
                format!(
                    "potion {:?} color {:?} custom {}",
                    pc.potion_id,
                    pc.custom_color,
                    pc.custom_effects.len()
                )
            },
        );
    let pos = cloud.entity.pos.load();
    format!(
        "pos ({:.2}, {:.2}, {:.2}) radius {:.4} duration {} age {} delay {} wait {} per_tick {:.5} on_use {:.2} duration_on_use {} effects [{}] stack {} x{} {contents}",
        pos.x,
        pos.y,
        pos.z,
        get_f(&cloud.radius),
        get(&cloud.duration),
        get(&cloud.age),
        get(&cloud.reapplication_delay),
        get(&cloud.wait_time),
        get_f(&cloud.radius_on_tick),
        get_f(&cloud.radius_on_use),
        get(&cloud.duration_on_use),
        effects_text(cloud),
        stack.item.registry_key,
        stack.item_count,
    )
}

fn clouds_text(world: &World) -> String {
    let mut lines = Vec::new();
    for entity in world.entities.load().iter() {
        if let Some(cloud) = entity.cast_any().downcast_ref::<AreaEffectCloudEntity>() {
            lines.push(cloud_text(cloud));
        }
    }
    lines.join("\n    ")
}

fn spawn_zombie(area: &Area<'_>) -> Arc<LivingEntity> {
    let entity = Entity::from_uuid_with_id(
        83_000,
        Uuid::from_u128(0x4c49_4e47_0000),
        Arc::clone(area.world),
        area.point(HIT_AT),
        &EntityType::ZOMBIE,
    );
    let living = Arc::new(LivingEntity::new(entity));
    let base: Arc<dyn EntityBase> = living.clone();
    area.world.spawn_entity_non_save(base);
    living
}

/// Makes the watcher track the entity: its chunks count as sent and the tracker pairs them.
fn watch(world: &World, area: &Area<'_>, watcher: &Arc<Player>, entity_id: i32) {
    let chunk = Vector2::new((area.x + 12) >> 4, (area.z + 12) >> 4);
    let mut sender = watcher
        .chunk_sender
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for dx in -3..=3 {
        for dz in -3..=3 {
            sender.mark_sent_out_of_band(Vector2::new(chunk.x + dx, chunk.y + dz));
        }
    }
    drop(sender);
    if let Some(tracked) = world.entity_tracker.get_tracked_entity(entity_id) {
        tracked.update_player(watcher, world);
    }
}

fn run_scene(world: &Arc<World>, index: usize, scene: &Scene, report: &mut String) -> TestResult {
    let chunk_x = index as i32 * 4;
    for cx in chunk_x..chunk_x + 4 {
        for cz in 0..4 {
            world.level.loaded_chunks.insert(
                Vector2::new(cx, cz),
                pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
            );
        }
    }
    let area = Area {
        world,
        x: chunk_x * 16 + 16,
        z: 16,
    };
    for x in 0..24 {
        for z in 0..24 {
            area.put(&Block::STONE, x, FLOOR_Y, z);
        }
    }
    for (block, x, z) in FIRES {
        area.put(block, x, FLOOR_Y + 1, z);
    }

    let who = Identity {
        name: "LingerWatch",
        uuid: Uuid::from_u128(0x4c69_6e67_6572_0000 + index as u128),
        entity_id: 62_000 + index as i32,
    };
    let (watcher, mut tap) = test_support::java_player_as(world, GameMode::Survival, &who)
        .ok_or("the watcher's client no longer owned its outgoing queue")?;
    watcher.get_entity().set_pos(area.point((0.5, 64.0, 23.5)));
    watcher.watched_section.store(Cylindrical::new(
        Vector2::new((area.x + 12) >> 4, (area.z + 12) >> 4),
        NonZero::<u8>::MIN.saturating_add(3),
    ));
    world.add_player(&watcher)?;

    let zombie = spawn_zombie(&area);
    let entity = Entity::from_uuid_with_id(
        84_000,
        Uuid::from_u128(0x4c49_4e47_1000),
        Arc::clone(world),
        area.point(HIT_AT),
        &EntityType::LINGERING_POTION,
    );
    let flask = LingeringPotionEntity::new(entity);
    flask.set_item_stack(stack_of(scene.contents));
    let flask = Arc::new(flask);
    let base: Arc<dyn EntityBase> = flask.clone();
    world.spawn_entity_non_save(base);
    watch(world, &area, &watcher, 84_000);
    let before = world.entities.load().len();
    drop(tap.drain());

    let hit_pos = area.point(HIT_AT);
    let hit = match scene.target {
        Target::Block => ProjectileHit::Block {
            pos: BlockPos(Vector3::new(
                hit_pos.x.floor() as i32,
                FLOOR_Y,
                hit_pos.z.floor() as i32,
            )),
            face: BlockDirection::Up,
            hit_pos,
            normal: Vector3::new(0.0, 1.0, 0.0),
        },
        Target::Zombie => ProjectileHit::Entity {
            entity: zombie,
            hit_pos,
            normal: Vector3::new(0.0, 1.0, 0.0),
        },
    };
    flask.on_hit(hit);

    let _ = writeln!(report, "{}:", scene.name);
    let _ = writeln!(report, "  sent [{}]", sent_text(&mut tap));
    let _ = writeln!(report, "  fires {}", fires_text(&area));
    let _ = writeln!(
        report,
        "  entities {} -> {} flask removed {}",
        before,
        world.entities.load().len(),
        flask.get_entity().is_removed()
    );
    let _ = writeln!(report, "  clouds\n    {}", clouds_text(world));

    for entity in world.entities.load().iter() {
        world.remove_entity(entity.as_ref());
    }
    for cx in chunk_x..chunk_x + 4 {
        for cz in 0..4 {
            world.level.loaded_chunks.remove(&Vector2::new(cx, cz));
        }
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn lingering_potion_on_hit() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server
        .worlds
        .load()
        .first()
        .cloned()
        .ok_or("the server has no world")?;
    let mut report = String::new();
    let scenes: Vec<Scene> = base_scenes().into_iter().chain(custom_scenes()).collect();
    for (index, scene) in scenes.iter().enumerate() {
        run_scene(&world, index, scene, &mut report)?;
        for player in world.players.load().iter() {
            world.remove_player(player, false).await;
        }
    }
    server.stop();
    script_golden::bless_or_compare("lingering_potion_on_hit.txt", &report)
}
