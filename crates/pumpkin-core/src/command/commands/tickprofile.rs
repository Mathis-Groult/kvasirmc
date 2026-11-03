//! `KvasirMC`: `/tickprofile`, how long each phase of a tick takes (see `kvasir-tickprof`).

use pumpkin_util::PermissionLvl;
use pumpkin_util::permission::{Permission, PermissionDefault, PermissionRegistry};
use pumpkin_util::text::TextComponent;

use crate::command::argument_builder::{ArgumentBuilder, command, literal};
use crate::command::context::command_context::CommandContext;
use crate::command::node::dispatcher::CommandDispatcher;
use crate::command::node::{CommandExecutor, CommandExecutorResult};

const DESCRIPTION: &str = "Shows how long each phase of a server tick takes.";
const PERMISSION: &str = "kvasirmc:command.tickprofile";

struct ReportExecutor;

impl CommandExecutor for ReportExecutor {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let readings = context.source.server().tickprof.snapshot();
        for reading in &readings {
            context.source.send_feedback(
                TextComponent::text(format!("tickprofile: {reading}")),
                false,
            );
        }
        Ok(i32::try_from(readings.len()).unwrap_or(i32::MAX))
    }
}

struct ResetExecutor;

impl CommandExecutor for ResetExecutor {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        context.source.server().tickprof.reset();
        context
            .source
            .send_feedback(TextComponent::text("tickprofile: reset"), false);
        Ok(1)
    }
}

pub fn register(dispatcher: &mut CommandDispatcher, registry: &PermissionRegistry) {
    registry.register_permission_or_panic(Permission::new(
        PERMISSION,
        DESCRIPTION,
        PermissionDefault::Op(PermissionLvl::Three),
    ));
    dispatcher.register(
        command("tickprofile", DESCRIPTION)
            .requires(PERMISSION)
            .executes(ReportExecutor)
            .then(literal("reset").executes(ResetExecutor)),
    );
}
