//! Shared harness of the command goldens that run a script of commands as an op player and
//! record what every online player is sent (`bossbar`, `time`, `tick`).
//!
//! The players are real players over in-memory pipes with fixed identities, so the packets that
//! carry a name, UUID or entity id are the same in every run; each packet is recorded as its
//! length and an FNV hash of its encoded bytes.

use std::fmt::Write;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use pumpkin_util::GameMode;
use pumpkin_util::PermissionLvl;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::text::TextComponent;
use uuid::Uuid;

use crate::command::{CommandSender, CommandSource};
use crate::entity::EntityBase;
use crate::entity::ai::test_support::{self, Identity};
use crate::entity::player::Player;
use crate::server::Server;
use crate::world::World;

pub type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The source of every command: an op Java player.
pub const A: Identity = Identity {
    name: "GoldA",
    uuid: Uuid::from_u128(0x0b05_5ba0_0000_000a),
    entity_id: 71_001,
};
/// A second op, on Java: gets the copy of the feedback sent to ops.
pub const B: Identity = Identity {
    name: "GoldB",
    uuid: Uuid::from_u128(0x0b05_5ba0_0000_000b),
    entity_id: 71_002,
};
/// Not an op, on Java.
pub const C: Identity = Identity {
    name: "GoldC",
    uuid: Uuid::from_u128(0x0b05_5ba0_0000_000c),
    entity_id: 71_003,
};
/// An op on Bedrock: sees the Bedrock translation keys.
pub const D: Identity = Identity {
    name: "GoldD",
    uuid: Uuid::from_u128(0x0b05_5ba0_0000_000d),
    entity_id: 71_004,
};

pub struct Online {
    pub name: &'static str,
    pub player: Arc<Player>,
    tap: Tap,
}

enum Tap {
    Java(crate::net::java::PacketTap),
    Bedrock(crate::net::bedrock::BedrockPacketTap),
}

impl Tap {
    fn drain(&mut self) -> Vec<Bytes> {
        match self {
            Self::Java(tap) => tap.drain(),
            Self::Bedrock(tap) => tap.drain(),
        }
    }
}

pub fn fnv(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// What each player was sent since the last call, as ` Name[len:hash,..]` per player.
pub fn packets(online: &mut [Online]) -> String {
    packets_masked(online, &[])
}

/// As [`packets`], with every occurrence of each `mask` in a packet overwritten by as many `#`
/// before hashing (text that changes per build, such as the commit hash).
pub fn packets_masked(online: &mut [Online], masks: &[&str]) -> String {
    let mut text = String::new();
    for who in online {
        let sent: Vec<String> = who
            .tap
            .drain()
            .iter()
            .map(|packet| {
                let masked = mask(packet, masks);
                format!("{}:{:016x}", masked.len(), fnv(&masked))
            })
            .collect();
        let _ = write!(text, " {}[{}]", who.name, sent.join(","));
    }
    text
}

fn mask(packet: &[u8], masks: &[&str]) -> Vec<u8> {
    let mut bytes = packet.to_vec();
    for mask in masks {
        let needle = mask.as_bytes();
        if needle.is_empty() {
            continue;
        }
        let mut at = 0;
        while at + needle.len() <= bytes.len() {
            if bytes[at..at + needle.len()] == *needle {
                bytes[at..at + needle.len()].fill(b'#');
                at += needle.len();
            } else {
                at += 1;
            }
        }
    }
    bytes
}

/// The name of one of the four test players, or the UUID itself.
pub fn name_of(uuid: &Uuid) -> String {
    [A, B, C, D]
        .iter()
        .find(|who| who.uuid == *uuid)
        .map_or_else(|| uuid.to_string(), |who| who.name.to_owned())
}

/// Runs `command` as `source_player` (not silent) on its own thread; `ok N`, `err <text>` or
/// `HANG` when it does not return within a minute.
pub fn run(server: &Arc<Server>, source_player: &Arc<Player>, command: &str) -> String {
    let world = server.worlds.load()[0].clone();
    let source = CommandSource::new(
        CommandSender::Player(source_player.clone()),
        world,
        Some(source_player.clone() as Arc<dyn EntityBase>),
        Vector3::new(0.5, 64.0, 0.5),
        Vector2::new(0.0, 0.0),
        A.name.to_string(),
        TextComponent::text(A.name),
        server.clone(),
    );
    let server = server.clone();
    let command = command.to_owned();
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let dispatcher = server.command_dispatcher.load();
        let result = dispatcher.execute_input(&command, &source);
        let _ = send.send(match result {
            Ok(count) => format!("ok {count}"),
            Err(error) => format!("err {}", error.message.get_text()),
        });
    });
    receive
        .recv_timeout(Duration::from_secs(60))
        .unwrap_or_else(|_| "HANG".to_owned())
}

/// `GoldA`, `GoldB`, `GoldC` and `GoldD` added to `world`, with their join packets drained.
pub async fn join(world: &Arc<World>) -> Result<Vec<Online>, String> {
    let mut online = Vec::new();
    for (who, op) in [(&A, true), (&B, true), (&C, false)] {
        let (player, tap) = test_support::java_player_as(world, GameMode::Survival, who)
            .ok_or("a test player's client no longer owned its outgoing queue")?;
        online.push(Online {
            name: who.name,
            player,
            tap: Tap::Java(tap),
        });
        if op {
            online[online.len() - 1]
                .player
                .permission_lvl
                .store(PermissionLvl::Four);
        }
    }
    let (player, tap) = test_support::bedrock_player_as(world, GameMode::Survival, &D).await?;
    player.permission_lvl.store(PermissionLvl::Four);
    online.push(Online {
        name: D.name,
        player,
        tap: Tap::Bedrock(tap),
    });
    for who in &online {
        world.add_player(&who.player)?;
    }
    for who in &mut online {
        who.tap.drain();
    }
    Ok(online)
}

/// Writes `now` to `tests/golden/<file>` with `KVASIR_BLESS` set, else compares with it.
pub fn bless_or_compare(file: &str, now: &str) -> TestResult {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(file);
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, now)?;
    } else {
        assert_eq!(std::fs::read_to_string(&path)?, now);
    }
    Ok(())
}

/// Takes every player still online out of the world and stops the server.
pub async fn finish(server: &Server, world: &World, online: &[Online]) {
    for who in online {
        world.remove_player(&who.player, false).await;
    }
    server.stop();
}
