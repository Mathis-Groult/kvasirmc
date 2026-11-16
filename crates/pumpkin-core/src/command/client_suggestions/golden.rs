//! A record of the command-graph packet `send_c_commands_packet` sends
//! (`tests/golden/commands_packet.txt`; rewrite it on purpose with `KVASIR_BLESS=1`), made before
//! it was split.
//!
//! One line per case: the dispatcher (the server's own, or a copy of the default one with
//! commands disabled and `/`-prefixed plugin-style commands added), the player's permission level,
//! and the length and FNV hash of every packet the player was sent.

use std::fmt::Write;

use pumpkin_config::CommandsConfig;
use pumpkin_util::PermissionLvl;
use pumpkin_util::permission::PermissionManager;

use super::send_c_commands_packet;
use crate::command::argument_builder::{ArgumentBuilder, command, literal};
use crate::command::commands::default_dispatcher;
use crate::command::commands::script_golden::{self, TestResult};
use crate::command::context::command_context::CommandContext;
use crate::command::node::dispatcher::CommandDispatcher;
use crate::command::node::{CommandExecutor, CommandExecutorResult};
use crate::entity::ai::test_support;

const LEVELS: [PermissionLvl; 5] = [
    PermissionLvl::Zero,
    PermissionLvl::One,
    PermissionLvl::Two,
    PermissionLvl::Three,
    PermissionLvl::Four,
];

struct Nothing;

impl CommandExecutor for Nothing {
    fn execute(&self, _context: &CommandContext) -> CommandExecutorResult {
        Ok(0)
    }
}

/// The default dispatcher with `give` and `TP` disabled, and plugin-style commands whose names
/// start with `/`: `//pos` has a `/pos` twin, `//wand` has none.
fn changed_dispatcher() -> CommandDispatcher {
    let mut dispatcher = default_dispatcher(&PermissionManager::new(), &CommandsConfig::default());
    dispatcher.disable_command("give");
    dispatcher.disable_command("TP");
    dispatcher.register(command("/pos", "test").executes(Nothing));
    dispatcher.register(
        command("//pos", "test")
            .executes(Nothing)
            .then(literal("set").executes(Nothing)),
    );
    dispatcher.register(command("//wand", "test").executes(Nothing));
    dispatcher
}

/// Appended after the mutation run: a `/kill` whose plain twin `kill` exists, and a
/// subcommand literal that itself starts with `//`.
fn slash_dispatcher() -> CommandDispatcher {
    let mut dispatcher = default_dispatcher(&PermissionManager::new(), &CommandsConfig::default());
    dispatcher.register(command("/kill", "test").executes(Nothing));
    dispatcher.register(
        command("//tool", "test")
            .executes(Nothing)
            .then(literal("//sub").executes(Nothing)),
    );
    dispatcher
}

#[tokio::test(flavor = "multi_thread")]
async fn commands_packet_is_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let mut online = script_golden::join(&world).await?;
    let player = online[2].player.clone();
    let changed = changed_dispatcher();
    let slash = slash_dispatcher();

    let mut now = String::new();
    for (name, dispatcher) in [
        ("server", &**server.command_dispatcher.load()),
        ("changed", &changed),
        ("slash", &slash),
    ] {
        for level in LEVELS {
            player.permission_lvl.store(level);
            send_c_commands_packet(&player, &server, dispatcher);
            let sent = script_golden::packets(&mut online);
            writeln!(now, "{name} level {}:{sent}", level as u8)?;
        }
    }

    script_golden::bless_or_compare("commands_packet.txt", &now)?;
    script_golden::finish(&server, &world, &online).await;
    Ok(())
}
