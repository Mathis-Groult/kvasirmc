//! A record of `/bossbar` run through the real dispatcher (`tests/golden/bossbar_command.txt`;
//! rewrite it on purpose with `KVASIR_BLESS=1`), made before `SetExecutor::execute` and
//! `GetExecutor::execute` were split.
//!
//! One server, one script: the commands run in order and each sees the bars the earlier ones
//! left. The source is the op Java player `GoldA`; `GoldB` (op, Java), `GoldC` (not op, Java)
//! and `GoldD` (op, Bedrock) are online too (`script_golden`), so the feedback to the source, the
//! copy sent to the other ops and the boss bar packets each player gets are all visible. A step starting with
//! `!leave` takes `GoldC` out of the world. One line per step: the command, its result (count or
//! error text), the packets each player was sent (length and FNV hash of the encoded bytes) and
//! every custom bar afterwards (id, title text and a hash of the whole title, value, max,
//! visible, colour, style, flags, health and players by name). Each new bar's UUID is pinned
//! right after `add` (it is random), so the packets that carry it are the same in every run.

use std::fmt::Write;

use uuid::Uuid;

use crate::command::commands::script_golden::{self, TestResult, name_of};
use crate::entity::ai::test_support;
use crate::server::Server;

const STEPS: &[&str] = &[
    // Nothing yet.
    "bossbar list",
    "bossbar get gold:none max",
    "bossbar get gold:none players",
    "bossbar get gold:none value",
    "bossbar get gold:none visible",
    "bossbar set gold:none color red",
    "bossbar set gold:none max 5",
    "bossbar set gold:none name \"x\"",
    "bossbar set gold:none players",
    "bossbar set gold:none players GoldA",
    "bossbar set gold:none style notched_6",
    "bossbar set gold:none value 5",
    "bossbar set gold:none visible false",
    "bossbar remove gold:none",
    // Two bars.
    "bossbar add gold:one \"One\"",
    "bossbar add gold:one \"Again\"",
    "bossbar add two {\"text\":\"Two\",\"color\":\"red\"}",
    "bossbar list",
    // Reading a fresh bar.
    "bossbar get gold:one max",
    "bossbar get gold:one value",
    "bossbar get gold:one visible",
    "bossbar get gold:one players",
    // Players.
    "bossbar set gold:one players",
    "bossbar set gold:one players GoldA",
    "bossbar get gold:one players",
    "bossbar set gold:one players GoldA",
    "bossbar set gold:one players @a",
    "bossbar get gold:one players",
    "bossbar set gold:one players @a",
    "bossbar set gold:one players GoldB",
    "bossbar set gold:one players @a[name=!GoldB]",
    "bossbar set gold:one players",
    "bossbar set gold:one players",
    "bossbar get gold:one players",
    "bossbar set gold:one players @a",
    // Colour and style.
    "bossbar set gold:one color white",
    "bossbar set gold:one color red",
    "bossbar set gold:one color red",
    "bossbar set gold:one color blue",
    "bossbar set gold:one color green",
    "bossbar set gold:one color pink",
    "bossbar set gold:one color purple",
    "bossbar set gold:one color yellow",
    "bossbar set gold:one color orange",
    "bossbar set gold:one style progress",
    "bossbar set gold:one style notched_6",
    "bossbar set gold:one style notched_6",
    "bossbar set gold:one style notched_10",
    "bossbar set gold:one style notched_12",
    "bossbar set gold:one style notched_20",
    "bossbar set gold:one style notched_7",
    // Max and value.
    "bossbar set gold:one max 100",
    "bossbar set gold:one max 50",
    "bossbar set gold:one max 50",
    "bossbar get gold:one max",
    "bossbar set gold:one value 0",
    "bossbar set gold:one value 25",
    "bossbar set gold:one value 25",
    "bossbar get gold:one value",
    "bossbar set gold:one value 80",
    "bossbar get gold:one value",
    "bossbar set gold:one max 10",
    "bossbar get gold:one max",
    "bossbar get gold:one value",
    "bossbar set gold:one max 1",
    "bossbar set gold:one max 0",
    "bossbar set gold:one value -1",
    "bossbar set gold:one max 2147483647",
    "bossbar set gold:one value 2147483647",
    // Name.
    "bossbar set gold:one name \"Renamed\"",
    "bossbar set gold:one name \"Renamed\"",
    "bossbar set gold:one name {\"text\":\"Fancy\",\"bold\":true}",
    "bossbar get gold:one max",
    // Visibility.
    "bossbar set gold:one visible true",
    "bossbar set gold:one visible false",
    "bossbar set gold:one visible false",
    "bossbar get gold:one visible",
    "bossbar set gold:one value 3",
    "bossbar set gold:one max 4",
    "bossbar set gold:one color red",
    "bossbar set gold:one style progress",
    "bossbar set gold:one name \"Hidden\"",
    "bossbar set gold:one players GoldB",
    "bossbar set gold:one players @a",
    "bossbar set gold:one visible true",
    "bossbar get gold:one visible",
    // The second bar.
    "bossbar set two players @a[name=!GoldA]",
    "bossbar get two players",
    "bossbar set two value 1",
    "bossbar get two value",
    // A player leaves.
    "!leave GoldC",
    "bossbar get gold:one players",
    "bossbar set gold:one players @a",
    "bossbar get gold:one players",
    // Removing.
    "bossbar remove two",
    "bossbar remove two",
    "bossbar get two max",
    "bossbar list",
    "bossbar remove gold:one",
    "bossbar list",
    // Malformed.
    "bossbar set",
    "bossbar set gold:one",
    "bossbar get gold:one",
    "bossbar get gold:one colour",
    "bossbar add gold:three",
    "bossbar set gold:one visible maybe",
];

fn bars(server: &Server) -> String {
    let all = server.bossbars.lock().unwrap().get_all_bossbars();
    let mut lines: Vec<String> = all
        .iter()
        .map(|bar| {
            let data = &bar.bossbar_data;
            let players: Vec<String> = bar.players.iter().map(name_of).collect();
            format!(
                "{} title={}/{:016x} value={} max={} visible={} color={} style={} flags={} health={} players=[{}]",
                bar.namespace,
                data.title.clone().get_text(),
                script_golden::fnv(format!("{:?}", data.title).as_bytes()),
                bar.value,
                bar.max,
                bar.visible,
                data.color as u8,
                data.division as u8,
                data.flags.bits(),
                data.health,
                players.join(",")
            )
        })
        .collect();
    lines.sort();
    lines.join(" | ")
}

/// Pins the UUID of every bar whose UUID is not pinned yet (bar `n` in id order gets `n + 1`).
fn pin_uuids(server: &Server, pinned: &mut Vec<String>) {
    let mut bossbars = server.bossbars.lock().unwrap();
    let mut ids: Vec<String> = bossbars.custom_bossbars.keys().cloned().collect();
    ids.sort();
    for id in ids {
        if pinned.contains(&id) {
            continue;
        }
        pinned.push(id.clone());
        if let Some(bar) = bossbars.custom_bossbars.get_mut(&id) {
            bar.bossbar_data.uuid = Uuid::from_u128(0xba00_0000 + pinned.len() as u128);
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn bossbar_command_sends_the_same_packets() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let mut online = script_golden::join(&world).await?;
    let source_player = online[0].player.clone();

    let mut pinned = Vec::new();
    let mut now = String::new();
    for step in STEPS {
        if let Some(name) = step.strip_prefix("!leave ") {
            let index = online
                .iter()
                .position(|who| who.name == name)
                .ok_or("unknown player in a leave step")?;
            let who = online.remove(index);
            world.remove_player(&who.player, false).await;
            let left = script_golden::packets(&mut online);
            writeln!(now, "{step} ->{left}; bars {}", bars(&server))?;
            continue;
        }
        let result = script_golden::run(&server, &source_player, step);
        pin_uuids(&server, &mut pinned);
        let sent = script_golden::packets(&mut online);
        writeln!(now, "{step} -> {result};{sent}; bars {}", bars(&server))?;
    }

    script_golden::bless_or_compare("bossbar_command.txt", &now)?;
    script_golden::finish(&server, &world, &online).await;
    Ok(())
}
