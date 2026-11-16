//! A record of `/pumpkin` run through the shared script harness (`tests/golden/pumpkin_command.txt`;
//! rewrite it on purpose with `KVASIR_BLESS=1`), made before `Executor::execute` was split.
//!
//! The message carries the commit hash, which changes with every build: both hashes are masked
//! in the packets before hashing. `/pumpkin contributors` fetches from the network and is left
//! out.

use std::fmt::Write;

use super::{GIT_HASH, GIT_HASH_FULL};
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;

const STEPS: &[&str] = &["pumpkin", "pumpkin extra"];

#[tokio::test(flavor = "multi_thread")]
async fn pumpkin_command_sends_the_same_message() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let mut online = script_golden::join(&world).await?;
    let source_player = online[0].player.clone();

    let mut now = String::new();
    for step in STEPS {
        let result = script_golden::run(&server, &source_player, step);
        let sent = script_golden::packets_masked(&mut online, &[GIT_HASH_FULL, GIT_HASH]);
        writeln!(now, "{step} -> {result};{sent}")?;
    }

    script_golden::bless_or_compare("pumpkin_command.txt", &now)?;
    script_golden::finish(&server, &world, &online).await;
    Ok(())
}
