//! A record of what `LivingEntity::tick` does, made before the function was split
//! (`tests/golden/living_tick.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Player scenes: a real Java player with a fixed identity, not added to the world, stands on a
//! floor block in its own loaded area of a real server's world, poisoned and fast, hungry, and
//! has one item in use (`set_active_hand`, finishing after 1 or 3 ticks) whose stack sits in the
//! selected slot, the off hand or neither (the active-hand fallback), in survival or creative,
//! one or sixteen of them, with an empty or a full inventory (the remainder is dropped). The
//! items are foods and drinks whose consume effects all have probability 1 (the consume chance
//! draws from the unseeded `rand::random()`, so chorus fruit, rotten flesh, raw chicken and the
//! poisonous potato are left out), potions, milk, honey, stews, the ominous bottle, a shield and
//! bread with a use cooldown. Other player scenes: hurt-cooldown and last-hurt-by-player
//! counters, a dead player, a player standing on magma.
//!
//! Zombie scenes: a plain `LivingEntity` zombie (no AI: `get_mob` is `None`), spawned into the
//! world, sliding on stone, standing on magma, dead (death animation, poof and removal at 20),
//! falling below the world, standing inside stone.
//!
//! The entity is ticked 40 times by calling `LivingEntity::tick` directly. After each tick:
//! position, velocity, ground flag, health, death time, removed, hurt cooldown, last hurt by
//! player and its memory, the effects (sorted), and for players the item use time, the item in
//! use and active hand, every non-empty inventory slot as NBT with sorted keys, hunger,
//! saturation, exhaustion and cooldowns (sorted); plus the number of entities in the world.
//!
//! `KVASIR_DUMP=<file>` appends every recorded tick as text, to find what changed.
//!
//! Not seen: packets (sounds, particles, effect and health updates), advancements (no
//! advancement data is loaded), a plugin cancelling anything (the real server loads none).

use std::borrow::Cow;
use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

use pumpkin_data::Block;
use pumpkin_data::data_component::DataComponent;
use pumpkin_data::data_component_impl::{
    DataComponentImpl, OminousBottleAmplifierImpl, PotionContentsImpl, SuspiciousStewEffect,
    SuspiciousStewEffectsImpl, UseCooldownImpl,
};
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::potion::{Effect, Potion};
use pumpkin_inventory::Inventory;
use pumpkin_inventory::player::player_inventory::PlayerInventory;
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::{GameMode, Hand};
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use super::LivingEntity;
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::player::Player;
use crate::entity::{Entity, EntityBase};
use crate::server::Server;
use crate::world::World;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const FLOOR_Y: i32 = 64;
const TICKS: usize = 40;

/// FNV-1a over `text`, continuing `hash`.
fn fnv(hash: u64, text: &str) -> u64 {
    let mut hash = hash;
    for byte in text.bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// The item a player scene has in use.
#[derive(Clone, Copy)]
enum Use {
    Plain(&'static Item),
    Potion(&'static str),
    OminousBottle(i32),
    SuspiciousStew,
    Cooldown(Option<&'static str>),
}

impl Use {
    fn stack(self, count: u8) -> ItemStack {
        match self {
            Self::Plain(item) => ItemStack::new(count, item),
            Self::Potion(name) => {
                let mut stack = ItemStack::new(count, &Item::POTION);
                let potion_id = Potion::from_name(name).map(|potion| i32::from(potion.id));
                stack.patch.push((
                    DataComponent::PotionContents,
                    Some(
                        PotionContentsImpl {
                            potion_id,
                            custom_color: None,
                            custom_effects: Vec::new(),
                            custom_name: None,
                        }
                        .to_dyn(),
                    ),
                ));
                stack
            }
            Self::OminousBottle(amplifier) => {
                let mut stack = ItemStack::new(count, &Item::OMINOUS_BOTTLE);
                stack.patch.push((
                    DataComponent::OminousBottleAmplifier,
                    Some(OminousBottleAmplifierImpl { amplifier }.to_dyn()),
                ));
                stack
            }
            Self::SuspiciousStew => {
                let mut stack = ItemStack::new(count, &Item::SUSPICIOUS_STEW);
                stack.patch.push((
                    DataComponent::SuspiciousStewEffects,
                    Some(
                        SuspiciousStewEffectsImpl {
                            effects: Cow::Owned(vec![
                                SuspiciousStewEffect {
                                    effect: Cow::Borrowed("minecraft:night_vision"),
                                    duration: 100,
                                },
                                SuspiciousStewEffect {
                                    effect: Cow::Borrowed("minecraft:not_an_effect"),
                                    duration: 100,
                                },
                            ]),
                        }
                        .to_dyn(),
                    ),
                ));
                stack
            }
            Self::Cooldown(group) => {
                let mut stack = ItemStack::new(count, &Item::BREAD);
                stack.patch.push((
                    DataComponent::UseCooldown,
                    Some(UseCooldownImpl::new(1.5, group.map(str::to_owned)).to_dyn()),
                ));
                stack
            }
        }
    }
}

/// Where the stack in use sits.
#[derive(Clone, Copy)]
enum Place {
    /// The selected hotbar slot.
    Held,
    OffHand,
    /// Slot 9 of the main inventory; the selected slot holds dirt, `active_hand` says `hand`.
    Elsewhere(Option<Hand>),
}

/// What a scene ticks.
#[derive(Clone, Copy)]
enum Subject {
    Player {
        gamemode: GameMode,
        /// The item in use, its place, its count and the ticks until it finishes.
        using: Option<(Use, Place, u8, i32)>,
        /// Fill every empty main-inventory slot with stone.
        full: bool,
    },
    Zombie {
        velocity: (f64, f64, f64),
    },
}

/// Extra state set before the first tick.
#[derive(Clone, Copy)]
enum Extra {
    None,
    /// Hurt cooldown 5, hurt by a player 3 ticks ago.
    HurtCounters,
    Dead,
    /// Stands 150 blocks below the floor (below the world).
    BelowWorld,
    /// Stands inside a stone block.
    InStone,
}

struct Scene {
    name: &'static str,
    subject: Subject,
    floor: &'static Block,
    extra: Extra,
}

const fn player(
    name: &'static str,
    gamemode: GameMode,
    using: (Use, Place, u8, i32),
    full: bool,
) -> Scene {
    Scene {
        name,
        subject: Subject::Player {
            gamemode,
            using: Some(using),
            full,
        },
        floor: &Block::STONE,
        extra: Extra::None,
    }
}

const fn eat(name: &'static str, item: Use) -> Scene {
    player(name, GameMode::Survival, (item, Place::Held, 1, 1), false)
}

const S: GameMode = GameMode::Survival;
const C: GameMode = GameMode::Creative;
const BREAD: Use = Use::Plain(&Item::BREAD);
const STEW: Use = Use::Plain(&Item::MUSHROOM_STEW);
const MILK: Use = Use::Plain(&Item::MILK_BUCKET);
const HONEY: Use = Use::Plain(&Item::HONEY_BOTTLE);

const SCENES: &[Scene] = &[
    eat("bread", BREAD),
    eat("cooked_beef", Use::Plain(&Item::COOKED_BEEF)),
    eat("cookie", Use::Plain(&Item::COOKIE)),
    eat("dried_kelp", Use::Plain(&Item::DRIED_KELP)),
    eat("golden_apple", Use::Plain(&Item::GOLDEN_APPLE)),
    eat(
        "enchanted_golden_apple",
        Use::Plain(&Item::ENCHANTED_GOLDEN_APPLE),
    ),
    eat("spider_eye", Use::Plain(&Item::SPIDER_EYE)),
    eat("pufferfish", Use::Plain(&Item::PUFFERFISH)),
    eat("honey_bottle", HONEY),
    eat("milk_bucket", MILK),
    eat("mushroom_stew", STEW),
    eat("beetroot_soup", Use::Plain(&Item::BEETROOT_SOUP)),
    eat("rabbit_stew", Use::Plain(&Item::RABBIT_STEW)),
    eat("suspicious_stew", Use::SuspiciousStew),
    eat("potion_regeneration", Use::Potion("regeneration")),
    eat("potion_strong_healing", Use::Potion("strong_healing")),
    eat("potion_water", Use::Potion("water")),
    eat("ominous_bottle_2", Use::OminousBottle(2)),
    eat("ominous_bottle_9", Use::OminousBottle(9)),
    eat("shield", Use::Plain(&Item::SHIELD)),
    eat("cooldown_own_group", Use::Cooldown(None)),
    eat(
        "cooldown_named_group",
        Use::Cooldown(Some("minecraft:snacks")),
    ),
    player("bread_x16", S, (BREAD, Place::Held, 16, 1), false),
    player("bread_3_ticks", S, (BREAD, Place::Held, 1, 3), false),
    player("bread_off_hand", S, (BREAD, Place::OffHand, 1, 1), false),
    player(
        "bread_elsewhere_right",
        S,
        (BREAD, Place::Elsewhere(Some(Hand::Right)), 1, 1),
        false,
    ),
    player(
        "bread_elsewhere_left",
        S,
        (BREAD, Place::Elsewhere(Some(Hand::Left)), 1, 1),
        false,
    ),
    player(
        "bread_elsewhere_none",
        S,
        (BREAD, Place::Elsewhere(None), 1, 1),
        false,
    ),
    player("bread_creative", C, (BREAD, Place::Held, 1, 1), false),
    player("stew_x16", S, (STEW, Place::Held, 16, 1), false),
    player("stew_x16_full", S, (STEW, Place::Held, 16, 1), true),
    player("stew_off_hand", S, (STEW, Place::OffHand, 1, 1), false),
    player("stew_creative", C, (STEW, Place::Held, 1, 1), false),
    player("milk_x16_full", S, (MILK, Place::Held, 16, 1), true),
    player("milk_creative", C, (MILK, Place::Held, 1, 1), false),
    player("honey_x16_full", S, (HONEY, Place::Held, 16, 1), true),
    player("honey_off_hand", S, (HONEY, Place::OffHand, 1, 3), false),
    Scene {
        name: "player_idle",
        subject: Subject::Player {
            gamemode: S,
            using: None,
            full: false,
        },
        floor: &Block::STONE,
        extra: Extra::None,
    },
    Scene {
        name: "player_hurt_counters",
        subject: Subject::Player {
            gamemode: S,
            using: None,
            full: false,
        },
        floor: &Block::STONE,
        extra: Extra::HurtCounters,
    },
    Scene {
        name: "player_dead",
        subject: Subject::Player {
            gamemode: S,
            using: Some((BREAD, Place::Held, 1, 1)),
            full: false,
        },
        floor: &Block::STONE,
        extra: Extra::Dead,
    },
    Scene {
        name: "player_on_magma",
        subject: Subject::Player {
            gamemode: S,
            using: None,
            full: false,
        },
        floor: &Block::MAGMA_BLOCK,
        extra: Extra::None,
    },
    Scene {
        name: "player_in_stone",
        subject: Subject::Player {
            gamemode: S,
            using: None,
            full: false,
        },
        floor: &Block::STONE,
        extra: Extra::InStone,
    },
    Scene {
        name: "zombie_sliding",
        subject: Subject::Zombie {
            velocity: (0.4, 0.0, 0.1),
        },
        floor: &Block::STONE,
        extra: Extra::None,
    },
    Scene {
        name: "zombie_on_magma",
        subject: Subject::Zombie {
            velocity: (0.0, 0.0, 0.0),
        },
        floor: &Block::MAGMA_BLOCK,
        extra: Extra::None,
    },
    Scene {
        name: "zombie_hurt_counters",
        subject: Subject::Zombie {
            velocity: (0.0, 0.0, 0.0),
        },
        floor: &Block::STONE,
        extra: Extra::HurtCounters,
    },
    Scene {
        name: "zombie_dead",
        subject: Subject::Zombie {
            velocity: (0.2, 0.3, 0.0),
        },
        floor: &Block::STONE,
        extra: Extra::Dead,
    },
    Scene {
        name: "zombie_below_world",
        subject: Subject::Zombie {
            velocity: (0.0, 0.0, 0.0),
        },
        floor: &Block::STONE,
        extra: Extra::BelowWorld,
    },
    Scene {
        name: "zombie_in_stone",
        subject: Subject::Zombie {
            velocity: (0.0, 0.0, 0.0),
        },
        floor: &Block::STONE,
        extra: Extra::InStone,
    },
];

/// The entity a scene ticks.
enum Ticked {
    Player {
        player: Arc<Player>,
        /// Kept so the player's outgoing queue stays open; nothing reads it.
        _tap: crate::net::java::PacketTap,
    },
    Zombie(Arc<LivingEntity>),
}

impl Ticked {
    fn living(&self) -> &LivingEntity {
        match self {
            Self::Player { player, .. } => &player.living_entity,
            Self::Zombie(zombie) => zombie,
        }
    }

    fn caller(&self) -> &dyn EntityBase {
        match self {
            Self::Player { player, .. } => player.as_ref(),
            Self::Zombie(zombie) => zombie.as_ref(),
        }
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
) -> Result<Ticked, String> {
    match scene.subject {
        Subject::Player {
            gamemode,
            using,
            full,
        } => {
            let who = Identity {
                name: "LivingTick",
                uuid: Uuid::from_u128(0x4c69_7669_6e67_0000 + index as u128),
                entity_id: 50_000 + index as i32,
            };
            let (player, tap) = test_support::java_player_as(world, gamemode, &who)
                .ok_or("the test player's client no longer owned its outgoing queue")?;
            player.get_entity().set_pos(at);
            player.get_entity().on_ground.store(true, Relaxed);
            player.hunger_manager.level.store(6);
            player.hunger_manager.saturation.store(0.0);
            if full {
                for slot in 0..PlayerInventory::MAIN_SIZE {
                    player
                        .inventory
                        .set_stack(slot, ItemStack::new(64, &Item::STONE));
                }
            }
            if let Some((item, place, count, duration)) = using {
                let stack = item.stack(count);
                let hand = match place {
                    Place::Held => {
                        player.inventory.set_held_item(stack.clone());
                        Hand::Right
                    }
                    Place::OffHand => {
                        player
                            .inventory
                            .set_stack(PlayerInventory::OFF_HAND_SLOT, stack.clone());
                        Hand::Left
                    }
                    Place::Elsewhere(_) => {
                        player
                            .inventory
                            .set_held_item(ItemStack::new(3, &Item::DIRT));
                        player.inventory.set_stack(9, stack.clone());
                        Hand::Right
                    }
                };
                player.living_entity.set_active_hand(hand, stack, duration);
                if let Place::Elsewhere(active) = place {
                    *player
                        .living_entity
                        .active_hand
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = active;
                }
            }
            Ok(Ticked::Player { player, _tap: tap })
        }
        Subject::Zombie { velocity } => {
            let zombie = Arc::new(LivingEntity::new(Entity::new(
                Arc::clone(world),
                at,
                &EntityType::ZOMBIE,
            )));
            zombie
                .entity
                .velocity
                .store(Vector3::new(velocity.0, velocity.1, velocity.2));
            zombie.entity.on_ground.store(true, Relaxed);
            world.spawn_entity(zombie.clone());
            Ok(Ticked::Zombie(zombie))
        }
    }
}

fn set_up(ticked: &Ticked, extra: Extra) {
    let living = ticked.living();
    living.add_effect(Effect {
        effect_type: &StatusEffect::POISON,
        duration: 60,
        amplifier: 1,
        ambient: false,
        show_particles: true,
        show_icon: true,
        blend: false,
    });
    living.add_effect(Effect {
        effect_type: &StatusEffect::SPEED,
        duration: 30,
        amplifier: 0,
        ambient: false,
        show_particles: true,
        show_icon: true,
        blend: false,
    });
    match extra {
        Extra::None | Extra::BelowWorld | Extra::InStone => {}
        Extra::HurtCounters => {
            living.hurt_cooldown.store(5, Relaxed);
            living
                .last_hurt_by_player
                .store(Some(Uuid::from_u128(0x7e57)));
            living.last_hurt_by_player_memory_time.store(3, Relaxed);
        }
        Extra::Dead => living.health.store(0.0),
    }
}

/// NBT as text with the compound keys sorted (a compound is a `HashMap`).
fn nbt_text(tag: &NbtTag, text: &mut String) {
    match tag {
        NbtTag::Compound(compound) => compound_text(compound, text),
        NbtTag::List(items) => {
            text.push('[');
            for item in items {
                nbt_text(item, text);
                text.push(',');
            }
            text.push(']');
        }
        other => {
            let _ = write!(text, "{other:?}");
        }
    }
}

fn compound_text(compound: &NbtCompound, text: &mut String) {
    let mut keys: Vec<&str> = compound.child_tags.keys().map(AsRef::as_ref).collect();
    keys.sort_unstable();
    text.push('{');
    for key in keys {
        let _ = write!(text, "{key}:");
        if let Some(tag) = compound.child_tags.get(key) {
            nbt_text(tag, text);
        }
        text.push(',');
    }
    text.push('}');
}

fn stack_text(stack: &ItemStack, text: &mut String) {
    let mut nbt = NbtCompound::new();
    stack.write_item_stack(&mut nbt);
    compound_text(&nbt, text);
}

fn living_text(living: &LivingEntity, text: &mut String) {
    let entity = &living.entity;
    let _ = write!(
        text,
        "{:?} {:?} ground {} health {:?} death {} removed {} hurt {} by {:?} {} effects",
        entity.pos.load(),
        entity.velocity.load(),
        entity.on_ground.load(Relaxed),
        living.health.load(),
        living.death_time.load(Relaxed),
        entity.is_removed(),
        living.hurt_cooldown.load(Relaxed),
        living.last_hurt_by_player.load(),
        living.last_hurt_by_player_memory_time.load(Relaxed),
    );
    let effects = living
        .active_effects
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut effects: Vec<_> = effects.values().collect();
    effects.sort_by_key(|effect| effect.effect_type.id);
    for effect in effects {
        let _ = write!(
            text,
            " {}:{}:{}",
            effect.effect_type.minecraft_name, effect.amplifier, effect.duration
        );
    }
}

fn player_text(player: &Player, text: &mut String) {
    let living = &player.living_entity;
    let _ = write!(
        text,
        " use {} {:?} in_use ",
        living.item_use_time.load(Relaxed),
        *living
            .active_hand
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    if let Some(stack) = living
        .item_in_use
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
    {
        stack_text(stack, text);
    }
    let _ = write!(
        text,
        " food {} {:?} {:?} slots",
        player.hunger_manager.level.load(),
        player.hunger_manager.saturation.load(),
        player.hunger_manager.exhaustion.load(),
    );
    for slot in 0..player.inventory.size() {
        let stack = player.inventory.get_slot(slot);
        if !stack.is_empty() {
            let _ = write!(text, " {slot}=");
            stack_text(&stack, text);
        }
    }
    text.push_str(" cooldowns");
    let cooldowns = player
        .item_cooldowns
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut cooldowns: Vec<_> = cooldowns.iter().collect();
    cooldowns.sort_by_key(|(group, _)| group.as_str());
    for (group, cooldown) in cooldowns {
        let _ = write!(
            text,
            " {group}:{}+{}",
            cooldown.start_tick, cooldown.duration
        );
    }
}

/// A short readable end state for the scene's summary line.
fn summary(ticked: &Ticked) -> String {
    let living = ticked.living();
    let mut text = format!(
        "health {:.2} death {}{}",
        living.health.load(),
        living.death_time.load(Relaxed),
        if living.entity.is_removed() {
            " removed"
        } else {
            ""
        },
    );
    if let Ticked::Player { player, .. } = ticked {
        let held = player.inventory.held_item();
        let _ = write!(
            text,
            " food {} held {} {}",
            player.hunger_manager.level.load(),
            held.item.registry_key,
            held.item_count,
        );
    }
    text
}

fn run_scene(
    server: &Arc<Server>,
    world: &Arc<World>,
    index: usize,
    scene: &Scene,
    report: &mut String,
) -> TestResult {
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
            place_block(world, BlockPos::new(x + dx, FLOOR_Y, z + dz), scene.floor);
        }
    }
    let feet_y = match scene.extra {
        Extra::BelowWorld => FLOOR_Y - 150,
        _ => FLOOR_Y + 1,
    };
    if matches!(scene.extra, Extra::InStone) {
        place_block(world, BlockPos::new(x, FLOOR_Y + 1, z), &Block::STONE);
        place_block(world, BlockPos::new(x, FLOOR_Y + 2, z), &Block::STONE);
    }
    let at = Vector3::new(f64::from(x) + 0.5, f64::from(feet_y), f64::from(z) + 0.5);
    let ticked = spawn(world, index, scene, at)?;
    set_up(&ticked, scene.extra);

    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for tick in 1..=TICKS {
        if !ticked.living().entity.is_removed() {
            EntityBase::tick(ticked.living(), ticked.caller(), server);
        }
        let mut text = String::new();
        living_text(ticked.living(), &mut text);
        if let Ticked::Player { player, .. } = &ticked {
            player_text(player, &mut text);
        }
        let _ = write!(text, " entities {}", world.entities.load().len());
        if let Some(dump) = std::env::var_os("KVASIR_DUMP") {
            use std::io::Write as _;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dump)?;
            writeln!(file, "{} {tick} {text}", scene.name)?;
        }
        hash = fnv(hash, &text);
        if tick % 10 == 0 {
            let _ = writeln!(
                report,
                "{} ticks {}-{tick}: {hash:016x}",
                scene.name,
                tick - 9
            );
        }
    }
    let _ = writeln!(report, "{} end: {}", scene.name, summary(&ticked));

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
async fn living_tick() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server
        .worlds
        .load()
        .first()
        .cloned()
        .ok_or("the server has no world")?;
    let mut report = String::new();
    for (index, scene) in SCENES.iter().enumerate() {
        run_scene(&server, &world, index, scene, &mut report)?;
    }
    server.stop();

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("living_tick.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
