//! A record of what `Player::read_custom_nbt` and `Player::write_custom_nbt` do, made before the
//! two functions were split (`tests/golden/player_nbt.txt`; rewrite it on purpose with
//! `KVASIR_BLESS=1`).
//!
//! Each scene builds a player save compound (modern fields, legacy fields, missing fields, out of
//! range values, byte-typed game modes, short arrays, list-typed vehicle ids, inventories), reads
//! it into a fresh survival player of a real server and writes the player back. One line per
//! scene: the written compound with sorted keys, plus the state the writer does not show
//! (experience points, sleep timer, invulnerable, no physics, on ground). `XpSeed` is random when
//! the input has none: it is printed as `random` then.

use std::fmt::Write;
use std::sync::atomic::Ordering::Relaxed;

use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;
use pumpkin_util::GameMode;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::EntityBase;
use crate::entity::ai::test_support;

fn compound(entries: Vec<(&str, NbtTag)>) -> NbtCompound {
    let mut nbt = NbtCompound::new();
    for (key, value) in entries {
        nbt.put(key, value);
    }
    nbt
}

fn int(value: i32) -> NbtTag {
    NbtTag::Int(value)
}

fn float(value: f32) -> NbtTag {
    NbtTag::Float(value)
}

fn byte(value: i8) -> NbtTag {
    NbtTag::Byte(value)
}

fn short(value: i16) -> NbtTag {
    NbtTag::Short(value)
}

fn text(value: &str) -> NbtTag {
    NbtTag::String(value.into())
}

fn item(slot: i8, id: &str, count: i32) -> NbtTag {
    NbtTag::Compound(compound(vec![
        ("Slot", byte(slot)),
        ("id", text(id)),
        ("count", int(count)),
    ]))
}

fn full() -> NbtCompound {
    compound(vec![
        ("XpP", float(0.5)),
        ("XpLevel", int(7)),
        ("XpTotal", int(100)),
        ("XpSeed", int(42)),
        ("Score", int(3)),
        ("SleepTimer", short(50)),
        ("playerGameType", int(1)),
        ("previousPlayerGameType", int(0)),
        ("seenCredits", byte(1)),
        (
            "warden_spawn_tracker",
            NbtTag::Compound(compound(vec![
                ("warning_level", int(2)),
                ("ticks_since_last_warning", int(10)),
                ("cooldown_ticks", int(5)),
            ])),
        ),
        ("spawn_extra_particles_on_fall", byte(1)),
        ("HasPlayedBefore", byte(1)),
        ("foodLevel", int(15)),
        ("foodSaturationLevel", float(2.5)),
        ("foodExhaustionLevel", float(1.5)),
        ("foodTickTimer", int(30)),
        ("Air", short(200)),
        ("DrowningTick", int(5)),
        (
            "respawn",
            NbtTag::Compound(compound(vec![
                ("dimension", text("minecraft:the_nether")),
                ("pos", NbtTag::IntArray(vec![1, 2, 3])),
                ("angle", float(90.0)),
                ("forced", byte(1)),
            ])),
        ),
        (
            "RootVehicle",
            NbtTag::Compound(compound(vec![(
                "Attach",
                NbtTag::IntArray(vec![1, 2, 3, 4]),
            )])),
        ),
        (
            "abilities",
            NbtTag::Compound(compound(vec![
                ("flying", byte(1)),
                ("mayfly", byte(1)),
                ("invulnerable", byte(0)),
                ("instabuild", byte(1)),
                ("mayBuild", byte(1)),
                ("flySpeed", float(0.1)),
                ("walkSpeed", float(0.2)),
            ])),
        ),
    ])
}

fn scenes() -> Vec<(&'static str, NbtCompound)> {
    let mut scenes = save_field_scenes();
    scenes.extend(ability_scenes());
    scenes.extend(state_scenes());
    scenes
}

/// Experience, spawn point and game mode number saves.
fn save_field_scenes() -> Vec<(&'static str, NbtCompound)> {
    vec![
        ("empty", NbtCompound::new()),
        ("full modern save", full()),
        (
            "experience total only",
            compound(vec![("XpTotal", int(100))]),
        ),
        (
            "experience level with almost full bar",
            compound(vec![("XpLevel", int(3)), ("XpP", float(0.999))]),
        ),
        (
            "experience level without bar",
            compound(vec![("XpLevel", int(30)), ("XpTotal", int(5))]),
        ),
        (
            "legacy spawn point",
            compound(vec![
                ("SpawnX", int(-5)),
                ("SpawnY", int(70)),
                ("SpawnZ", int(9)),
                ("SpawnDimension", text("minecraft:the_end")),
                ("SpawnForced", byte(1)),
            ]),
        ),
        (
            "legacy spawn point without dimension",
            compound(vec![
                ("SpawnX", int(1)),
                ("SpawnY", int(2)),
                ("SpawnZ", int(3)),
            ]),
        ),
        (
            "legacy spawn point missing z",
            compound(vec![("SpawnX", int(1)), ("SpawnY", int(2))]),
        ),
        (
            "respawn with a short position and an unknown dimension",
            compound(vec![(
                "respawn",
                NbtTag::Compound(compound(vec![
                    ("dimension", text("minecraft:nowhere")),
                    ("pos", NbtTag::IntArray(vec![1, 2])),
                ])),
            )]),
        ),
        (
            "respawn beside legacy fields",
            compound(vec![
                (
                    "respawn",
                    NbtTag::Compound(compound(vec![("pos", NbtTag::IntArray(vec![7, 8, 9]))])),
                ),
                ("SpawnX", int(1)),
                ("SpawnY", int(2)),
                ("SpawnZ", int(3)),
            ]),
        ),
        (
            "spectator as a byte",
            compound(vec![
                ("playerGameType", byte(3)),
                ("previousPlayerGameType", byte(2)),
            ]),
        ),
        (
            "unknown game modes",
            compound(vec![
                ("playerGameType", int(9)),
                ("previousPlayerGameType", int(-1)),
            ]),
        ),
    ]
}

/// Game modes against the abilities compound, and air.
fn ability_scenes() -> Vec<(&'static str, NbtCompound)> {
    vec![
        (
            "creative with abilities saying otherwise",
            compound(vec![
                ("playerGameType", int(1)),
                (
                    "abilities",
                    NbtTag::Compound(compound(vec![
                        ("invulnerable", byte(0)),
                        ("mayfly", byte(0)),
                        ("instabuild", byte(0)),
                    ])),
                ),
            ]),
        ),
        (
            "survival with creative abilities",
            compound(vec![
                ("playerGameType", int(0)),
                (
                    "abilities",
                    NbtTag::Compound(compound(vec![
                        ("invulnerable", byte(1)),
                        ("mayfly", byte(1)),
                        ("flying", byte(1)),
                    ])),
                ),
            ]),
        ),
        ("air below zero", compound(vec![("Air", short(-5))])),
        (
            "air above the maximum",
            compound(vec![("Air", short(9999))]),
        ),
        (
            "air supply as an int",
            compound(vec![("AirSupply", int(120))]),
        ),
        (
            "air short and air supply",
            compound(vec![("Air", short(40)), ("AirSupply", int(120))]),
        ),
        (
            "drowning tick too high",
            compound(vec![("DrowningTick", int(999))]),
        ),
        (
            "drowning tick below zero",
            compound(vec![("DrowningTick", int(-3))]),
        ),
    ]
}

/// Sleep, vehicle, inventory and flag saves.
fn state_scenes() -> Vec<(&'static str, NbtCompound)> {
    vec![
        ("sleep timer zero", compound(vec![("SleepTimer", short(0))])),
        (
            "sleep timer below zero",
            compound(vec![("SleepTimer", short(-4))]),
        ),
        (
            "vehicle id as a list",
            compound(vec![(
                "RootVehicle",
                NbtTag::Compound(compound(vec![(
                    "Attach",
                    NbtTag::List(vec![int(5), int(6), int(7), int(8)]),
                )])),
            )]),
        ),
        (
            "vehicle id too short",
            compound(vec![(
                "RootVehicle",
                NbtTag::Compound(compound(vec![("Attach", NbtTag::IntArray(vec![1, 2]))])),
            )]),
        ),
        (
            "inventories",
            compound(vec![
                (
                    "Inventory",
                    NbtTag::List(vec![
                        item(0, "minecraft:stone", 5),
                        item(8, "minecraft:diamond", 2),
                    ]),
                ),
                (
                    "EnderItems",
                    NbtTag::List(vec![item(3, "minecraft:apple", 1)]),
                ),
            ]),
        ),
        (
            "flags false",
            compound(vec![
                ("seenCredits", byte(0)),
                ("spawn_extra_particles_on_fall", byte(0)),
                ("HasPlayedBefore", byte(0)),
                ("Score", int(-2)),
            ]),
        ),
    ]
}

/// NBT as text with the compound keys sorted.
fn nbt_text(tag: &NbtTag, out: &mut String) {
    match tag {
        NbtTag::Compound(compound) => compound_text(compound, out),
        NbtTag::List(items) => {
            out.push('[');
            for item in items {
                nbt_text(item, out);
                out.push(',');
            }
            out.push(']');
        }
        other => {
            let _ = write!(out, "{other:?}");
        }
    }
}

fn compound_text(compound: &NbtCompound, out: &mut String) {
    let mut keys: Vec<&str> = compound.child_tags.keys().map(AsRef::as_ref).collect();
    keys.sort_unstable();
    out.push('{');
    for key in keys {
        let _ = write!(out, "{key}:");
        if let Some(tag) = compound.child_tags.get(key) {
            nbt_text(tag, out);
        }
        out.push(',');
    }
    out.push('}');
}

#[tokio::test(flavor = "multi_thread")]
async fn player_nbt_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let mut out = String::new();
    for (name, input) in scenes() {
        let player = test_support::java_player(&world, GameMode::Survival);
        player.read_custom_nbt(&input);
        let mut written = NbtCompound::new();
        player.write_custom_nbt(&mut written);
        if input.get_int("XpSeed").is_none() {
            written.put("XpSeed", text("random"));
        }
        let entity = player.get_entity();
        let _ = write!(
            out,
            "{name}: points {} sleeping {:?} invulnerable {} no_physics {} ground {} saved ",
            player.experience_points.load(Relaxed),
            player.sleeping_since.load(),
            entity.invulnerable.load(Relaxed),
            entity.no_physics.load(Relaxed),
            entity.on_ground.load(Relaxed),
        );
        compound_text(&written, &mut out);
        out.push('\n');
    }
    server.stop();
    script_golden::bless_or_compare("player_nbt.txt", &out)
}
