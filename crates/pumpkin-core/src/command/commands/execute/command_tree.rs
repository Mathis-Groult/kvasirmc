//! The command tree of `/execute`: one function per branch.
//!
//! `execute_command` lists the branches in the order the tree has always had; the modifiers and
//! executors the branches name live in the parent module with the rest of the command's logic.

use super::{
    ConditionalExecutor, DESCRIPTION, NumericConditionalExecutor, PERMISSION, add_conditional,
    count_blocks_match, count_entities, execute_align_modifier, execute_anchored_modifier,
    execute_as_modifier, execute_at_modifier, execute_facing_entity_modifier,
    execute_facing_modifier, execute_if_biome_modifier, execute_if_block_modifier,
    execute_if_blocks_all_modifier, execute_if_blocks_masked_modifier,
    execute_if_data_block_modifier, execute_if_data_entity_modifier,
    execute_if_data_storage_modifier, execute_if_dimension_modifier, execute_if_entity_modifier,
    execute_if_loaded_modifier, execute_if_score_eq_modifier, execute_if_score_ge_modifier,
    execute_if_score_gt_modifier, execute_if_score_le_modifier, execute_if_score_lt_modifier,
    execute_if_score_matches_modifier, execute_if_stopwatch_modifier, execute_in_modifier,
    execute_on_attacker_modifier, execute_on_controller_modifier, execute_on_leasher_modifier,
    execute_on_origin_modifier, execute_on_owner_modifier, execute_on_passengers_modifier,
    execute_on_target_modifier, execute_on_vehicle_modifier, execute_positioned_as_modifier,
    execute_positioned_modifier, execute_positioned_over_modifier, execute_rotated_as_modifier,
    execute_rotated_modifier, execute_summon_modifier, execute_unless_biome_modifier,
    execute_unless_block_modifier, execute_unless_blocks_all_modifier,
    execute_unless_blocks_masked_modifier, execute_unless_data_block_modifier,
    execute_unless_data_entity_modifier, execute_unless_data_storage_modifier,
    execute_unless_dimension_modifier, execute_unless_entity_modifier,
    execute_unless_loaded_modifier, execute_unless_score_eq_modifier,
    execute_unless_score_ge_modifier, execute_unless_score_gt_modifier,
    execute_unless_score_le_modifier, execute_unless_score_lt_modifier,
    execute_unless_score_matches_modifier, execute_unless_stopwatch_modifier,
};
use crate::command::argument_builder::{
    ArgumentBuilder, RequiredArgumentBuilder, argument, command, literal,
};
use crate::command::argument_builder::{CommandArgumentBuilder, LiteralArgumentBuilder};
use crate::command::argument_types::argument_type::JavaClientArgumentType;
use crate::command::argument_types::block_predicate::BlockPredicateArgumentType;
use crate::command::argument_types::client_parsed::ClientParsed;
use crate::command::argument_types::coordinates::block_pos::BlockPosArgumentType;
use crate::command::argument_types::coordinates::rotation::RotationArgumentType;
use crate::command::argument_types::coordinates::swizzle::SwizzleArgumentType;
use crate::command::argument_types::coordinates::vec3::Vec3ArgumentType;
use crate::command::argument_types::core::string::StringArgumentType;
use crate::command::argument_types::entity::EntityArgumentType;
use crate::command::argument_types::entity_anchor::EntityAnchorArgumentType;
use crate::command::argument_types::identifier::IdentifierArgumentType;
use crate::command::argument_types::nbt_path::NbtPathArgumentType;
use crate::command::argument_types::objective::ObjectiveArgumentType;
use crate::command::argument_types::range::{FloatRangeArgumentType, IntRangeArgumentType};
use crate::command::argument_types::resource::ENTITY_TYPE_ARGUMENT;
use crate::command::argument_types::resource_key::{
    BIOME_REGISTRY, DIMENSION_REGISTRY, ResourceKeyArgument,
};
use crate::command::argument_types::resource_or_tag::ResourceOrTagArgument;
use crate::command::argument_types::score_holder::ScoreHolderArgumentType;
use crate::command::node::{RedirectModifier, Redirection};
use std::sync::Arc;

/// The whole `/execute` command with its permission and every branch.
pub(super) fn execute_command() -> CommandArgumentBuilder {
    command("execute", DESCRIPTION)
        .requires(PERMISSION)
        .then(literal("run").redirect(Redirection::Root))
        .then(as_branch())
        .then(at_branch())
        .then(in_branch())
        .then(positioned_branch())
        .then(rotated_branch())
        .then(align_branch())
        .then(anchored_branch())
        .then(facing_branch())
        .then(summon_branch())
        .then(on_branch())
        .then(if_branch())
        .then(unless_branch())
}

/// The `as` branch of `/execute`.
fn as_branch() -> LiteralArgumentBuilder {
    literal("as").then(argument("targets", EntityArgumentType::Entities).fork(
        Redirection::Root,
        RedirectModifier::Custom(Arc::new(execute_as_modifier)),
    ))
}

/// The `at` branch of `/execute`.
fn at_branch() -> LiteralArgumentBuilder {
    literal("at").then(argument("targets", EntityArgumentType::Entities).fork(
        Redirection::Root,
        RedirectModifier::Custom(Arc::new(execute_at_modifier)),
    ))
}

/// The `in` branch of `/execute`.
fn in_branch() -> LiteralArgumentBuilder {
    literal("in").then(
        argument("dimension", ResourceKeyArgument(DIMENSION_REGISTRY)).redirect_with_modifier(
            Redirection::Root,
            RedirectModifier::Custom(Arc::new(execute_in_modifier)),
        ),
    )
}

/// The `positioned` branch of `/execute`.
fn positioned_branch() -> LiteralArgumentBuilder {
    literal("positioned")
        .then(
            literal("as").then(argument("targets", EntityArgumentType::Entities).fork(
                Redirection::Root,
                RedirectModifier::Custom(Arc::new(execute_positioned_as_modifier)),
            )),
        )
        .then(
            literal("over").then(
                argument(
                    "heightmap",
                    ClientParsed::new(
                        StringArgumentType::SingleWord,
                        JavaClientArgumentType::Heightmap,
                    ),
                )
                .redirect_with_modifier(
                    Redirection::Root,
                    RedirectModifier::Custom(Arc::new(execute_positioned_over_modifier)),
                ),
            ),
        )
        .then(
            argument("pos", Vec3ArgumentType::Default).redirect_with_modifier(
                Redirection::Root,
                RedirectModifier::Custom(Arc::new(execute_positioned_modifier)),
            ),
        )
}

/// The `rotated` branch of `/execute`.
fn rotated_branch() -> LiteralArgumentBuilder {
    literal("rotated")
        .then(
            literal("as").then(argument("targets", EntityArgumentType::Entities).fork(
                Redirection::Root,
                RedirectModifier::Custom(Arc::new(execute_rotated_as_modifier)),
            )),
        )
        .then(
            argument("rot", RotationArgumentType).redirect_with_modifier(
                Redirection::Root,
                RedirectModifier::Custom(Arc::new(execute_rotated_modifier)),
            ),
        )
}

/// The `align` branch of `/execute`.
fn align_branch() -> LiteralArgumentBuilder {
    literal("align").then(
        argument("axes", SwizzleArgumentType).redirect_with_modifier(
            Redirection::Root,
            RedirectModifier::Custom(Arc::new(execute_align_modifier)),
        ),
    )
}

/// The `anchored` branch of `/execute`.
fn anchored_branch() -> LiteralArgumentBuilder {
    literal("anchored").then(
        argument("anchor", EntityAnchorArgumentType).redirect_with_modifier(
            Redirection::Root,
            RedirectModifier::Custom(Arc::new(execute_anchored_modifier)),
        ),
    )
}

/// The `facing` branch of `/execute`.
fn facing_branch() -> LiteralArgumentBuilder {
    literal("facing")
        .then(
            literal("entity").then(argument("targets", EntityArgumentType::Entities).then(
                argument("anchor", EntityAnchorArgumentType).fork(
                    Redirection::Root,
                    RedirectModifier::Custom(Arc::new(execute_facing_entity_modifier)),
                ),
            )),
        )
        .then(
            argument("pos", Vec3ArgumentType::Default).redirect_with_modifier(
                Redirection::Root,
                RedirectModifier::Custom(Arc::new(execute_facing_modifier)),
            ),
        )
}

/// The `summon` branch of `/execute`.
fn summon_branch() -> LiteralArgumentBuilder {
    literal("summon").then(
        argument("entity", ENTITY_TYPE_ARGUMENT.clone()).redirect_with_modifier(
            Redirection::Root,
            RedirectModifier::Custom(Arc::new(execute_summon_modifier)),
        ),
    )
}

/// The `on` branch of `/execute`.
fn on_branch() -> LiteralArgumentBuilder {
    literal("on")
        .then(literal("owner").fork(
            Redirection::Root,
            RedirectModifier::Custom(Arc::new(execute_on_owner_modifier)),
        ))
        .then(literal("leasher").fork(
            Redirection::Root,
            RedirectModifier::Custom(Arc::new(execute_on_leasher_modifier)),
        ))
        .then(literal("target").fork(
            Redirection::Root,
            RedirectModifier::Custom(Arc::new(execute_on_target_modifier)),
        ))
        .then(literal("attacker").fork(
            Redirection::Root,
            RedirectModifier::Custom(Arc::new(execute_on_attacker_modifier)),
        ))
        .then(literal("vehicle").fork(
            Redirection::Root,
            RedirectModifier::Custom(Arc::new(execute_on_vehicle_modifier)),
        ))
        .then(literal("controller").fork(
            Redirection::Root,
            RedirectModifier::Custom(Arc::new(execute_on_controller_modifier)),
        ))
        .then(literal("origin").fork(
            Redirection::Root,
            RedirectModifier::Custom(Arc::new(execute_on_origin_modifier)),
        ))
        .then(literal("passengers").fork(
            Redirection::Root,
            RedirectModifier::Custom(Arc::new(execute_on_passengers_modifier)),
        ))
}

/// The `destination` argument of `/execute` below `if blocks start end`.
fn if_blocks_start_end_destination_argument() -> RequiredArgumentBuilder {
    argument("destination", BlockPosArgumentType)
        .then(
            literal("all")
                .fork(
                    Redirection::Root,
                    RedirectModifier::Custom(Arc::new(execute_if_blocks_all_modifier)),
                )
                .executes(NumericConditionalExecutor {
                    expected: true,
                    count: |context| count_blocks_match(context, false),
                }),
        )
        .then(
            literal("masked")
                .fork(
                    Redirection::Root,
                    RedirectModifier::Custom(Arc::new(execute_if_blocks_masked_modifier)),
                )
                .executes(NumericConditionalExecutor {
                    expected: true,
                    count: |context| count_blocks_match(context, true),
                }),
        )
}

/// The `targetObjective` argument of `/execute` below `if score target`.
fn if_score_target_target_objective_argument() -> RequiredArgumentBuilder {
    argument("targetObjective", ObjectiveArgumentType)
        .then(literal("matches").then(add_conditional(
            argument("range", IntRangeArgumentType),
            execute_if_score_matches_modifier,
        )))
        .then(
            literal("=").then(argument("source", ScoreHolderArgumentType::Single).then(
                add_conditional(
                    argument("sourceObjective", ObjectiveArgumentType),
                    execute_if_score_eq_modifier,
                ),
            )),
        )
        .then(
            literal("<").then(argument("source", ScoreHolderArgumentType::Single).then(
                add_conditional(
                    argument("sourceObjective", ObjectiveArgumentType),
                    execute_if_score_lt_modifier,
                ),
            )),
        )
        .then(
            literal("<=").then(argument("source", ScoreHolderArgumentType::Single).then(
                add_conditional(
                    argument("sourceObjective", ObjectiveArgumentType),
                    execute_if_score_le_modifier,
                ),
            )),
        )
        .then(
            literal(">").then(argument("source", ScoreHolderArgumentType::Single).then(
                add_conditional(
                    argument("sourceObjective", ObjectiveArgumentType),
                    execute_if_score_gt_modifier,
                ),
            )),
        )
        .then(
            literal(">=").then(argument("source", ScoreHolderArgumentType::Single).then(
                add_conditional(
                    argument("sourceObjective", ObjectiveArgumentType),
                    execute_if_score_ge_modifier,
                ),
            )),
        )
}

/// The `data` branch of `/execute` below `if`.
fn if_data_branch() -> LiteralArgumentBuilder {
    literal("data")
        .then(
            literal("block").then(
                argument("pos", BlockPosArgumentType).then(
                    argument("path", NbtPathArgumentType)
                        .fork(
                            Redirection::Root,
                            RedirectModifier::Custom(Arc::new(execute_if_data_block_modifier)),
                        )
                        .executes(ConditionalExecutor(execute_if_data_block_modifier)),
                ),
            ),
        )
        .then(
            literal("entity").then(
                argument("target", EntityArgumentType::Entity).then(
                    argument("path", NbtPathArgumentType)
                        .fork(
                            Redirection::Root,
                            RedirectModifier::Custom(Arc::new(execute_if_data_entity_modifier)),
                        )
                        .executes(ConditionalExecutor(execute_if_data_entity_modifier)),
                ),
            ),
        )
        .then(
            literal("storage").then(
                argument("id", IdentifierArgumentType).then(
                    argument("path", NbtPathArgumentType)
                        .fork(
                            Redirection::Root,
                            RedirectModifier::Custom(Arc::new(execute_if_data_storage_modifier)),
                        )
                        .executes(ConditionalExecutor(execute_if_data_storage_modifier)),
                ),
            ),
        )
}

/// The `if` branch of `/execute`.
fn if_branch() -> LiteralArgumentBuilder {
    literal("if")
        .then(
            literal("entity").then(
                argument("entities", EntityArgumentType::Entities)
                    .fork(
                        Redirection::Root,
                        RedirectModifier::Custom(Arc::new(execute_if_entity_modifier)),
                    )
                    .executes(NumericConditionalExecutor {
                        expected: true,
                        count: count_entities,
                    }),
            ),
        )
        .then(
            literal("block").then(
                argument("pos", BlockPosArgumentType).then(
                    argument("block", BlockPredicateArgumentType)
                        .fork(
                            Redirection::Root,
                            RedirectModifier::Custom(Arc::new(execute_if_block_modifier)),
                        )
                        .executes(ConditionalExecutor(execute_if_block_modifier)),
                ),
            ),
        )
        .then(
            literal("loaded").then(
                argument("pos", BlockPosArgumentType)
                    .fork(
                        Redirection::Root,
                        RedirectModifier::Custom(Arc::new(execute_if_loaded_modifier)),
                    )
                    .executes(ConditionalExecutor(execute_if_loaded_modifier)),
            ),
        )
        .then(
            literal("dimension").then(
                argument("dimension", ResourceKeyArgument(DIMENSION_REGISTRY))
                    .fork(
                        Redirection::Root,
                        RedirectModifier::Custom(Arc::new(execute_if_dimension_modifier)),
                    )
                    .executes(ConditionalExecutor(execute_if_dimension_modifier)),
            ),
        )
        .then(
            literal("biome").then(
                argument("pos", BlockPosArgumentType).then(
                    argument("biome", ResourceOrTagArgument(BIOME_REGISTRY.clone()))
                        .fork(
                            Redirection::Root,
                            RedirectModifier::Custom(Arc::new(execute_if_biome_modifier)),
                        )
                        .executes(ConditionalExecutor(execute_if_biome_modifier)),
                ),
            ),
        )
        .then(
            literal("blocks").then(
                argument("start", BlockPosArgumentType).then(
                    argument("end", BlockPosArgumentType)
                        .then(if_blocks_start_end_destination_argument()),
                ),
            ),
        )
        .then(
            literal("score").then(
                argument("target", ScoreHolderArgumentType::Single)
                    .then(if_score_target_target_objective_argument()),
            ),
        )
        .then(if_data_branch())
        .then(
            literal("stopwatch").then(
                argument("id", IdentifierArgumentType).then(
                    argument("range", FloatRangeArgumentType)
                        .fork(
                            Redirection::Root,
                            RedirectModifier::Custom(Arc::new(execute_if_stopwatch_modifier)),
                        )
                        .executes(ConditionalExecutor(execute_if_stopwatch_modifier)),
                ),
            ),
        )
}

/// The `destination` argument of `/execute` below `unless blocks start end`.
fn unless_blocks_start_end_destination_argument() -> RequiredArgumentBuilder {
    argument("destination", BlockPosArgumentType)
        .then(
            literal("all")
                .fork(
                    Redirection::Root,
                    RedirectModifier::Custom(Arc::new(execute_unless_blocks_all_modifier)),
                )
                .executes(NumericConditionalExecutor {
                    expected: false,
                    count: |context| count_blocks_match(context, false),
                }),
        )
        .then(
            literal("masked")
                .fork(
                    Redirection::Root,
                    RedirectModifier::Custom(Arc::new(execute_unless_blocks_masked_modifier)),
                )
                .executes(NumericConditionalExecutor {
                    expected: false,
                    count: |context| count_blocks_match(context, true),
                }),
        )
}

/// The `targetObjective` argument of `/execute` below `unless score target`.
fn unless_score_target_target_objective_argument() -> RequiredArgumentBuilder {
    argument("targetObjective", ObjectiveArgumentType)
        .then(literal("matches").then(add_conditional(
            argument("range", IntRangeArgumentType),
            execute_unless_score_matches_modifier,
        )))
        .then(
            literal("=").then(argument("source", ScoreHolderArgumentType::Single).then(
                add_conditional(
                    argument("sourceObjective", ObjectiveArgumentType),
                    execute_unless_score_eq_modifier,
                ),
            )),
        )
        .then(
            literal("<").then(argument("source", ScoreHolderArgumentType::Single).then(
                add_conditional(
                    argument("sourceObjective", ObjectiveArgumentType),
                    execute_unless_score_lt_modifier,
                ),
            )),
        )
        .then(
            literal("<=").then(argument("source", ScoreHolderArgumentType::Single).then(
                add_conditional(
                    argument("sourceObjective", ObjectiveArgumentType),
                    execute_unless_score_le_modifier,
                ),
            )),
        )
        .then(
            literal(">").then(argument("source", ScoreHolderArgumentType::Single).then(
                add_conditional(
                    argument("sourceObjective", ObjectiveArgumentType),
                    execute_unless_score_gt_modifier,
                ),
            )),
        )
        .then(
            literal(">=").then(argument("source", ScoreHolderArgumentType::Single).then(
                add_conditional(
                    argument("sourceObjective", ObjectiveArgumentType),
                    execute_unless_score_ge_modifier,
                ),
            )),
        )
}

/// The `data` branch of `/execute` below `unless`.
fn unless_data_branch() -> LiteralArgumentBuilder {
    literal("data")
        .then(
            literal("block").then(
                argument("pos", BlockPosArgumentType).then(
                    argument("path", NbtPathArgumentType)
                        .fork(
                            Redirection::Root,
                            RedirectModifier::Custom(Arc::new(execute_unless_data_block_modifier)),
                        )
                        .executes(ConditionalExecutor(execute_unless_data_block_modifier)),
                ),
            ),
        )
        .then(
            literal("entity").then(
                argument("target", EntityArgumentType::Entity).then(
                    argument("path", NbtPathArgumentType)
                        .fork(
                            Redirection::Root,
                            RedirectModifier::Custom(Arc::new(execute_unless_data_entity_modifier)),
                        )
                        .executes(ConditionalExecutor(execute_unless_data_entity_modifier)),
                ),
            ),
        )
        .then(
            literal("storage").then(
                argument("id", IdentifierArgumentType).then(
                    argument("path", NbtPathArgumentType)
                        .fork(
                            Redirection::Root,
                            RedirectModifier::Custom(Arc::new(
                                execute_unless_data_storage_modifier,
                            )),
                        )
                        .executes(ConditionalExecutor(execute_unless_data_storage_modifier)),
                ),
            ),
        )
}

/// The `unless` branch of `/execute`.
fn unless_branch() -> LiteralArgumentBuilder {
    literal("unless")
        .then(
            literal("entity").then(
                argument("entities", EntityArgumentType::Entities)
                    .fork(
                        Redirection::Root,
                        RedirectModifier::Custom(Arc::new(execute_unless_entity_modifier)),
                    )
                    .executes(NumericConditionalExecutor {
                        expected: false,
                        count: count_entities,
                    }),
            ),
        )
        .then(
            literal("block").then(
                argument("pos", BlockPosArgumentType).then(
                    argument("block", BlockPredicateArgumentType)
                        .fork(
                            Redirection::Root,
                            RedirectModifier::Custom(Arc::new(execute_unless_block_modifier)),
                        )
                        .executes(ConditionalExecutor(execute_unless_block_modifier)),
                ),
            ),
        )
        .then(
            literal("loaded").then(
                argument("pos", BlockPosArgumentType)
                    .fork(
                        Redirection::Root,
                        RedirectModifier::Custom(Arc::new(execute_unless_loaded_modifier)),
                    )
                    .executes(ConditionalExecutor(execute_unless_loaded_modifier)),
            ),
        )
        .then(
            literal("dimension").then(
                argument("dimension", ResourceKeyArgument(DIMENSION_REGISTRY))
                    .fork(
                        Redirection::Root,
                        RedirectModifier::Custom(Arc::new(execute_unless_dimension_modifier)),
                    )
                    .executes(ConditionalExecutor(execute_unless_dimension_modifier)),
            ),
        )
        .then(
            literal("biome").then(
                argument("pos", BlockPosArgumentType).then(
                    argument("biome", ResourceOrTagArgument(BIOME_REGISTRY.clone()))
                        .fork(
                            Redirection::Root,
                            RedirectModifier::Custom(Arc::new(execute_unless_biome_modifier)),
                        )
                        .executes(ConditionalExecutor(execute_unless_biome_modifier)),
                ),
            ),
        )
        .then(
            literal("blocks").then(
                argument("start", BlockPosArgumentType).then(
                    argument("end", BlockPosArgumentType)
                        .then(unless_blocks_start_end_destination_argument()),
                ),
            ),
        )
        .then(
            literal("score").then(
                argument("target", ScoreHolderArgumentType::Single)
                    .then(unless_score_target_target_objective_argument()),
            ),
        )
        .then(unless_data_branch())
        .then(
            literal("stopwatch").then(
                argument("id", IdentifierArgumentType).then(
                    argument("range", FloatRangeArgumentType)
                        .fork(
                            Redirection::Root,
                            RedirectModifier::Custom(Arc::new(execute_unless_stopwatch_modifier)),
                        )
                        .executes(ConditionalExecutor(execute_unless_stopwatch_modifier)),
                ),
            ),
        )
}
