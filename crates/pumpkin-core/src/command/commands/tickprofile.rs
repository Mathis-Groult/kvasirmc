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
        let profile = &context.source.server().tickprof;
        let say = |text: String| {
            context
                .source
                .send_feedback(TextComponent::text(format!("tickprofile: {text}")), false);
        };
        let readings = profile.snapshot();
        for reading in &readings {
            say(reading.to_string());
        }
        say(format!("dist {}", profile.distribution()));
        for phase in profile.slow_phases() {
            if !phase.mean_over_budget.is_zero() {
                say(format!("slowphase {phase}"));
            }
        }
        for tick in profile.worst_ticks() {
            say(format!("worst {tick}"));
        }
        for (part, row) in kvasir_tickprof::parts::report() {
            say(format!(
                "part {} ticks={} mean_us={:.1} total_ms={:.0}",
                part.name(),
                row.ticks,
                row.mean_us(),
                row.total_ns as f64 / 1e6
            ));
        }
        for row in profile.kinds.top(12) {
            let name = u16::try_from(row.kind)
                .ok()
                .and_then(pumpkin_data::entity::EntityType::from_raw)
                .map_or("other", |kind| kind.resource_name);
            say(format!(
                "kind {name} ticks={} mean_us={:.1} total_ms={:.0}",
                row.ticks,
                row.mean_us(),
                row.total_ns as f64 / 1e6
            ));
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
