//! A record of `/effect clear` run through the shared script harness
//! (`tests/golden/effect_clear_command.txt`; rewrite it on purpose with `KVASIR_BLESS=1`), made
//! before `ClearExecutor::execute` was split.
//!
//! `/effect give` steps set the effects up. Per step the command, its result, every packet each
//! player was sent and each player's active effects afterwards (name, amplifier, duration,
//! particles), sorted. Nothing ticks the server, so durations do not run down.

use std::fmt::Write;

use crate::command::commands::script_golden::{self, Online, TestResult};
use crate::entity::ai::test_support;

const STEPS: &[&str] = &[
    // Nothing to clear.
    "effect clear",
    "effect clear GoldB",
    "effect clear @a",
    "effect clear GoldB minecraft:speed",
    "effect clear @a minecraft:speed",
    // The source itself.
    "effect give GoldA minecraft:speed 30 1",
    "effect give GoldA minecraft:haste 60 0 true",
    "effect clear",
    "effect clear",
    // One target.
    "effect give GoldB minecraft:speed 30 1",
    "effect give GoldB minecraft:strength infinite 2",
    "effect clear GoldB minecraft:haste",
    "effect clear GoldB minecraft:speed",
    "effect clear GoldB minecraft:speed",
    "effect clear GoldB",
    "effect clear GoldB",
    // Several targets, some without the effect.
    "effect give GoldB minecraft:regeneration 10 0",
    "effect give GoldD minecraft:regeneration 10 3",
    "effect give GoldC minecraft:night_vision 100 0",
    "effect clear @a minecraft:regeneration",
    "effect clear @a minecraft:regeneration",
    "effect give @a minecraft:glowing 5 0",
    "effect give GoldA minecraft:jump_boost 5 4",
    "effect clear @a[name=!GoldC] minecraft:jump_boost",
    "effect clear @a[name=!GoldA]",
    "effect clear @a",
    "effect clear @a",
    // Malformed.
    "effect clear GoldB minecraft:no_such_effect",
    "effect clear NoSuchPlayer",
    "effect clear @e[type=minecraft:zombie]",
    "effect clear GoldB speed",
];

fn effects(online: &[Online]) -> String {
    let mut text = String::new();
    for who in online {
        let active = who
            .player
            .living_entity
            .active_effects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut list: Vec<String> = active
            .iter()
            .map(|(kind, effect)| {
                format!(
                    "{}:{}:{}:{}",
                    kind.minecraft_name, effect.amplifier, effect.duration, effect.show_particles
                )
            })
            .collect();
        list.sort();
        let _ = write!(text, " {}[{}]", who.name, list.join(","));
    }
    text
}

#[tokio::test(flavor = "multi_thread")]
async fn effect_clear_command_sends_the_same_packets() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let mut online = script_golden::join(&world).await?;
    let source_player = online[0].player.clone();

    let mut now = String::new();
    for step in STEPS {
        let result = script_golden::run(&server, &source_player, step);
        let sent = script_golden::packets(&mut online);
        writeln!(now, "{step} -> {result};{sent};{}", effects(&online))?;
    }

    script_golden::bless_or_compare("effect_clear_command.txt", &now)?;
    script_golden::finish(&server, &world, &online).await;
    Ok(())
}
