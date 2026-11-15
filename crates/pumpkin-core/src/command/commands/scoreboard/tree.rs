//! The `/scoreboard` command tree.

use pumpkin_util::PermissionLvl;
use pumpkin_util::permission::{Permission, PermissionDefault, PermissionRegistry};

use crate::command::argument_builder::{
    ArgumentBuilder, LiteralArgumentBuilder, argument, command, literal,
};
use crate::command::argument_types::argument_type::JavaClientArgumentType;
use crate::command::argument_types::client_parsed::{ClientParsed, WordArgumentType};
use crate::command::argument_types::component::ComponentArgumentType;
use crate::command::argument_types::core::integer::IntegerArgumentType;
use crate::command::argument_types::core::string::StringArgumentType;
use crate::command::argument_types::objective::ObjectiveArgumentType;
use crate::command::argument_types::operation::OperationArgumentType;
use crate::command::argument_types::score_holder::ScoreHolderArgumentType;
use crate::command::node::dispatcher::CommandDispatcher;

use super::{
    ARG_CRITERION, ARG_DISPLAY_NAME, ARG_OBJECTIVE, ARG_OPERATION, ARG_SCORE, ARG_SOURCE_OBJECTIVE,
    ARG_SOURCE_TARGETS, ARG_TARGETS, DESCRIPTION, ObjectivesAddExecutor, ObjectivesRemoveExecutor,
    PERMISSION, PlayersAddRemoveExecutor, PlayersEnableExecutor, PlayersGetExecutor,
    PlayersOperationExecutor, PlayersResetExecutor, PlayersSetExecutor,
};

pub fn register(dispatcher: &mut CommandDispatcher, registry: &PermissionRegistry) {
    registry.register_permission_or_panic(Permission::new(
        PERMISSION,
        DESCRIPTION,
        PermissionDefault::Op(PermissionLvl::Two),
    ));

    dispatcher.register(
        command("scoreboard", DESCRIPTION)
            .requires(PERMISSION)
            .then(objectives_branch())
            .then(players_branch()),
    );
}

/// scoreboard objectives (add | remove)
fn objectives_branch() -> LiteralArgumentBuilder {
    literal("objectives")
        .then(
            literal("add").then(
                argument(ARG_OBJECTIVE, StringArgumentType::SingleWord).then(
                    argument(
                        ARG_CRITERION,
                        ClientParsed::new(
                            WordArgumentType,
                            JavaClientArgumentType::ObjectiveCriteria,
                        ),
                    )
                    .executes(ObjectivesAddExecutor {
                        has_display_name: false,
                    })
                    .then(
                        argument(ARG_DISPLAY_NAME, ComponentArgumentType).executes(
                            ObjectivesAddExecutor {
                                has_display_name: true,
                            },
                        ),
                    ),
                ),
            ),
        )
        .then(literal("remove").then(
            argument(ARG_OBJECTIVE, ObjectiveArgumentType).executes(ObjectivesRemoveExecutor),
        ))
}

/// scoreboard players (set | get | add | remove | reset | operation | enable)
fn players_branch() -> LiteralArgumentBuilder {
    literal("players")
        .then(literal("set").then(
            argument(ARG_TARGETS, ScoreHolderArgumentType::Multiple).then(
                argument(ARG_OBJECTIVE, ObjectiveArgumentType).then(
                    argument(ARG_SCORE, IntegerArgumentType::any()).executes(PlayersSetExecutor),
                ),
            ),
        ))
        .then(
            literal("get").then(
                argument(ARG_TARGETS, ScoreHolderArgumentType::Single).then(
                    argument(ARG_OBJECTIVE, ObjectiveArgumentType).executes(PlayersGetExecutor),
                ),
            ),
        )
        .then(
            literal("add").then(
                argument(ARG_TARGETS, ScoreHolderArgumentType::Multiple).then(
                    argument(ARG_OBJECTIVE, ObjectiveArgumentType).then(
                        argument(ARG_SCORE, IntegerArgumentType::with_min(0))
                            .executes(PlayersAddRemoveExecutor { remove: false }),
                    ),
                ),
            ),
        )
        .then(
            literal("remove").then(
                argument(ARG_TARGETS, ScoreHolderArgumentType::Multiple).then(
                    argument(ARG_OBJECTIVE, ObjectiveArgumentType).then(
                        argument(ARG_SCORE, IntegerArgumentType::with_min(0))
                            .executes(PlayersAddRemoveExecutor { remove: true }),
                    ),
                ),
            ),
        )
        .then(
            literal("reset").then(
                argument(ARG_TARGETS, ScoreHolderArgumentType::Multiple)
                    .executes(PlayersResetExecutor {
                        has_objective: false,
                    })
                    .then(argument(ARG_OBJECTIVE, ObjectiveArgumentType).executes(
                        PlayersResetExecutor {
                            has_objective: true,
                        },
                    )),
            ),
        )
        .then(
            literal("operation").then(
                argument(ARG_TARGETS, ScoreHolderArgumentType::Multiple).then(
                    argument(ARG_OBJECTIVE, ObjectiveArgumentType).then(
                        argument(ARG_OPERATION, OperationArgumentType).then(
                            argument(ARG_SOURCE_TARGETS, ScoreHolderArgumentType::Multiple).then(
                                argument(ARG_SOURCE_OBJECTIVE, ObjectiveArgumentType)
                                    .executes(PlayersOperationExecutor),
                            ),
                        ),
                    ),
                ),
            ),
        )
        .then(literal("enable").then(
            argument(ARG_TARGETS, ScoreHolderArgumentType::Multiple).then(
                argument(ARG_OBJECTIVE, ObjectiveArgumentType).executes(PlayersEnableExecutor),
            ),
        ))
}
