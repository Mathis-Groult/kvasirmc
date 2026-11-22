//! A record of the packet `Player::send_abilities_update` queues, made before the function was
//! split (`tests/golden/player_abilities_update.txt`; rewrite it on purpose with
//! `KVASIR_BLESS=1`).
//!
//! Java part: all 16 combinations of the four flags (invulnerable, flying, allow flying,
//! creative) over three fly/walk speed pairs, then the same flags with the game mode and the
//! permission level changed (the Java packet must not depend on them). Bedrock part: the 16
//! combinations over every game mode and three permission levels (operator is level four only),
//! then the 16 combinations with odd speeds (the Bedrock layers fix the speeds). Every line shows
//! the packet bytes the player's outgoing queue received.

use std::fmt::Write;
use std::sync::Arc;

use pumpkin_util::GameMode;
use pumpkin_util::permission::PermissionLvl;
use uuid::Uuid;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::player::{Abilities, Player};

const JAVA_USER: Identity = Identity {
    name: "Flyer",
    uuid: Uuid::from_u128(0x0000_00fe),
    entity_id: 9_821,
};

const BEDROCK_USER: Identity = Identity {
    name: "Floater",
    uuid: Uuid::from_u128(0x0000_00ff),
    entity_id: 9_822,
};

const SPEEDS: [(f32, f32); 3] = [(0.05, 0.1), (0.0, 0.0), (0.2, 0.5)];
const MODES: [GameMode; 4] = [
    GameMode::Survival,
    GameMode::Creative,
    GameMode::Adventure,
    GameMode::Spectator,
];
const LEVELS: [PermissionLvl; 3] = [
    PermissionLvl::Zero,
    PermissionLvl::Three,
    PermissionLvl::Four,
];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

fn flag_text(mask: u8) -> String {
    let names = ["invulnerable", "flying", "allow_flying", "creative"];
    let on: Vec<&str> = names
        .iter()
        .enumerate()
        .filter(|(bit, _)| mask & (1 << bit) != 0)
        .map(|(_, name)| *name)
        .collect();
    if on.is_empty() {
        "none".to_string()
    } else {
        on.join("+")
    }
}

fn set_abilities(player: &Player, mask: u8, speeds: (f32, f32)) {
    *player
        .abilities
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Abilities {
        invulnerable: mask & 1 != 0,
        flying: mask & 2 != 0,
        allow_flying: mask & 4 != 0,
        creative: mask & 8 != 0,
        allow_modify_world: false,
        fly_speed: speeds.0,
        walk_speed: speeds.1,
    };
}

fn java_lines(
    player: &Player,
    tap: &mut crate::net::java::PacketTap,
    text: &mut String,
) -> TestResult {
    let mut section = |title: &str, mode: GameMode, level: PermissionLvl, speeds: &[(f32, f32)]| {
        player.gamemode.store(mode);
        player.permission_lvl.store(level);
        writeln!(text, "--- java {title} ---")?;
        for speed in speeds {
            for mask in 0..16 {
                set_abilities(player, mask, *speed);
                tap.drain();
                player.send_abilities_update();
                let packets: Vec<String> = tap.drain().iter().map(|p| hex(p)).collect();
                writeln!(
                    text,
                    "{:?} {:?} speed {speed:?} {} -> {packets:?}",
                    mode,
                    level,
                    flag_text(mask)
                )?;
            }
        }
        Ok::<(), std::fmt::Error>(())
    };
    section(
        "flags and speeds",
        GameMode::Survival,
        PermissionLvl::Zero,
        &SPEEDS,
    )?;
    section(
        "spectator operator",
        GameMode::Spectator,
        PermissionLvl::Four,
        &SPEEDS[..1],
    )?;
    section(
        "creative admin",
        GameMode::Creative,
        PermissionLvl::Three,
        &SPEEDS[2..],
    )?;
    Ok(())
}

fn bedrock_lines(
    player: &Player,
    tap: &mut crate::net::bedrock::BedrockPacketTap,
    text: &mut String,
) -> TestResult {
    for level in LEVELS {
        for mode in MODES {
            writeln!(text, "--- bedrock {mode:?} {level:?} ---")?;
            player.gamemode.store(mode);
            player.permission_lvl.store(level);
            for mask in 0..16 {
                set_abilities(player, mask, SPEEDS[0]);
                tap.drain();
                player.send_abilities_update();
                let packets: Vec<String> = tap.drain().iter().map(|p| hex(p)).collect();
                writeln!(text, "{} -> {packets:?}", flag_text(mask))?;
            }
        }
    }
    writeln!(text, "--- bedrock odd speeds ---")?;
    player.gamemode.store(GameMode::Survival);
    player.permission_lvl.store(PermissionLvl::Zero);
    for mask in 0..16 {
        set_abilities(player, mask, SPEEDS[2]);
        tap.drain();
        player.send_abilities_update();
        let packets: Vec<String> = tap.drain().iter().map(|p| hex(p)).collect();
        writeln!(text, "{} -> {packets:?}", flag_text(mask))?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn abilities_update_every_state() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world: Arc<_> = server.worlds.load().first().cloned().ok_or("no world")?;
    let (java, mut java_tap) = test_support::java_player_as(&world, GameMode::Survival, &JAVA_USER)
        .ok_or("no Java packet tap")?;
    let (bedrock, mut bedrock_tap) =
        test_support::bedrock_player_as(&world, GameMode::Survival, &BEDROCK_USER).await?;
    let mut text = String::new();
    java_lines(&java, &mut java_tap, &mut text)?;
    bedrock_lines(&bedrock, &mut bedrock_tap, &mut text)?;
    server.stop();
    script_golden::bless_or_compare("player_abilities_update.txt", &text)
}
