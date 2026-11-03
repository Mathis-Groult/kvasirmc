//! `KvasirMC`: `/pregen`, world pre-generation (see `server/pregen.rs`).

use kvasir_pregen::{ChunkPos, Job, MAX_RADIUS, Shape, ShapeError};
use kvasir_pregen_driver::Control;
use pumpkin_util::PermissionLvl;
use pumpkin_util::permission::{Permission, PermissionDefault, PermissionRegistry};
use pumpkin_util::text::TextComponent;

use crate::command::argument_builder::{ArgumentBuilder, argument, command, literal};
use crate::command::argument_types::coordinates::Coordinates;
use crate::command::argument_types::coordinates::column_pos::ColumnPosArgumentType;
use crate::command::argument_types::core::integer::IntegerArgumentType;
use crate::command::context::command_context::CommandContext;
use crate::command::errors::error_types::LiteralCommandErrorType;
use crate::command::node::dispatcher::CommandDispatcher;
use crate::command::node::{CommandExecutor, CommandExecutorResult};
use crate::server::pregen::StartError;

const DESCRIPTION: &str = "Generates the world around a point ahead of the players.";
const PERMISSION: &str = "kvasirmc:command.pregen";

static NO_WORLD: LiteralCommandErrorType = LiteralCommandErrorType::new("No world in source");
static NO_JOB: LiteralCommandErrorType =
    LiteralCommandErrorType::new("No pre-generation job in this dimension");
static ALREADY: LiteralCommandErrorType = LiteralCommandErrorType::new(
    "This dimension already has a pre-generation job: /pregen cancel it first",
);
static TOO_LARGE: LiteralCommandErrorType =
    LiteralCommandErrorType::new("That radius is larger than the world can hold");
static OUT_OF_RANGE: LiteralCommandErrorType =
    LiteralCommandErrorType::new("That area leaves the range of chunk coordinates");

struct StartExecutor {
    circle: bool,
}

impl CommandExecutor for StartExecutor {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let radius = u32::try_from(IntegerArgumentType::get(context, "radius")?).unwrap_or(1);
        let world = context
            .source
            .world
            .as_ref()
            .ok_or_else(|| NO_WORLD.create_without_context())?;
        let (block_x, block_z) = if context.get_argument::<Coordinates>("centre").is_ok() {
            let column = ColumnPosArgumentType::get_column_pos(context, "centre")?;
            (column.0.x, column.0.y)
        } else {
            (
                context.source.position.x.floor() as i32,
                context.source.position.z.floor() as i32,
            )
        };
        let centre = ChunkPos {
            x: block_x >> 4,
            z: block_z >> 4,
        };
        let shape = if self.circle {
            Shape::Circle { radius }
        } else {
            Shape::Square { radius }
        };
        let dimension = world.dimension.minecraft_name;
        let job = Job::new(dimension, centre, shape).map_err(|error| match error {
            ShapeError::TooLarge(_) => TOO_LARGE.create_without_context(),
            _ => OUT_OF_RANGE.create_without_context(),
        })?;
        let total = job.total();
        let server = context.source.server();
        server
            .pregen
            .start(server, world, job)
            .map_err(|error| match error {
                StartError::AlreadyRunning => ALREADY.create_without_context(),
                StartError::Shape(ShapeError::TooLarge(_)) => TOO_LARGE.create_without_context(),
                StartError::Shape(_) => OUT_OF_RANGE.create_without_context(),
            })?;
        context.source.send_feedback(
            TextComponent::text(format!(
                "Pre-generating {total} chunks of {dimension} ({} of radius {radius}) around chunk [{}, {}]. /pregen status shows progress.",
                if self.circle { "circle" } else { "square" },
                centre.x,
                centre.z
            )),
            true,
        );
        Ok(i32::try_from(total).unwrap_or(i32::MAX))
    }
}

struct ControlExecutor {
    command: Control,
    done: &'static str,
}

impl CommandExecutor for ControlExecutor {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let world = context
            .source
            .world
            .as_ref()
            .ok_or_else(|| NO_WORLD.create_without_context())?;
        let server = context.source.server();
        if !server
            .pregen
            .control(world.dimension.minecraft_name, self.command)
        {
            return Err(NO_JOB.create_without_context());
        }
        context.source.send_feedback(
            TextComponent::text(format!(
                "Pre-generation of {} {}.",
                world.dimension.minecraft_name, self.done
            )),
            true,
        );
        Ok(1)
    }
}

struct StatusExecutor;

impl CommandExecutor for StatusExecutor {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let world = context
            .source
            .world
            .as_ref()
            .ok_or_else(|| NO_WORLD.create_without_context())?;
        let dimension = world.dimension.minecraft_name;
        let report = context
            .source
            .server()
            .pregen
            .report(dimension)
            .ok_or_else(|| NO_JOB.create_without_context())?;
        context.source.send_feedback(
            TextComponent::text(format!("Pre-generation of {dimension} {report}")),
            false,
        );
        Ok(report.progress.percent() as i32)
    }
}

pub fn register(dispatcher: &mut CommandDispatcher, registry: &PermissionRegistry) {
    registry.register_permission_or_panic(Permission::new(
        PERMISSION,
        DESCRIPTION,
        PermissionDefault::Op(PermissionLvl::Three),
    ));

    let radius = || {
        argument(
            "radius",
            IntegerArgumentType::new(1, i32::try_from(MAX_RADIUS).unwrap_or(i32::MAX)),
        )
    };
    let shape = |name: &'static str, circle: bool| {
        literal(name).then(
            radius()
                .executes(StartExecutor { circle })
                .then(argument("centre", ColumnPosArgumentType).executes(StartExecutor { circle })),
        )
    };
    let control = |name: &'static str, command: Control, done: &'static str| {
        literal(name).executes(ControlExecutor { command, done })
    };

    dispatcher.register(
        command("pregen", DESCRIPTION)
            .requires(PERMISSION)
            .then(
                literal("start")
                    .then(shape("square", false))
                    .then(shape("circle", true)),
            )
            .then(control("pause", Control::Pause, "paused"))
            .then(control("resume", Control::Resume, "resumed"))
            .then(control("cancel", Control::Cancel, "cancelled"))
            .then(literal("status").executes(StatusExecutor)),
    );
}
