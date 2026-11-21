//! A record of `/test run` and `/test stop` run through the real dispatcher
//! (`tests/golden/test_command.txt`; rewrite it on purpose with `KVASIR_BLESS=1`), made before
//! `RunExecutor::execute` was split.
//!
//! One server, one script run as the op Java player `GoldA` (or the non-op `GoldC`) with two
//! more players online (see `script_golden`): per step the command, its result, the packets
//! each player was sent and the queued game-test requests afterwards (nothing ticks the
//! server, so the queue is only changed by the commands).

use std::fmt::Write;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;
use crate::server::server_test_manager::queued_game_tests;

/// `(source player index in the online list: 0 = GoldA op, 2 = GoldC non-op, command)`.
const STEPS: &[(usize, &str)] = &[
    (0, "test run always_pass"),
    (0, "test run minecraft:always_pass"),
    (0, "test run always_pass 1"),
    (0, "test run always_pass 3"),
    (0, "test run always_pass 3 true"),
    (0, "test run always_pass 3 false"),
    (0, "test run always_pass 0"),
    (0, "test run always_pass 0 true"),
    (0, "test run always_pass 1 true"),
    (0, "test run always_pass 1 false"),
    (0, "test run always_pass 2 false 1"),
    (0, "test run always_pass 2 false 3"),
    (0, "test run always_pass 2 false 4 5"),
    (0, "test run always_pass 2 false 0 0"),
    (0, "test run always_pass 2 false -1 -2"),
    (0, "test run always_pass -1"),
    (0, "test run always_pass 1 maybe"),
    (0, "test run always_pass 1 false x"),
    // Selectors.
    (0, "test run does_not_exist"),
    (0, "test run minecraft:does_not_exist"),
    (0, "test run other:always_pass"),
    (0, "test run always_pas?"),
    (0, "test run always_pas"),
    (0, "test run *always_pass"),
    (0, "test run *always_pa*"),
    (0, "test run minecraft:always_*"),
    (0, "test run minecraft:?lways_pass"),
    (0, "test run ALWAYS_PASS"),
    (0, "test run always pass"),
    (0, "test run"),
    (0, "test"),
    (
        0,
        "test run pumpkin:dispenser_honeycomb_beehive_and_shulker",
    ),
    (0, "test run pumpkin:*beehive*"),
    (0, "test run pumpkin:*beehive* 2 false 1 1"),
    (0, "test run pumpkin:*beehive* 2 false 0 1"),
    (0, "test run pumpkin:*dispenser_shears*"),
    (0, "test run pumpkin:*dispenser_shears* 1 true 2 2"),
    (0, "test run pumpkin:dispenser_*"),
    (0, "test run pumpkin:dispenser_* 1 false 1 3"),
    (0, "test run pumpkin:dispenser_* 1 false 2 0"),
    (0, "test run pumpkin:*"),
    // Permission and replacement.
    (2, "test run always_pass"),
    (2, "test stop"),
    (0, "test run always_pass 2"),
    (0, "test run pumpkin:*beehive*"),
    (0, "test stop"),
    (0, "test stop"),
    (0, "test run always_pass"),
    (0, "test run does_not_exist"),
    (0, "test stop"),
];

#[tokio::test(flavor = "multi_thread")]
async fn run_and_stop_are_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let mut online = script_golden::join(&world).await?;
    let mut out = String::new();
    let names = server.datapack_manager.get_test_instance_names();
    let _ = writeln!(
        out,
        "instances {} ({})",
        names.len(),
        names
            .iter()
            .filter(|name| name.contains("always_pass")
                || name.contains("beehive")
                || name.contains("dispenser_utilities"))
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    );
    for (who, command) in STEPS {
        let player = online[*who].player.clone();
        let result = script_golden::run(&server, &player, command);
        let sent = script_golden::packets(&mut online);
        let queue = queued_game_tests();
        let _ = writeln!(out, "{command} as {}: {result} |{sent}", online[*who].name);
        let _ = writeln!(out, "  queue {}: {}", queue.len(), queue.join(" ; "));
    }
    server.stop();
    script_golden::bless_or_compare("test_command.txt", &out)
}
