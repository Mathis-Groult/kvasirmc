//! A record of what `LivingEntity::tick_movement` does, made before the function was split
//! (`tests/golden/living_movement.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Each scene builds one subject in its own loaded area of a real server's world (stone floor,
//! optionally water, lava or stone around its feet), sets its state (jump key, jump cooldown,
//! movement input, velocity, ground flag, fluid flags and heights, fall distance, an effect, no AI,
//! dead or removed, flying, AI level of detail, age, rotation) and calls `tick_movement` on it four
//! times. The record is the state after each tick: position, velocity, ground flag, fall
//! distance, jump key and cooldown, movement input, health, no-action time, age, rotations and
//! the velocities of the neighbours that overlap it.
//!
//! Subjects: a player (survival, creative, flying or not), plain `LivingEntity` of several
//! types (no `get_mob`), and real mobs (zombie, chicken, cow, squid, strider, cod). A mob whose
//! AI runs draws from the unseeded thread rng (goals, flopping, squid movement), so its lines
//! hold only health, jump cooldown, age and idle time (`random` scenes); no-AI and dead mobs are exact.

use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

use pumpkin_data::Block;
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::entity::EntityType;
use pumpkin_data::potion::Effect;
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use super::super::LivingEntity;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::mob::zombie::zombie::ZombieEntity;
use crate::entity::passive::chicken::ChickenEntity;
use crate::entity::passive::cod::CodEntity;
use crate::entity::passive::cow::CowEntity;
use crate::entity::passive::squid::SquidEntity;
use crate::entity::passive::strider::StriderEntity;
use crate::entity::{Entity, EntityBase};
use crate::world::World;

const FLOOR_Y: i32 = 64;
const TICKS: usize = 4;

#[derive(Clone, Copy)]
enum Kind {
    Player(GameMode),
    Plain(&'static EntityType),
    Zombie,
    Chicken,
    Cow,
    Squid,
    Strider,
    Cod,
}

/// A mob's level of detail: `(dormant, rate, on its decision tick)`.
type Lod = (bool, u8, bool);

#[derive(Clone, Copy)]
struct Scene {
    name: &'static str,
    kind: Kind,
    jumping: bool,
    cooldown: u8,
    input: (f64, f64, f64),
    velocity: (f64, f64, f64),
    airborne: bool,
    water: Option<f64>,
    lava: Option<f64>,
    fall: f32,
    effect: Option<&'static StatusEffect>,
    no_ai: bool,
    dead: bool,
    removed: bool,
    flying: bool,
    stone: bool,
    lod: Option<Lod>,
    neighbour: bool,
    age: Option<i32>,
    turned: bool,
    head: Option<f32>,
    trailing: bool,
    ground: bool,
    random: bool,
}

const fn scene(name: &'static str, kind: Kind) -> Scene {
    Scene {
        name,
        kind,
        jumping: false,
        cooldown: 0,
        input: (0.0, 0.0, 0.0),
        velocity: (0.0, 0.0, 0.0),
        airborne: false,
        water: None,
        lava: None,
        fall: 0.0,
        effect: None,
        no_ai: false,
        dead: false,
        removed: false,
        flying: false,
        stone: false,
        lod: None,
        neighbour: false,
        age: None,
        turned: false,
        head: None,
        trailing: false,
        ground: false,
        random: false,
    }
}

const ZOMBIE: Kind = Kind::Plain(&EntityType::ZOMBIE);
const SURVIVAL: Kind = Kind::Player(GameMode::Survival);
const CREATIVE: Kind = Kind::Player(GameMode::Creative);

impl Scene {
    const fn jumping(mut self) -> Self {
        self.jumping = true;
        self
    }
    const fn cooldown(mut self, cooldown: u8) -> Self {
        self.cooldown = cooldown;
        self
    }
    const fn input(mut self, x: f64, y: f64, z: f64) -> Self {
        self.input = (x, y, z);
        self
    }
    const fn velocity(mut self, x: f64, y: f64, z: f64) -> Self {
        self.velocity = (x, y, z);
        self
    }
    const fn airborne(mut self) -> Self {
        self.airborne = true;
        self
    }
    const fn water(mut self, height: f64) -> Self {
        self.water = Some(height);
        self
    }
    const fn lava(mut self, height: f64) -> Self {
        self.lava = Some(height);
        self
    }
    const fn fall(mut self, distance: f32) -> Self {
        self.fall = distance;
        self
    }
    const fn effect(mut self, effect: &'static StatusEffect) -> Self {
        self.effect = Some(effect);
        self
    }
    const fn no_ai(mut self) -> Self {
        self.no_ai = true;
        self
    }
    const fn dead(mut self) -> Self {
        self.dead = true;
        self
    }
    const fn removed(mut self) -> Self {
        self.removed = true;
        self
    }
    const fn flying(mut self) -> Self {
        self.flying = true;
        self
    }
    const fn stone(mut self) -> Self {
        self.stone = true;
        self
    }
    const fn lod(mut self, dormant: bool, rate: u8, decision: bool) -> Self {
        self.lod = Some((dormant, rate, decision));
        self
    }
    const fn neighbour(mut self) -> Self {
        self.neighbour = true;
        self
    }
    const fn age(mut self, age: i32) -> Self {
        self.age = Some(age);
        self
    }
    const fn turned(mut self) -> Self {
        self.turned = true;
        self
    }
    /// Head turned to `head` degrees against a body and yaw that are not.
    const fn head(mut self, head: f32) -> Self {
        self.head = Some(head);
        self
    }
    /// Moved by 0.2 on x and z since the last tick, as far as the body rotation can tell.
    const fn trailing(mut self) -> Self {
        self.trailing = true;
        self
    }
    /// A `random` scene that also records the ground flag.
    const fn ground(mut self) -> Self {
        self.ground = true;
        self
    }
    const fn random(mut self) -> Self {
        self.random = true;
        self
    }
}

fn input_scenes() -> Vec<Scene> {
    vec![
        scene("zombie at rest", ZOMBIE),
        scene("zombie with input", ZOMBIE).input(1.0, 0.5, -1.0),
        scene("zombie tiny motion", ZOMBIE).velocity(0.002, 0.002, -0.002),
        scene("zombie small motion", ZOMBIE).velocity(0.004, 0.004, 0.004),
        scene("zombie tiny x only", ZOMBIE).velocity(0.002, 0.0, 0.004),
        scene("zombie tiny z only", ZOMBIE).velocity(0.004, 0.0, 0.002),
        scene("player tiny motion", SURVIVAL).velocity(0.002, 0.0, 0.002),
        scene("player small motion", SURVIVAL).velocity(0.002, 0.0, 0.0025),
        scene("player with input", SURVIVAL).input(0.5, 1.0, 1.0),
        scene("zombie slow falling", ZOMBIE)
            .airborne()
            .fall(5.0)
            .effect(&StatusEffect::SLOW_FALLING),
        scene("zombie levitating", ZOMBIE)
            .airborne()
            .fall(5.0)
            .effect(&StatusEffect::LEVITATION),
        scene("zombie falling without effect", ZOMBIE)
            .airborne()
            .fall(5.0),
        scene("zombie in stone", ZOMBIE).stone(),
        scene("player in stone", SURVIVAL).stone(),
    ]
}

fn jump_scenes() -> Vec<Scene> {
    vec![
        scene("zombie jumps on ground", ZOMBIE).jumping(),
        scene("zombie jumping in the air", ZOMBIE)
            .jumping()
            .airborne(),
        scene("zombie jumping with cooldown 3", ZOMBIE)
            .jumping()
            .cooldown(3),
        scene("zombie cooldown 3 but not jumping", ZOMBIE).cooldown(3),
        scene("zombie cooldown 1 in the air", ZOMBIE)
            .jumping()
            .airborne()
            .cooldown(1),
        scene("zombie jumping with input", ZOMBIE)
            .jumping()
            .input(1.0, 0.0, 1.0),
        scene("survival player jumps", SURVIVAL).jumping(),
        scene("survival player jumping with cooldown 2", SURVIVAL)
            .jumping()
            .cooldown(2),
        scene("flying player jumping on ground", CREATIVE)
            .jumping()
            .flying(),
        scene("flying player cooldown 3", CREATIVE)
            .jumping()
            .flying()
            .cooldown(3),
        scene("creative player jumps", CREATIVE).jumping(),
        scene("dead zombie jumping with input", ZOMBIE)
            .dead()
            .jumping()
            .input(1.0, 0.5, 1.0),
        scene("dead player jumping with input", SURVIVAL)
            .dead()
            .jumping()
            .input(1.0, 0.5, 1.0),
    ]
}

fn water_scenes() -> Vec<Scene> {
    vec![
        scene("zombie in shallow water", ZOMBIE).water(0.2),
        scene("zombie in shallow water jumping", ZOMBIE)
            .water(0.2)
            .jumping(),
        scene("zombie in 0.4 water jumping", ZOMBIE)
            .water(0.4)
            .jumping(),
        scene("zombie in deep water jumping", ZOMBIE)
            .water(0.5)
            .jumping(),
        scene("zombie in shallow water jumping with cooldown 5", ZOMBIE)
            .water(0.2)
            .jumping()
            .cooldown(5),
        scene("zombie swimming up in shallow water", ZOMBIE)
            .water(0.2)
            .jumping()
            .airborne(),
        scene("zombie in water with input", ZOMBIE)
            .water(0.9)
            .input(1.0, 0.0, 1.0),
        scene("zombie in deep water", ZOMBIE).water(0.9),
        scene(
            "silverfish in water jumping",
            Kind::Plain(&EntityType::SILVERFISH),
        )
        .water(0.2)
        .jumping(),
        scene("breeze in water jumping", Kind::Plain(&EntityType::BREEZE))
            .water(1.0)
            .jumping(),
        scene("survival player in water", SURVIVAL)
            .water(0.9)
            .input(0.5, 0.0, 0.5),
        scene("survival player swimming up", SURVIVAL)
            .water(0.9)
            .jumping(),
        scene("flying player in water", CREATIVE)
            .water(0.9)
            .flying()
            .jumping()
            .airborne()
            .input(0.5, 0.0, 0.5),
        scene("creative player in water jumping", CREATIVE)
            .water(0.9)
            .jumping()
            .airborne(),
    ]
}

fn lava_scenes() -> Vec<Scene> {
    vec![
        scene("zombie in shallow lava", ZOMBIE).lava(0.1),
        scene("zombie in shallow lava jumping", ZOMBIE)
            .lava(0.1)
            .jumping(),
        scene("zombie in deep lava jumping", ZOMBIE)
            .lava(0.9)
            .jumping(),
        scene("zombie in lava in the air jumping", ZOMBIE)
            .lava(0.1)
            .jumping()
            .airborne(),
        scene("zombie in lava with input", ZOMBIE)
            .lava(0.9)
            .input(1.0, 0.0, 1.0),
        scene("zombie in shallow lava and deep water jumping", ZOMBIE)
            .lava(0.1)
            .water(0.9)
            .jumping(),
        scene("zombie in deep lava and shallow water jumping", ZOMBIE)
            .lava(0.9)
            .water(0.1)
            .jumping(),
        scene("strider in lava jumping", Kind::Strider)
            .no_ai()
            .lava(0.9)
            .jumping(),
        scene("strider in lava", Kind::Strider).no_ai().lava(0.9),
        scene("dead strider in lava", Kind::Strider)
            .dead()
            .lava(0.9)
            .velocity(0.1, 0.0, 0.1),
        scene("dead strider in water", Kind::Strider)
            .dead()
            .water(0.9)
            .velocity(0.1, 0.0, 0.1),
        scene("dead zombie in lava", ZOMBIE)
            .dead()
            .lava(0.9)
            .velocity(0.1, 0.0, 0.1),
        scene("survival player in lava", SURVIVAL)
            .lava(0.9)
            .input(0.5, 0.0, 0.5),
        scene("flying player in lava", CREATIVE)
            .lava(0.9)
            .flying()
            .jumping()
            .airborne(),
    ]
}

fn mob_scenes() -> Vec<Scene> {
    vec![
        scene("zombie mob with AI at rest", Kind::Zombie).random(),
        scene("zombie mob with AI in the air", Kind::Zombie)
            .airborne()
            .velocity(0.0, 0.3, 0.0)
            .random(),
        scene("zombie mob with AI jumping", Kind::Zombie)
            .jumping()
            .random(),
        scene("zombie mob with AI in water", Kind::Zombie)
            .water(0.9)
            .random(),
        scene("zombie mob without AI", Kind::Zombie).no_ai(),
        scene("zombie mob without AI in the air", Kind::Zombie)
            .no_ai()
            .airborne()
            .velocity(0.1, 0.3, 0.1),
        scene("zombie mob without AI in water", Kind::Zombie)
            .no_ai()
            .water(0.9)
            .velocity(0.1, 0.0, 0.1),
        scene("zombie mob without AI jumping", Kind::Zombie)
            .no_ai()
            .jumping()
            .input(1.0, 0.0, 1.0),
        scene("dead zombie mob with AI", Kind::Zombie)
            .dead()
            .jumping()
            .input(1.0, 0.5, 1.0)
            .velocity(0.1, 0.0, 0.1),
        scene("dead zombie mob in the air", Kind::Zombie)
            .dead()
            .airborne()
            .velocity(0.1, 0.3, 0.1),
        scene("zombie mob without AI, head turned", Kind::Zombie)
            .no_ai()
            .turned(),
        scene("moving zombie mob without AI, head turned", Kind::Zombie)
            .no_ai()
            .turned()
            .velocity(0.2, 0.0, 0.2),
        scene("squid mob with AI in water", Kind::Squid)
            .water(0.9)
            .velocity(0.1, 0.05, 0.1)
            .random(),
        scene("dead squid in water", Kind::Squid)
            .dead()
            .water(0.9)
            .velocity(0.1, 0.05, 0.1),
        scene("dead squid in the air", Kind::Squid)
            .dead()
            .airborne()
            .velocity(0.1, 0.05, 0.1),
        scene("cod flopping on land", Kind::Cod).no_ai().random(),
        scene("strider mob with AI in lava", Kind::Strider)
            .lava(0.9)
            .random(),
    ]
}

/// Appended after the first mutation run: a flopping fish (the ground flag it leaves) and the
/// body rotation of still and moving mobs with the head turned far.
fn widened_scenes() -> Vec<Scene> {
    vec![
        scene("cod flopping on land, ground flag", Kind::Cod)
            .no_ai()
            .ground()
            .random(),
        scene("zombie mob without AI, head far turned", Kind::Zombie)
            .no_ai()
            .head(120.0),
        scene(
            "moving zombie mob without AI, head far turned",
            Kind::Zombie,
        )
        .no_ai()
        .head(120.0)
        .trailing(),
        scene(
            "moving zombie mob without AI, head turned a little",
            Kind::Zombie,
        )
        .no_ai()
        .head(40.0)
        .trailing(),
    ]
}

fn ageable_scenes() -> Vec<Scene> {
    vec![
        scene("baby cow without AI", Kind::Cow).no_ai().age(-100),
        scene("cow in love cooldown without AI", Kind::Cow)
            .no_ai()
            .age(50),
        scene("adult cow without AI", Kind::Cow).no_ai(),
        scene("dead baby cow without AI", Kind::Cow)
            .no_ai()
            .age(-100)
            .dead(),
        scene("removed baby cow without AI", Kind::Cow)
            .no_ai()
            .age(-100)
            .removed(),
        scene("baby cow with AI", Kind::Cow).age(-100).random(),
        scene("dead baby cow with AI", Kind::Cow)
            .age(-100)
            .dead()
            .random(),
        scene("chicken falling without AI", Kind::Chicken)
            .no_ai()
            .airborne()
            .velocity(0.0, -0.5, 0.0),
        scene("chicken on ground without AI", Kind::Chicken)
            .no_ai()
            .velocity(0.0, -0.5, 0.0),
        scene("chicken rising without AI", Kind::Chicken)
            .no_ai()
            .airborne()
            .velocity(0.0, 0.5, 0.0),
        scene("baby chicken falling without AI", Kind::Chicken)
            .no_ai()
            .airborne()
            .age(-100)
            .velocity(0.0, -0.5, 0.0),
    ]
}

fn push_scenes() -> Vec<Scene> {
    vec![
        scene("zombie near a neighbour", ZOMBIE).neighbour(),
        scene("player near a neighbour", SURVIVAL).neighbour(),
        scene("zombie mob near a neighbour", Kind::Zombie)
            .no_ai()
            .neighbour(),
        scene(
            "dormant mob on its decision tick near a neighbour",
            Kind::Zombie,
        )
        .no_ai()
        .lod(true, 4, true)
        .neighbour(),
        scene(
            "dormant mob between decisions near a neighbour",
            Kind::Zombie,
        )
        .no_ai()
        .lod(true, 4, false)
        .neighbour(),
        scene(
            "relaxed mob between decisions near a neighbour",
            Kind::Zombie,
        )
        .no_ai()
        .lod(false, 4, false)
        .neighbour(),
        scene(
            "relaxed mob on its decision tick near a neighbour",
            Kind::Zombie,
        )
        .no_ai()
        .lod(false, 4, true)
        .neighbour(),
        scene("dormant mob between decisions alone", Kind::Zombie)
            .no_ai()
            .lod(true, 4, false),
    ]
}

fn scenes() -> Vec<Scene> {
    let mut all = input_scenes();
    all.extend(jump_scenes());
    all.extend(water_scenes());
    all.extend(lava_scenes());
    all.extend(mob_scenes());
    all.extend(ageable_scenes());
    all.extend(push_scenes());
    all.extend(widened_scenes());
    all
}

struct Subject {
    base: Arc<dyn EntityBase>,
    /// Kept so a player's outgoing queue stays open; nothing reads it.
    _tap: Option<crate::net::java::PacketTap>,
}

impl Subject {
    fn living(&self) -> Result<&LivingEntity, String> {
        self.base
            .get_living_entity()
            .ok_or_else(|| "the subject is not a living entity".to_string())
    }
}

fn place_block(world: &Arc<World>, position: BlockPos, block: &Block) {
    world.set_block_state(
        &position,
        block.default_state.id,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

fn spawn(
    world: &Arc<World>,
    index: usize,
    scene: &Scene,
    at: Vector3<f64>,
) -> Result<Subject, String> {
    let id = 60_000 + index as i32;
    let uuid = Uuid::from_u128(0x4d6f_7665_0000_0000 + index as u128);
    let entity = |entity_type: &'static EntityType| {
        Entity::from_uuid_with_id(id, uuid, world.clone(), at, entity_type)
    };
    if let Kind::Player(gamemode) = scene.kind {
        let who = Identity {
            name: "LivingMove",
            uuid,
            entity_id: id,
        };
        let (player, tap) = test_support::java_player_as(world, gamemode, &who)
            .ok_or("the test player's client no longer owned its outgoing queue")?;
        player.get_entity().set_pos(at);
        return Ok(Subject {
            base: player,
            _tap: Some(tap),
        });
    }
    let base: Arc<dyn EntityBase> = match scene.kind {
        Kind::Plain(entity_type) => Arc::new(LivingEntity::new(entity(entity_type))),
        Kind::Zombie => ZombieEntity::new(entity(&EntityType::ZOMBIE)),
        Kind::Chicken => ChickenEntity::new(entity(&EntityType::CHICKEN)),
        Kind::Cow => CowEntity::new(entity(&EntityType::COW)),
        Kind::Squid => SquidEntity::new(entity(&EntityType::SQUID)),
        Kind::Strider => StriderEntity::new(entity(&EntityType::STRIDER)),
        Kind::Cod => CodEntity::new(entity(&EntityType::COD)),
        Kind::Player(_) => return Err("a player is built above".to_string()),
    };
    Ok(Subject { base, _tap: None })
}

fn set_up_mob(subject: &Subject, scene: &Scene, id: i32) {
    let Some(mob) = subject.base.get_mob() else {
        return;
    };
    let mob_entity = mob.get_mob_entity();
    mob_entity.set_no_ai(scene.no_ai);
    if let (Some(age), Some(ageable)) = (scene.age, mob.as_ageable()) {
        ageable.set_age(age);
    }
    if let Some((dormant, rate, decision)) = scene.lod {
        let tier = if dormant {
            kvasir_moblod::Tier::Dormant
        } else {
            kvasir_moblod::Tier::Relaxed
        };
        mob_entity.set_ai_level(tier, rate);
        // `tick_count` does not move inside `tick_movement`: pick one that is, or is not, a
        // decision tick of this mob.
        let base = 10 + i32::from(rate) - (10 + id) % i32::from(rate);
        let tick = if decision { base } else { base + 1 };
        mob_entity
            .living_entity
            .entity
            .tick_count
            .store(tick, Relaxed);
    }
}

fn set_up(subject: &Subject, scene: &Scene, id: i32) -> Result<(), String> {
    let living = subject.living()?;
    let entity = &living.entity;
    entity.on_ground.store(!scene.airborne, Relaxed);
    entity.velocity.store(Vector3::new(
        scene.velocity.0,
        scene.velocity.1,
        scene.velocity.2,
    ));
    living
        .movement_input
        .store(Vector3::new(scene.input.0, scene.input.1, scene.input.2));
    living.jumping.store(scene.jumping, Relaxed);
    living.jumping_cooldown.store(scene.cooldown, Relaxed);
    living.fall_distance.store(scene.fall);
    if let Some(height) = scene.water {
        entity.touching_water.store(true, Relaxed);
        entity.water_height.store(height);
    }
    if let Some(height) = scene.lava {
        entity.touching_lava.store(true, Relaxed);
        entity.lava_height.store(height);
    }
    if let Some(effect_type) = scene.effect {
        living.add_effect(Effect {
            effect_type,
            duration: 100,
            amplifier: 0,
            ambient: false,
            show_particles: false,
            show_icon: false,
            blend: false,
        });
    }
    if scene.flying
        && let Some(player) = subject.base.get_player()
    {
        player
            .abilities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .flying = true;
    }
    if scene.turned {
        entity.yaw.store(90.0);
        entity.head_yaw.store(40.0);
        entity.body_yaw.store(0.0);
    }
    if let Some(head) = scene.head {
        entity.yaw.store(90.0);
        entity.head_yaw.store(head);
        entity.body_yaw.store(0.0);
    }
    if scene.trailing {
        let pos = entity.pos.load();
        entity
            .last_pos
            .store(Vector3::new(pos.x - 0.2, pos.y, pos.z - 0.2));
    }
    if scene.dead {
        living.health.store(0.0);
    }
    if matches!(scene.kind, Kind::Cod) {
        entity.vertical_collision.store(true, Relaxed);
    }
    set_up_mob(subject, scene, id);
    Ok(())
}

fn age_text(subject: &Subject) -> String {
    subject
        .base
        .get_mob()
        .and_then(|mob| mob.as_ageable())
        .map_or_else(|| "-".to_string(), |ageable| ageable.get_age().to_string())
}

/// The state after a tick. A `random` scene keeps only what the mob's AI draws cannot change.
fn state_text(
    subject: &Subject,
    others: &[Arc<LivingEntity>],
    random: bool,
    ground: bool,
) -> Result<String, String> {
    let living = subject.living()?;
    let entity = &living.entity;
    let pos = entity.pos.load();
    let velocity = entity.velocity.load();
    let mut text = String::new();
    if random {
        let _ = write!(
            text,
            "health {:.1} cooldown {} age {} idle {}",
            living.health.load(),
            living.jumping_cooldown.load(Relaxed),
            age_text(subject),
            living.no_action_time.load(Relaxed),
        );
        if ground {
            let _ = write!(text, " ground {}", entity.on_ground.load(Relaxed));
        }
        return Ok(text);
    }
    let _ = write!(
        text,
        "y {:.6} vy {:.6} ground {} health {:.1} cooldown {} age {}",
        pos.y,
        velocity.y,
        entity.on_ground.load(Relaxed),
        living.health.load(),
        living.jumping_cooldown.load(Relaxed),
        age_text(subject),
    );
    let input = living.movement_input.load();
    let _ = write!(
        text,
        " pos {:.6} {:.6} vel {:.6} {:.6} fall {:.3} jump {} input {:.6} {:.6} {:.6} idle {} yaw {:.1} body {:.1} head {:.1}",
        pos.x,
        pos.z,
        velocity.x,
        velocity.z,
        living.fall_distance.load(),
        living.jumping.load(Relaxed),
        input.x,
        input.y,
        input.z,
        living.no_action_time.load(Relaxed),
        entity.yaw.load(),
        entity.body_yaw.load(),
        entity.head_yaw.load(),
    );
    for other in others {
        let v = other.entity.velocity.load();
        let _ = write!(text, " | neighbour {:.6} {:.6} {:.6}", v.x, v.y, v.z);
    }
    Ok(text)
}

fn run_scene(
    world: &Arc<World>,
    index: usize,
    scene: &Scene,
    report: &mut String,
) -> Result<(), String> {
    let chunk_x = index as i32 * 4;
    for cx in chunk_x..chunk_x + 4 {
        for cz in 0..4 {
            world.level.loaded_chunks.insert(
                Vector2::new(cx, cz),
                pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
            );
        }
    }
    let x = chunk_x * 16 + 24;
    let z = 24;
    for dx in -2..=2 {
        for dz in -2..=2 {
            place_block(world, BlockPos::new(x + dx, FLOOR_Y, z + dz), &Block::STONE);
        }
    }
    let fluid = if scene.lava.is_some() {
        Some(&Block::LAVA)
    } else if scene.water.is_some() {
        Some(&Block::WATER)
    } else if scene.stone {
        Some(&Block::STONE)
    } else {
        None
    };
    if let Some(block) = fluid {
        place_block(world, BlockPos::new(x, FLOOR_Y + 1, z), block);
        place_block(world, BlockPos::new(x, FLOOR_Y + 2, z), block);
    }
    let at = Vector3::new(
        f64::from(x) + 0.5,
        f64::from(FLOOR_Y + 1),
        f64::from(z) + 0.5,
    );
    let id = 60_000 + index as i32;
    let subject = spawn(world, index, scene, at)?;
    let mut others = Vec::new();
    if scene.neighbour {
        let other = Arc::new(LivingEntity::new(Entity::from_uuid_with_id(
            id + 1_000,
            Uuid::from_u128(0x4d6f_7665_0001_0000 + index as u128),
            world.clone(),
            Vector3::new(at.x + 0.3, at.y, at.z + 0.1),
            &EntityType::ZOMBIE,
        )));
        world.spawn_entity(other.clone());
        others.push(other);
    }
    set_up(&subject, scene, id)?;
    if scene.removed {
        subject.living()?.entity.remove();
    }

    let _ = writeln!(report, "{}:", scene.name);
    for tick in 1..=TICKS {
        subject.living()?.tick_movement(subject.base.as_ref());
        let text = state_text(&subject, &others, scene.random, scene.ground)?;
        let _ = writeln!(report, "  {tick}: {text}");
    }

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
async fn living_movement() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server
        .worlds
        .load()
        .first()
        .cloned()
        .ok_or("the server has no world")?;
    let mut report = String::new();
    for (index, scene) in scenes().iter().enumerate() {
        run_scene(&world, index, scene, &mut report)?;
    }
    server.stop();
    script_golden::bless_or_compare("living_movement.txt", &report)
}
