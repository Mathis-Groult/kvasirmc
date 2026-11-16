//! A record of `/time` run through the real dispatcher (`tests/golden/time_command.txt`; rewrite
//! it on purpose with `KVASIR_BLESS=1`), made before `ActionExecutor::execute` was split.
//!
//! One server, one script run as the op Java player `GoldA` with three more players online (see
//! `script_golden`): per step the command, its result, the packets each player was sent and the
//! overworld's time afterwards (time of day, world age, rate, paused). Nothing ticks the server,
//! so only the commands move the time.

use std::fmt::Write;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;
use crate::world::World;

const STEPS: &[&str] = &[
    "time query time",
    "time query gametime",
    "time query daytime",
    "time query day",
    // Presets and values.
    "time set day",
    "time set noon",
    "time set night",
    "time set midnight",
    "time set 0",
    "time set 1000",
    "time set 1d",
    "time set 2.5d",
    "time set 30s",
    "time set 100t",
    "time set -1",
    "time set 2147483647",
    "time query time",
    "time query daytime",
    "time query day",
    "time set 1x",
    // Adding.
    "time set 500",
    "time add 1000",
    "time add -500",
    "time add 0",
    "time add 1d",
    "time query day",
    "time add 2147483647",
    "time query time",
    "time query daytime",
    "time query day",
    "time add -2147483648",
    "time query time",
    "time set 23999",
    "time add 1",
    "time query daytime",
    "time query day",
    // Pausing.
    "time pause",
    "time pause",
    "time query time",
    "time set 6000",
    "time add 10",
    "time resume",
    "time resume",
    // Rate.
    "time rate 2",
    "time rate 0.5",
    "time rate 1000",
    "time rate 0.00001",
    "time rate 1000.5",
    "time rate 0",
    "time rate 1",
    // Clocks.
    "time of minecraft:overworld set day",
    "time of minecraft:overworld add 5",
    "time of minecraft:overworld query time",
    "time of minecraft:overworld query gametime",
    "time of minecraft:overworld query daytime",
    "time of minecraft:overworld query day",
    "time of minecraft:overworld pause",
    "time of minecraft:overworld resume",
    "time of minecraft:overworld rate 3",
    "time of minecraft:the_end set noon",
    "time of minecraft:the_end query time",
    "time of minecraft:no_such_clock set day",
    "time of minecraft:no_such_clock query time",
    // Malformed.
    "time",
    "time set",
    "time add",
    "time rate",
    "time query",
    "time set dusk",
];

fn level_time(world: &World) -> String {
    let time = world
        .level_time
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    format!(
        "time_of_day={} world_age={} rate={} paused={}",
        time.time_of_day, time.world_age, time.rate, time.paused
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn time_command_sends_the_same_packets() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let mut online = script_golden::join(&world).await?;
    let source_player = online[0].player.clone();

    let mut now = String::new();
    writeln!(now, "start; {}", level_time(&world))?;
    for step in STEPS {
        let result = script_golden::run(&server, &source_player, step);
        let sent = script_golden::packets(&mut online);
        writeln!(now, "{step} -> {result};{sent}; {}", level_time(&world))?;
    }

    script_golden::bless_or_compare("time_command.txt", &now)?;
    script_golden::finish(&server, &world, &online).await;
    Ok(())
}
