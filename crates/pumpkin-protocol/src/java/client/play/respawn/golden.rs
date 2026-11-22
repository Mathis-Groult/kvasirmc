//! A record of `CRespawn::read` (`tests/golden/respawn_read.txt`; rewrite it on purpose with
//! `KVASIR_BLESS=1`), made before it was split.
//!
//! The body is a pure decoder with four wire layouts: before 1.16 (legacy dimension id, level
//! type text), 1.16 to 1.20 (dimension name or NBT, game modes, keep flag, death location, portal
//! cooldown), and 1.20.2 onwards (spawn data first, keep byte last). The bytes of every scene are
//! built here by hand from the layout, one line per version and scene: the bytes (hex), then the
//! decoded packet and the bytes left over, or the error. Versions straddle every flag the body
//! reads (1.14, 1.15, 1.16, 1.16.2, 1.19, 1.19.3, 1.20, 1.20.2, 1.20.5, 1.21.2, 26.3). A second
//! section writes each scene with `CRespawn::write_packet_data` and reads it back, and a third
//! reads every truncation of one full packet per version, runs of equal results folded.

use std::fmt::Write as _;

use pumpkin_data::dimension::Dimension;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::version::JavaMinecraftVersion as V;

use super::CRespawn;
use crate::java::client::play::player_spawn_data::PlayerSpawnData;
use crate::ser::{NetworkWriteExt, WritingError};
use crate::{ClientPacket, ServerPacket, VarInt};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const VERSIONS: [V; 22] = [
    V::V_1_12_2,
    V::V_1_13_2,
    V::V_1_14,
    V::V_1_14_4,
    V::V_1_15,
    V::V_1_15_2,
    V::V_1_16,
    V::V_1_16_1,
    V::V_1_16_2,
    V::V_1_18_2,
    V::V_1_19,
    V::V_1_19_1,
    V::V_1_19_3,
    V::V_1_19_4,
    V::V_1_20,
    V::V_1_20_2,
    V::V_1_20_5,
    V::V_1_21,
    V::V_1_21_2,
    V::V_1_21_11,
    V::V_26_2,
    V::V_26_3,
];

/// An empty compound with an empty name.
const NBT_EMPTY: &[u8] = &[0x0A, 0, 0, 0];
/// A compound named `x` holding one byte tag `b` = 5.
const NBT_BYTE: &[u8] = &[0x0A, 0, 1, b'x', 0x01, 0, 1, b'b', 5, 0];
/// Not NBT: tag id 255.
const NBT_BAD: &[u8] = &[0xFF];

/// Every field any layout carries, with the value each layout writes for it.
#[derive(Clone)]
struct Fields {
    legacy_dim: i32,
    dim_name: &'static str,
    dim_id: i32,
    nbt: &'static [u8],
    world: &'static str,
    seed: i64,
    mode: u8,
    prev: i8,
    debug: bool,
    flat: bool,
    death: Option<(&'static str, [i32; 3])>,
    cooldown: i32,
    sealevel: i32,
    keep: u8,
    level_type: &'static str,
    difficulty: u8,
}

fn base() -> Fields {
    Fields {
        legacy_dim: -1,
        dim_name: "minecraft:the_nether",
        dim_id: 3,
        nbt: NBT_EMPTY,
        world: "w",
        seed: 0x0123_4567_89AB_CDEF,
        mode: 1,
        prev: 2,
        debug: false,
        flat: true,
        death: Some(("minecraft:the_end", [-5, 70, 300])),
        cooldown: 7,
        sealevel: 90,
        keep: 3,
        level_type: "flat",
        difficulty: 3,
    }
}

fn scenes() -> Vec<(&'static str, Fields)> {
    vec![
        ("nether, flat, death set", base()),
        (
            "end, debug, no death, previous -1",
            Fields {
                legacy_dim: 1,
                dim_name: "minecraft:the_end",
                dim_id: 2,
                seed: -1,
                mode: 3,
                prev: -1,
                debug: true,
                flat: false,
                death: None,
                cooldown: 0,
                sealevel: 0,
                keep: 2,
                level_type: "debug_all_block_states",
                difficulty: 0,
                ..base()
            },
        ),
        (
            "overworld, previous 0",
            Fields {
                legacy_dim: 0,
                dim_name: "minecraft:overworld",
                dim_id: 0,
                seed: i64::MIN,
                mode: 0,
                prev: 0,
                flat: false,
                keep: 1,
                level_type: "default",
                difficulty: 1,
                ..base()
            },
        ),
        (
            "unknown dimension, previous 3",
            Fields {
                legacy_dim: 7,
                dim_name: "minecraft:custom",
                dim_id: 9,
                seed: i64::MAX,
                mode: 2,
                prev: 3,
                flat: false,
                keep: 0,
                level_type: "customized",
                difficulty: 2,
                ..base()
            },
        ),
        (
            "caves, far death, long varints",
            Fields {
                dim_name: "minecraft:overworld_caves",
                dim_id: 1,
                death: Some(("minecraft:overworld", [1_000_000, -64, -1_000_000])),
                cooldown: 300,
                sealevel: 320,
                ..base()
            },
        ),
        (
            "nbt with a tag, empty world name",
            Fields {
                nbt: NBT_BYTE,
                world: "",
                death: Some(("", [0, 0, 0])),
                ..base()
            },
        ),
        (
            "invalid nbt",
            Fields {
                nbt: NBT_BAD,
                ..base()
            },
        ),
    ]
}

/// The game modes of every layout: two plain bytes before 26.3, var ints from 26.3 on where the
/// previous mode is stored plus one and 0 means none.
fn put_modes(out: &mut Vec<u8>, version: V, f: &Fields) -> Result<(), WritingError> {
    if version >= V::V_26_3 {
        out.write_var_int(&VarInt(i32::from(f.mode)))?;
        let previous = if f.prev < 0 { 0 } else { i32::from(f.prev) + 1 };
        out.write_var_int(&VarInt(previous))
    } else {
        out.write_u8(f.mode)?;
        out.write_i8(f.prev)
    }
}

fn put_death(out: &mut Vec<u8>, version: V, f: &Fields) -> Result<(), WritingError> {
    match &f.death {
        Some((dim, [x, y, z])) => {
            out.write_bool(true)?;
            out.write_string(dim)?;
            out.write_block_pos(&BlockPos::new(*x, *y, *z), &version)
        }
        None => out.write_bool(false),
    }
}

fn legacy_layout(version: V, f: &Fields) -> Result<Vec<u8>, WritingError> {
    let mut out = Vec::new();
    out.write_i32_be(f.legacy_dim)?;
    if version >= V::V_1_15 {
        out.write_i64_be(f.seed)?;
    }
    if version < V::V_1_14 {
        out.write_u8(f.difficulty)?;
    }
    out.write_u8(f.mode)?;
    out.write_string(f.level_type)?;
    Ok(out)
}

fn middle_layout(version: V, f: &Fields) -> Result<Vec<u8>, WritingError> {
    let mut out = Vec::new();
    if version >= V::V_1_16_2 && version < V::V_1_19 {
        out.extend_from_slice(f.nbt);
    } else {
        out.write_string(f.dim_name)?;
    }
    out.write_string(f.world)?;
    out.write_i64_be(f.seed)?;
    put_modes(&mut out, version, f)?;
    out.write_bool(f.debug)?;
    out.write_bool(f.flat)?;
    if version >= V::V_1_19_3 {
        out.write_u8(f.keep)?;
    } else {
        out.write_bool(f.keep & 1 != 0)?;
    }
    if version >= V::V_1_19 {
        put_death(&mut out, version, f)?;
    }
    if version >= V::V_1_20 {
        out.write_var_int(&VarInt(f.cooldown))?;
    }
    Ok(out)
}

fn modern_layout(version: V, f: &Fields) -> Result<Vec<u8>, WritingError> {
    let mut out = Vec::new();
    if version >= V::V_1_20_5 {
        out.write_var_int(&VarInt(f.dim_id))?;
    } else {
        out.write_string(f.dim_name)?;
    }
    out.write_string(f.world)?;
    out.write_i64_be(f.seed)?;
    put_modes(&mut out, version, f)?;
    out.write_bool(f.debug)?;
    out.write_bool(f.flat)?;
    put_death(&mut out, version, f)?;
    out.write_var_int(&VarInt(f.cooldown))?;
    if version >= V::V_1_21_2 {
        out.write_var_int(&VarInt(f.sealevel))?;
    }
    out.write_u8(f.keep)?;
    Ok(out)
}

fn layout(version: V, f: &Fields) -> Result<Vec<u8>, WritingError> {
    if version < V::V_1_16 {
        legacy_layout(version, f)
    } else if version < V::V_1_20_2 {
        middle_layout(version, f)
    } else {
        modern_layout(version, f)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

fn describe(packet: &CRespawn) -> String {
    let info = &packet.player_spawn_info;
    format!(
        "{} seed {} mode {} prev {} debug {} flat {} death {:?} cooldown {} sealevel {} keep {}",
        info.dimension.minecraft_name,
        info.hashed_seed,
        info.game_mode,
        info.previous_gamemode,
        info.debug,
        info.is_flat,
        info.death_dimension_name,
        info.portal_cooldown.0,
        info.sealevel.0,
        packet.data_kept
    )
}

fn render(version: V, bytes: &[u8]) -> String {
    let mut rest = bytes;
    match CRespawn::read(&mut rest, &version) {
        Ok(packet) => format!("ok {} left {}", describe(&packet), rest.len()),
        Err(error) => format!("err {error:?}"),
    }
}

/// The packet a scene means, for the writer.
fn packet(f: &Fields) -> CRespawn {
    let dimension = Dimension::from_name(f.dim_name)
        .cloned()
        .unwrap_or(Dimension::OVERWORLD);
    let death = f
        .death
        .map(|(dim, [x, y, z])| (dim.to_string(), BlockPos::new(x, y, z)));
    CRespawn::new(
        PlayerSpawnData::new(
            dimension,
            f.seed,
            f.mode,
            f.prev,
            f.debug,
            f.flat,
            death,
            VarInt(f.cooldown),
            VarInt(f.sealevel),
        ),
        f.keep,
    )
}

fn known_bytes_lines() -> Result<String, Box<dyn std::error::Error>> {
    let mut text = String::new();
    for version in VERSIONS {
        for (name, f) in scenes() {
            let bytes = layout(version, &f)?;
            writeln!(
                text,
                "{version:?} {name}: {} => {}",
                hex(&bytes),
                render(version, &bytes)
            )?;
        }
        let mut longer = layout(version, &base())?;
        longer.extend_from_slice(&[0xEE, 0xEF]);
        writeln!(
            text,
            "{version:?} trailing bytes: {} => {}",
            hex(&longer),
            render(version, &longer)
        )?;
    }
    Ok(text)
}

/// Game mode sweep: every current mode with previous none, 0 and 3, on one version per layout.
fn game_mode_lines() -> Result<String, Box<dyn std::error::Error>> {
    let mut text = String::new();
    for version in [V::V_1_13_2, V::V_1_16, V::V_1_20_2, V::V_26_3] {
        for mode in 0..=3 {
            for prev in [-1, 0, 3] {
                let f = Fields {
                    mode,
                    prev,
                    ..base()
                };
                let bytes = layout(version, &f)?;
                writeln!(
                    text,
                    "{version:?} mode {mode} prev {prev}: {} => {}",
                    hex(&bytes),
                    render(version, &bytes)
                )?;
            }
        }
    }
    Ok(text)
}

fn round_trip(version: V, sent: &CRespawn) -> String {
    let mut bytes = Vec::new();
    if let Err(error) = sent.write_packet_data(&mut bytes, &version) {
        return format!("write error {error:?}");
    }
    let mut rest = bytes.as_slice();
    match CRespawn::read(&mut rest, &version) {
        Ok(got) if got == *sent && rest.is_empty() => "same".to_string(),
        Ok(got) => format!("differs: {} left {}", describe(&got), rest.len()),
        Err(error) => format!("read error {error:?}"),
    }
}

fn round_trip_lines() -> Result<String, Box<dyn std::error::Error>> {
    let mut text = String::new();
    for version in VERSIONS {
        for (name, f) in scenes() {
            let verdict = round_trip(version, &packet(&f));
            writeln!(text, "{version:?} {name}: roundtrip {verdict}")?;
        }
    }
    Ok(text)
}

/// Every truncation of `full`, runs of equal results folded into one line.
fn truncation_lines(version: V, name: &str, full: &[u8]) -> Result<String, std::fmt::Error> {
    let mut text = String::new();
    let mut run: Option<(usize, String)> = None;
    for len in 0..=full.len() {
        let result = render(version, &full[..len]);
        match &run {
            Some((_, previous)) if *previous == result => {}
            _ => {
                if let Some((start, previous)) = run.take() {
                    writeln!(
                        text,
                        "{version:?} {name} truncated {start}..{}: {previous}",
                        len - 1
                    )?;
                }
                run = Some((len, result));
            }
        }
    }
    if let Some((start, previous)) = run {
        writeln!(
            text,
            "{version:?} {name} truncated {start}..{}: {previous}",
            full.len()
        )?;
    }
    Ok(text)
}

fn sweep_lines() -> Result<String, Box<dyn std::error::Error>> {
    let mut text = String::new();
    for version in VERSIONS {
        text.push_str(&truncation_lines(
            version,
            "full",
            &layout(version, &base())?,
        )?);
    }
    for version in [V::V_1_16_2, V::V_1_18_2] {
        let f = Fields {
            nbt: NBT_BYTE,
            ..base()
        };
        text.push_str(&truncation_lines(
            version,
            "nbt with a tag",
            &layout(version, &f)?,
        )?);
    }
    Ok(text)
}

fn record() -> Result<String, Box<dyn std::error::Error>> {
    let mut now = known_bytes_lines()?;
    now.push_str(&game_mode_lines()?);
    now.push_str(&round_trip_lines()?);
    now.push_str(&sweep_lines()?);
    Ok(now)
}

#[test]
fn respawn_decodes_the_same() -> TestResult {
    let now = record()?;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("respawn_read.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("no parent")?)?;
        std::fs::write(&path, now)?;
    } else {
        assert_eq!(std::fs::read_to_string(&path)?, now);
    }
    Ok(())
}
