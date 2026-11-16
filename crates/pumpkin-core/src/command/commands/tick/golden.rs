//! A record of `/tick` run through the real dispatcher (`tests/golden/tick_command.txt`; rewrite
//! it on purpose with `KVASIR_BLESS=1`), made before `TickExecutor::execute` was split.
//!
//! One server, one script run as the op Java player `GoldA` with three more players online (see
//! `script_golden`): per step the command, its result, the packets each player was sent and the
//! tick rate manager afterwards (rate, nanoseconds per tick, frozen, runs normally, sprinting,
//! stepping). Nothing ticks the server, so a step or sprint stays requested; a `!sprint-work`
//! step does the sprint bookkeeping of the tick loop until the sprint ends.

use std::fmt::Write;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;
use crate::server::Server;

const STEPS: &[&str] = &[
    "tick query",
    // Rate.
    "tick rate 20",
    "tick rate 1",
    "tick rate 10000",
    "tick rate 25.5",
    "tick query",
    "tick rate 0.5",
    "tick rate 10001",
    "tick rate 20",
    // Stepping needs a frozen game.
    "tick step",
    "tick step 5",
    "tick step stop",
    "tick freeze",
    "tick freeze",
    "tick query",
    "tick step",
    "tick query",
    "tick step 10",
    "tick step 1s",
    "tick step 0",
    "tick step stop",
    "tick step stop",
    "tick unfreeze",
    "tick unfreeze",
    "tick query",
    // Sprinting.
    "tick sprint stop",
    "tick sprint 100",
    "tick query",
    "tick sprint 1d",
    "tick sprint stop",
    "tick sprint stop",
    "tick freeze",
    "tick sprint 20",
    "tick query",
    "tick step",
    "tick sprint stop",
    "tick unfreeze",
    "tick rate 20",
    // Malformed.
    "tick",
    "tick rate",
    "tick sprint",
    "tick step -1",
    "tick freeze now",
    // Appended after the mutation run: how many ticks a sprint lasts.
    "tick sprint 3",
    "!sprint-work",
    "tick query",
    "tick sprint stop",
    "tick sprint 1s",
    "!sprint-work",
    "tick sprint stop",
];

fn manager(server: &Server) -> String {
    let manager = &server.tick_rate_manager;
    format!(
        "rate={} nanos_per_tick={} frozen={} normal={} sprinting={} stepping={}",
        manager.tickrate(),
        manager.nanoseconds_per_tick(),
        manager.is_frozen(),
        manager.runs_normally(),
        manager.is_sprinting(),
        manager.is_stepping_forward()
    )
}

/// Does the sprint bookkeeping of ticks until the sprint reports its end (at most 100 ticks),
/// as the server's tick loop would: `ended after N` or `still sprinting`.
fn run_sprint(server: &Server) -> String {
    let manager = &server.tick_rate_manager;
    for tick in 1..=100 {
        manager.start_sprint_tick_work();
        if manager.end_sprint_tick_work() {
            return format!("ended after {tick}");
        }
    }
    "still sprinting".to_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn tick_command_sends_the_same_packets() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let mut online = script_golden::join(&world).await?;
    let source_player = online[0].player.clone();

    let mut now = String::new();
    writeln!(now, "start; {}", manager(&server))?;
    for step in STEPS {
        if *step == "!sprint-work" {
            let ticks = run_sprint(&server);
            let sent = script_golden::packets(&mut online);
            writeln!(now, "{step} -> {ticks};{sent}; {}", manager(&server))?;
            continue;
        }
        let result = script_golden::run(&server, &source_player, step);
        let sent = script_golden::packets(&mut online);
        writeln!(now, "{step} -> {result};{sent}; {}", manager(&server))?;
    }

    script_golden::bless_or_compare("tick_command.txt", &now)?;
    script_golden::finish(&server, &world, &online).await;
    Ok(())
}
