//! A record of `Raid::tick` (`tests/golden/raid_tick.txt`; rewrite it on purpose with
//! `KVASIR_BLESS=1`), made before `tick` was split.
//!
//! Each scene builds a raid (status, counters, cooldown, omen level, spawn position, raiders
//! that are real pillagers in the world) beside three listeners: `GoldA` and `GoldC` near the
//! raid centre, `GoldB` far away. It ticks the raid and records a line whenever the raid's
//! state, a player's effect, statistic or advancement, or a packet sent to a player changed
//! (counters that move every tick are printed on those lines and on the end line). Random
//! parts are not recorded raw: a spawned group prints only that raiders appeared (and where,
//! when the scene fixed the spawn position), sounds print as `sound` unless the position is
//! fixed, and at most three raiders are counted.

use std::fmt::Write;
use std::sync::Arc;

use pumpkin_data::effect::StatusEffect;
use pumpkin_data::entity::EntityType;
use pumpkin_data::packet::CURRENT_MC_VERSION;
use pumpkin_data::statistic::CustomStatistic;
use pumpkin_protocol::java::client::play::{
    CBossEvent, CSoundEffect, CUpdateAdvancements, CUpdateMobEffect,
};
use pumpkin_protocol::packet::MultiVersionJavaPacket;
use pumpkin_util::GameMode;
use pumpkin_util::difficulty::Difficulty;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::text::TextContent;
use uuid::Uuid;

use super::{Raid, RaidStatus};
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::EntityBase;
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::player::Player;
use crate::entity::r#type::from_type;
use crate::net::java::PacketTap;
use crate::world::World;

/// What a scene does at a tick, before the raid ticks.
#[derive(Clone, Copy)]
enum Act {
    /// Kills the first `n` raiders that are still alive.
    Kill(usize),
    /// Moves the player (index) far from the raid.
    Far(usize),
    /// Moves the player (index) next to the raid centre.
    Near(usize),
    Edit(fn(&mut Raid)),
}

struct Scene {
    name: &'static str,
    difficulty: Difficulty,
    setup: fn(&mut Raid),
    raiders: usize,
    far: &'static [usize],
    heroes: &'static [usize],
    /// A hero that is not in the world.
    hero_absent: bool,
    in_raid: &'static [usize],
    ticks: u32,
    script: &'static [(u32, Act)],
    fixed_pos: bool,
}

const fn scene(name: &'static str, ticks: u32) -> Scene {
    Scene {
        name,
        difficulty: Difficulty::Normal,
        setup: |_| {},
        raiders: 0,
        far: &[1],
        heroes: &[],
        hero_absent: false,
        in_raid: &[],
        ticks,
        script: &[],
        fixed_pos: false,
    }
}

impl Scene {
    const fn setup(mut self, setup: fn(&mut Raid)) -> Self {
        self.setup = setup;
        self
    }
    const fn raiders(mut self, raiders: usize) -> Self {
        self.raiders = raiders;
        self
    }
    const fn far(mut self, far: &'static [usize]) -> Self {
        self.far = far;
        self
    }
    const fn heroes(mut self, heroes: &'static [usize]) -> Self {
        self.heroes = heroes;
        self
    }
    const fn hero_absent(mut self) -> Self {
        self.hero_absent = true;
        self
    }
    const fn in_raid(mut self, in_raid: &'static [usize]) -> Self {
        self.in_raid = in_raid;
        self
    }
    const fn peaceful(mut self) -> Self {
        self.difficulty = Difficulty::Peaceful;
        self
    }
    const fn at(mut self, script: &'static [(u32, Act)]) -> Self {
        self.script = script;
        self
    }
    const fn fixed_pos(mut self) -> Self {
        self.fixed_pos = true;
        self
    }
}

fn countdown_scenes() -> Vec<Scene> {
    vec![
        scene("stopped", 3).setup(|r| {
            r.status = RaidStatus::Stopped;
            r.raid_cooldown_ticks = 5;
        }),
        scene("peaceful stops", 3)
            .peaceful()
            .in_raid(&[0, 1])
            .far(&[1]),
        scene("timeout boundary", 4)
            .setup(|r| r.ticks_active = 47_998)
            .in_raid(&[0]),
        scene("countdown from 297", 24).setup(|r| r.raid_cooldown_ticks = 297),
        scene("countdown from 300", 3).setup(|r| r.raid_cooldown_ticks = 300),
        scene("countdown from 301", 3).setup(|r| r.raid_cooldown_ticks = 301),
        scene("countdown from 310", 14).setup(|r| r.raid_cooldown_ticks = 310),
        scene("spawn position kept", 6).setup(|r| {
            r.raid_cooldown_ticks = 10;
            r.wave_spawn_pos = Some(BlockPos::new(r.center.0.x + 10, 64, r.center.0.z + 10));
        }),
        scene("spawn position found at a multiple of five", 3).setup(|r| r.raid_cooldown_ticks = 5),
        scene("spawn position not found off a multiple", 3).setup(|r| r.raid_cooldown_ticks = 4),
        scene("cooldown at the update step", 25)
            .setup(|r| r.raid_cooldown_ticks = 41)
            .far(&[1, 2]),
    ]
}

fn cooldown_reset_scenes() -> Vec<Scene> {
    vec![
        scene("cooldown zero after a wave", 3).setup(|r| {
            r.groups_spawned = 1;
            r.raid_cooldown_ticks = 0;
            r.started = true;
            r.bossbar.title = pumpkin_util::text::TextComponent::text("custom");
        }),
        scene("cooldown negative after a wave", 3).setup(|r| {
            r.groups_spawned = 2;
            r.raid_cooldown_ticks = -5;
            r.started = true;
            r.bossbar.title = pumpkin_util::text::TextComponent::text("custom");
        }),
        scene("cooldown negative, no wave yet", 3).setup(|r| {
            r.raid_cooldown_ticks = -1;
            r.bossbar.title = pumpkin_util::text::TextComponent::text("custom");
        }),
        scene("raiders alive keep the cooldown", 3)
            .setup(|r| {
                r.groups_spawned = 1;
                r.raid_cooldown_ticks = 0;
                r.started = true;
            })
            .raiders(2),
        scene("raiders alive, cooldown running", 3)
            .setup(|r| {
                r.groups_spawned = 1;
                r.raid_cooldown_ticks = 50;
                r.started = true;
            })
            .raiders(1),
        scene("no more waves, cooldown running", 3).setup(|r| {
            r.groups_spawned = 5;
            r.raid_cooldown_ticks = 50;
            r.started = true;
        }),
        scene("bonus wave pending, cooldown running", 3).setup(|r| {
            r.groups_spawned = 5;
            r.raid_cooldown_ticks = 50;
            r.raid_omen_level = 3;
            r.started = true;
        }),
    ]
}

fn spawn_scenes() -> Vec<Scene> {
    vec![
        scene("first group at cooldown zero", 2).setup(|r| r.raid_cooldown_ticks = 0),
        scene("second group at a fixed position", 3)
            .setup(|r| {
                r.groups_spawned = 1;
                r.raid_cooldown_ticks = 2;
                r.started = true;
                r.wave_spawn_pos = Some(BlockPos::new(r.center.0.x + 10, 64, r.center.0.z + 10));
            })
            .fixed_pos(),
        scene("spawn after the countdown", 4).setup(|r| {
            r.groups_spawned = 1;
            r.raid_cooldown_ticks = 3;
            r.started = true;
        }),
        scene("last group", 2).setup(|r| {
            r.groups_spawned = 4;
            r.raid_cooldown_ticks = 1;
            r.started = true;
        }),
        scene("bonus group", 3)
            .setup(|r| {
                r.groups_spawned = 5;
                r.raid_cooldown_ticks = 1;
                r.raid_omen_level = 3;
                r.started = true;
                r.wave_spawn_pos = Some(BlockPos::new(r.center.0.x - 6, 64, r.center.0.z + 6));
            })
            .fixed_pos(),
        scene("no bonus group without omen", 3).setup(|r| {
            r.groups_spawned = 5;
            r.raid_cooldown_ticks = 1;
            r.started = true;
        }),
        scene("spawn with nobody near", 2)
            .setup(|r| r.raid_cooldown_ticks = 0)
            .far(&[0, 1, 2]),
    ]
}

fn update_scenes() -> Vec<Scene> {
    let alive = |name: &'static str, raiders: usize| {
        scene(name, 2)
            .setup(|r| {
                r.ticks_active = 18;
                r.raid_cooldown_ticks = 0;
                r.groups_spawned = 1;
                r.started = true;
            })
            .raiders(raiders)
    };
    vec![
        alive("update step, no raiders", 0).setup(|r| {
            r.ticks_active = 18;
            r.groups_spawned = 5;
            r.started = true;
        }),
        alive("update step, one raider", 1),
        alive("update step, two raiders", 2),
        alive("update step, three raiders", 3),
        alive("update step, a raider dies", 4).at(&[(2, Act::Kill(1))]),
        alive("update step, raiders die one by one", 3).at(&[(1, Act::Kill(1)), (2, Act::Kill(1))]),
        alive("update step at forty", 2)
            .setup(|r| {
                r.ticks_active = 38;
                r.groups_spawned = 1;
                r.raid_cooldown_ticks = 0;
                r.started = true;
            })
            .raiders(2)
            .in_raid(&[0]),
        alive("player leaves at the update step", 2)
            .in_raid(&[0])
            .at(&[(1, Act::Far(0))]),
        alive("player arrives at the update step", 2)
            .far(&[0, 1])
            .at(&[(1, Act::Near(0))]),
    ]
}

fn victory_scenes() -> Vec<Scene> {
    let won = |name: &'static str, ticks: u32| {
        scene(name, ticks).setup(|r| {
            r.groups_spawned = 5;
            r.started = true;
            r.post_raid_ticks = 38;
        })
    };
    vec![
        won("victory after forty quiet ticks", 6)
            .heroes(&[0, 1])
            .hero_absent(),
        won("victory, omen three", 4)
            .setup(|r| {
                r.groups_spawned = 6;
                r.started = true;
                r.post_raid_ticks = 40;
                r.raid_omen_level = 3;
            })
            .heroes(&[0]),
        won("victory, omen zero", 2)
            .setup(|r| {
                r.groups_spawned = 5;
                r.started = true;
                r.post_raid_ticks = 40;
                r.raid_omen_level = 0;
            })
            .heroes(&[2]),
        won("victory, omen five", 2)
            .setup(|r| {
                r.groups_spawned = 6;
                r.started = true;
                r.post_raid_ticks = 40;
                r.raid_omen_level = 5;
            })
            .heroes(&[0]),
        won("no victory before the raid started", 3).setup(|r| {
            r.groups_spawned = 5;
            r.post_raid_ticks = 38;
        }),
        won("no victory with a raider left", 3).raiders(1),
        won("no victory, bonus wave not spawned", 3).setup(|r| {
            r.groups_spawned = 5;
            r.started = true;
            r.post_raid_ticks = 38;
            r.raid_omen_level = 3;
            r.raid_cooldown_ticks = 40;
        }),
        won("raiders die into victory", 6)
            .setup(|r| {
                r.groups_spawned = 5;
                r.started = true;
                r.post_raid_ticks = 38;
                r.ticks_active = 17;
            })
            .raiders(2)
            .heroes(&[0])
            .at(&[(3, Act::Kill(2))]),
        won("victory with nobody to reward", 3).heroes(&[]),
    ]
}

fn celebration_scenes() -> Vec<Scene> {
    let over = |name: &'static str, ticks: u32| scene(name, ticks).in_raid(&[0]);
    vec![
        over("victory celebration ends", 3)
            .setup(|r| {
                r.status = RaidStatus::Victory;
                r.celebration_ticks = 597;
            })
            .in_raid(&[0, 1]),
        over("loss celebration ends", 3).setup(|r| {
            r.status = RaidStatus::Loss;
            r.celebration_ticks = 598;
        }),
        over("celebration one before the end", 2).setup(|r| {
            r.status = RaidStatus::Victory;
            r.celebration_ticks = 598;
        }),
        over("victory title at the update step", 3)
            .setup(|r| {
                r.status = RaidStatus::Victory;
                r.celebration_ticks = 18;
            })
            .in_raid(&[])
            .raiders(2),
        over("victory title resets the progress", 3)
            .setup(|r| {
                r.status = RaidStatus::Victory;
                r.bossbar.health = 0.75;
                r.celebration_ticks = 18;
            })
            .in_raid(&[0]),
        over("loss title at the update step", 3)
            .setup(|r| {
                r.status = RaidStatus::Loss;
                r.celebration_ticks = 18;
            })
            .in_raid(&[]),
        over("celebration at forty", 2).setup(|r| {
            r.status = RaidStatus::Victory;
            r.celebration_ticks = 38;
        }),
        over("celebration, player leaves", 2)
            .setup(|r| {
                r.status = RaidStatus::Loss;
                r.celebration_ticks = 19;
            })
            .at(&[(1, Act::Far(0))]),
        over("celebration between updates", 3).setup(|r| {
            r.status = RaidStatus::Victory;
            r.celebration_ticks = 4;
        }),
        over("ongoing, then lost by edit", 4)
            .setup(|r| {
                r.groups_spawned = 1;
                r.raid_cooldown_ticks = 50;
                r.started = true;
            })
            .at(&[(2, Act::Edit(|r| r.status = RaidStatus::Loss))]),
    ]
}

fn scenes() -> Vec<Scene> {
    let mut all = countdown_scenes();
    all.extend(cooldown_reset_scenes());
    all.extend(spawn_scenes());
    all.extend(update_scenes());
    all.extend(victory_scenes());
    all.extend(celebration_scenes());
    all
}

struct Listener {
    who: &'static Identity,
    player: Arc<Player>,
    tap: PacketTap,
}

fn title_key(raid: &Raid) -> String {
    match &*raid.bossbar.title.0.content {
        TextContent::Translate { translate, .. } => translate.to_string(),
        TextContent::Text { text } => text.to_string(),
        _ => "other".to_string(),
    }
}

fn names(raid: &Raid) -> String {
    let mut names: Vec<String> = raid
        .players_in_raid
        .iter()
        .map(script_golden::name_of)
        .collect();
    names.sort();
    names.join(",")
}

fn wave_pos(raid: &Raid, preset: Option<BlockPos>) -> &'static str {
    match raid.wave_spawn_pos {
        None => "none",
        pos if pos == preset => "preset",
        Some(_) => "found",
    }
}

/// The raid's state without the counters that move every tick.
fn key(raid: &Raid, preset: Option<BlockPos>) -> String {
    format!(
        "{:?} active {} started {} groups {} omen {} wave_pos {} alive {} title {} in_raid [{}]",
        raid.status,
        raid.active,
        raid.started,
        raid.groups_spawned,
        raid.raid_omen_level,
        wave_pos(raid, preset),
        raid.get_total_raiders_alive().min(3),
        title_key(raid),
        names(raid),
    )
}

fn counters(raid: &Raid) -> String {
    format!(
        "ticks {} cd {} post {} cel {} health {:.3}",
        raid.ticks_active,
        raid.raid_cooldown_ticks,
        raid.post_raid_ticks,
        raid.celebration_ticks,
        raid.bossbar.health,
    )
}

/// Effect, statistic and advancement of each player that has any.
fn rewards(listeners: &[Listener]) -> String {
    let mut text = String::new();
    for listener in listeners {
        let player = &listener.player;
        let effect = player
            .get_effect(&StatusEffect::HERO_OF_THE_VILLAGE)
            .map_or_else(String::new, |e| {
                format!(
                    " effect {}/{}/{}/{}/{}{}",
                    e.amplifier,
                    e.duration,
                    e.ambient,
                    e.show_particles,
                    e.show_icon,
                    if e.blend { "" } else { "/noblend" }
                )
            });
        let stat = player.get_custom_stat(CustomStatistic::RaidWin);
        let advancements = player
            .advancements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let hero = advancements.progress_changed.iter().any(|advancement| {
            advancement.id.to_string() == "minecraft:adventure/hero_of_the_village"
        });
        let changed = advancements.progress_changed.len();
        if !effect.is_empty() || stat != 0 || changed != 0 {
            let _ = write!(
                text,
                " {}{{{effect} stat {stat} hero_advancement {hero} changed {changed}}}",
                listener.who.name
            );
        }
    }
    text
}

/// The packet's id and the bytes after it, whether or not it starts with its length.
fn split_packet(packet: &[u8]) -> Option<(i32, &[u8])> {
    let known = [
        CBossEvent::to_id(CURRENT_MC_VERSION),
        CSoundEffect::to_id(CURRENT_MC_VERSION),
        CUpdateMobEffect::to_id(CURRENT_MC_VERSION),
        CUpdateAdvancements::to_id(CURRENT_MC_VERSION),
    ];
    let (first, next) = script_golden::var_int(packet, 0)?;
    if known.contains(&first) {
        return Some((first, &packet[next..]));
    }
    let (second, end) = script_golden::var_int(packet, next)?;
    known.contains(&second).then(|| (second, &packet[end..]))
}

fn boss_text(body: &[u8]) -> String {
    let Some((action, at)) = script_golden::var_int(body, 16) else {
        return "boss?".to_string();
    };
    let rest = &body[at..];
    match action {
        0 => format!("boss+{:08x}", script_golden::fnv(rest) & 0xffff_ffff),
        1 => "boss-".to_string(),
        2 => {
            let bytes: [u8; 4] = rest
                .get(..4)
                .and_then(|b| b.try_into().ok())
                .unwrap_or([0; 4]);
            format!("boss={:.3}", f32::from_be_bytes(bytes))
        }
        3 => format!("boss~{:08x}", script_golden::fnv(rest) & 0xffff_ffff),
        other => format!("boss?{other}"),
    }
}

/// The packets a listener was sent since the last call.
fn packets(tap: &mut PacketTap, fixed_pos: bool) -> String {
    let mut parts = Vec::new();
    for packet in tap.drain() {
        let Some((id, body)) = split_packet(&packet) else {
            parts.push("other".to_string());
            continue;
        };
        parts.push(if id == CBossEvent::to_id(CURRENT_MC_VERSION) {
            boss_text(body)
        } else if id == CSoundEffect::to_id(CURRENT_MC_VERSION) {
            if fixed_pos && packet.len() >= 8 {
                let mut text = "sound ".to_string();
                for byte in &packet[..packet.len() - 8] {
                    let _ = write!(text, "{byte:02x}");
                }
                text
            } else {
                "sound".to_string()
            }
        } else if id == CUpdateMobEffect::to_id(CURRENT_MC_VERSION) {
            "effect".to_string()
        } else {
            "advancement".to_string()
        });
    }
    parts.join(" ")
}

fn sent(listeners: &mut [Listener], fixed_pos: bool) -> String {
    let mut text = String::new();
    for listener in listeners {
        let parts = packets(&mut listener.tap, fixed_pos);
        if !parts.is_empty() {
            let _ = write!(text, " {}[{parts}]", listener.who.name);
        }
    }
    text
}

fn place(listener: &Listener, center: BlockPos, near: bool) {
    let dx = if near { 3.0 } else { 500.0 };
    listener.player.get_entity().set_pos(Vector3::new(
        f64::from(center.0.x) + dx,
        64.0,
        f64::from(center.0.z) + 3.0,
    ));
}

fn reset(listener: &Listener) {
    let player = &listener.player;
    player.remove_effect(&StatusEffect::HERO_OF_THE_VILLAGE);
    player.set_custom_stat(CustomStatistic::RaidWin, 0);
    let mut advancements = player
        .advancements
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    advancements.progress.clear();
    advancements.progress_changed.clear();
}

fn add_raiders(world: &Arc<World>, raid: &mut Raid, count: usize) -> Vec<Arc<dyn EntityBase>> {
    let at = Vector3::new(
        f64::from(raid.center.0.x) + 2.0,
        65.0,
        f64::from(raid.center.0.z) + 2.0,
    );
    (0..count)
        .map(|_| {
            let uuid = Uuid::new_v4();
            let base = from_type(&EntityType::PILLAGER, at, world, uuid);
            raid.join_raid(1, uuid, &base);
            world.spawn_entity_non_save(base.clone());
            base
        })
        .collect()
}

fn act(act: &Act, raid: &mut Raid, raiders: &[Arc<dyn EntityBase>], listeners: &[Listener]) {
    match *act {
        Act::Kill(n) => {
            let alive = raiders
                .iter()
                .filter_map(|raider| raider.get_living_entity())
                .filter(|living| living.health.load() > 0.0);
            for living in alive.take(n) {
                living.health.store(0.0);
            }
        }
        Act::Far(index) => place(&listeners[index], raid.center, false),
        Act::Near(index) => place(&listeners[index], raid.center, true),
        Act::Edit(edit) => edit(raid),
    }
}

/// Where the raiders that appeared during the tick stand (when the scene fixed the spawn
/// position), or just that some appeared.
fn spawned(world: &World, before: usize, raid: &Raid, fixed_pos: bool) -> String {
    let entities = world.entities.load();
    let new = &entities[before.min(entities.len())..];
    if new.is_empty() {
        return String::new();
    }
    if !fixed_pos {
        let near = new.iter().all(|e| {
            let pos = e.get_entity().pos.load();
            (pos.x - f64::from(raid.center.0.x)).abs() <= 20.0
                && (pos.z - f64::from(raid.center.0.z)).abs() <= 20.0
        });
        return format!(" +spawned near {near}");
    }
    let mut at: Vec<String> = new
        .iter()
        .map(|e| {
            let pos = e.get_entity().pos.load();
            format!(
                "({:.1},{:.1},{:.1})",
                pos.x - f64::from(raid.center.0.x),
                pos.y,
                pos.z - f64::from(raid.center.0.z)
            )
        })
        .collect();
    at.sort();
    at.dedup();
    format!(" +spawned at {}", at.join(" "))
}

fn run(
    out: &mut String,
    world: &Arc<World>,
    listeners: &mut [Listener],
    index: usize,
    scene: &Scene,
) -> std::fmt::Result {
    let center = BlockPos::new(index as i32 * 1000, 64, 0);
    world.level_info.rcu(|info| {
        let mut next = (**info).clone();
        next.difficulty = scene.difficulty;
        Arc::new(next)
    });
    let mut raid = Raid::new(index as i32, center, scene.difficulty);
    for (slot, listener) in listeners.iter().enumerate() {
        place(listener, center, !scene.far.contains(&slot));
        reset(listener);
        if scene.heroes.contains(&slot) {
            raid.add_hero_of_the_village(listener.player.gameprofile.id);
        }
        if scene.in_raid.contains(&slot) {
            raid.players_in_raid.insert(listener.player.gameprofile.id);
        }
    }
    if scene.hero_absent {
        raid.add_hero_of_the_village(Uuid::from_u128(0xabcd));
    }
    let raiders = add_raiders(world, &mut raid, scene.raiders);
    (scene.setup)(&mut raid);
    let preset = raid.wave_spawn_pos;
    let _ = sent(listeners, scene.fixed_pos);
    let mut last = key(&raid, preset) + &rewards(listeners);
    writeln!(out, "{}: start {last} | {}", scene.name, counters(&raid))?;
    for tick in 1..=scene.ticks {
        for (at, step) in scene.script {
            if *at == tick {
                act(step, &mut raid, &raiders, listeners);
            }
        }
        let before = world.entities.load().len();
        raid.tick(world);
        let packets = sent(listeners, scene.fixed_pos);
        let now = key(&raid, preset) + &rewards(listeners);
        let spawn = spawned(world, before, &raid, scene.fixed_pos);
        if now != last || !packets.is_empty() || !spawn.is_empty() {
            writeln!(
                out,
                "{}: tick {tick} {now} | {}{spawn}{packets}",
                scene.name,
                counters(&raid)
            )?;
            last = now;
        }
    }
    writeln!(out, "{}: end {last} | {}", scene.name, counters(&raid))
}

#[tokio::test(flavor = "multi_thread")]
async fn raid_tick_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let scenes = scenes();
    let mut listeners = Vec::new();
    for who in [&script_golden::A, &script_golden::B, &script_golden::C] {
        let (player, tap) = test_support::java_player_as(&world, GameMode::Survival, who)
            .ok_or("a test player's client no longer owned its outgoing queue")?;
        world.add_player(&player)?;
        player
            .advancements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .player = Arc::downgrade(&player);
        listeners.push(Listener { who, player, tap });
    }
    for listener in &mut listeners {
        listener.tap.drain();
    }
    let mut now = String::new();
    for (index, scene) in scenes.iter().enumerate() {
        run(&mut now, &world, &mut listeners, index, scene)?;
    }
    script_golden::bless_or_compare("raid_tick.txt", &now)?;
    for listener in &listeners {
        world.remove_player(&listener.player, false).await;
    }
    server.stop();
    Ok(())
}
