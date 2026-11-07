use std::cell::Cell;
use std::sync::Arc;

use pumpkin_data::translation;
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;
use pumpkin_util::PermissionLvl;
use pumpkin_util::identifier::Identifier;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::permission::{Permission, PermissionDefault, PermissionRegistry};
use pumpkin_util::text::TextComponent;
use rand::RngExt;

use crate::command::argument_builder::{ArgumentBuilder, argument, command, literal};
use crate::command::argument_types::FromStringReader;
use crate::command::argument_types::argument_type::{ArgumentType, JavaClientArgumentType};
use crate::command::argument_types::client_parsed::ClientParsed;
use crate::command::argument_types::coordinates::block_pos::BlockPosArgumentType;
use crate::command::argument_types::core::double::DoubleArgumentType;
use crate::command::argument_types::entity::EntityArgumentType;
use crate::command::context::command_context::CommandContext;
use crate::command::context::command_source::CommandSource;
use crate::command::errors::command_syntax_error::CommandSyntaxError;
use crate::command::errors::error_types::{CommandErrorType, LiteralCommandErrorType};
use crate::command::node::dispatcher::CommandDispatcher;
use crate::command::node::{CommandExecutor, CommandExecutorResult};
use crate::command::snbt::SnbtParser;
use crate::command::string_reader::StringReader;
use crate::command::suggestion::provider::{SuggestionProvider, SuggestionProviderResult};
use crate::command::suggestion::suggestions::SuggestionsBuilder;
use crate::data::datapack::DatapackManager;
use crate::entity::EntityBase;

mod float_ops;
use float_ops::evaluate_compound_float;

const DESCRIPTION: &str = "Evaluates number providers.";
const PERMISSION: &str = "minecraft:command.compute";

const ARG_POS: &str = "computePos";
const ARG_TARGET: &str = "computeTarget";
const ARG_INT_PROVIDER: &str = "provider";
const ARG_FLOAT_PROVIDER: &str = "provider";
const ARG_SCALE: &str = "scale";

const MAX_PROVIDER_DEPTH: usize = 64;

const ERROR_DIV_BY_ZERO: LiteralCommandErrorType = LiteralCommandErrorType::new("Division by zero");
const ERROR_POW_ZERO_ZERO: LiteralCommandErrorType =
    LiteralCommandErrorType::new("0^0 is undefined");
const ERROR_EVALUATION_FAILED: LiteralCommandErrorType =
    LiteralCommandErrorType::new("Failed to evaluate number provider");

#[allow(dead_code)]
pub const ERROR_BLOCK_INVALID: CommandErrorType<0> = CommandErrorType::new(
    translation::java::COMMANDS_DATA_BLOCK_INVALID,
    translation::java::COMMANDS_DATA_BLOCK_INVALID,
);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TargetType {
    Default,
    Block,
    Entity,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProviderArg {
    Number(f64),
    Nbt(NbtCompound),
    Id(Identifier),
}

pub struct ProviderArgumentType;

impl ArgumentType<CommandSource> for ProviderArgumentType {
    type Item = ProviderArg;

    fn parse(&self, reader: &mut StringReader) -> Result<Self::Item, CommandSyntaxError> {
        reader.skip_whitespace();
        if reader.peek() == Some('{') {
            let tag = SnbtParser::parse_for_commands(reader)?;
            if let NbtTag::Compound(compound) = tag {
                return Ok(ProviderArg::Nbt(compound));
            }
        }

        let start = reader.cursor();
        if let Ok(num) = reader.read_double() {
            let is_word = reader
                .peek()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == ':');
            if !is_word {
                return Ok(ProviderArg::Number(num));
            }
        }
        reader.set_cursor(start);

        let id = Identifier::from_reader(reader)?;
        Ok(ProviderArg::Id(id))
    }

    fn client_side_parser(&self) -> JavaClientArgumentType {
        JavaClientArgumentType::NbtTag
    }

    fn examples(&self) -> Vec<String> {
        vec![
            "5".to_string(),
            "3.14".to_string(),
            "minecraft:constant".to_string(),
            "{type:\"constant\",value:5}".to_string(),
            "{type:\"add\",left:1,right:2}".to_string(),
        ]
    }
}

impl ProviderArgumentType {
    pub fn get<'a>(
        context: &'a CommandContext,
        name: &str,
    ) -> Result<&'a ProviderArg, CommandSyntaxError> {
        context.get_argument(name)
    }
}

pub struct ComputeContext<'a> {
    pub source: &'a CommandSource,
    #[allow(dead_code)]
    pub target_type: TargetType,
    #[allow(dead_code)]
    pub block_pos: Option<BlockPos>,
    pub target_entity: Option<Arc<dyn EntityBase>>,
    pub datapack_manager: Option<&'a DatapackManager>,
    pub depth: Cell<usize>,
}

fn build_context<'a>(
    context: &'a CommandContext,
    target_type: TargetType,
) -> Result<ComputeContext<'a>, CommandSyntaxError> {
    let datapack_manager = context
        .source
        .server
        .as_ref()
        .map(|s| s.datapack_manager.as_ref());

    match target_type {
        TargetType::Default => Ok(ComputeContext {
            source: context.source.as_ref(),
            target_type: TargetType::Default,
            block_pos: None,
            target_entity: None,
            datapack_manager,
            depth: Cell::new(0),
        }),
        TargetType::Block => {
            let pos = BlockPosArgumentType::get_block_pos(context, ARG_POS)?;
            Ok(ComputeContext {
                source: context.source.as_ref(),
                target_type: TargetType::Block,
                block_pos: Some(pos),
                target_entity: None,
                datapack_manager,
                depth: Cell::new(0),
            })
        }
        TargetType::Entity => {
            let entity = EntityArgumentType::get_entity(context, ARG_TARGET)?;
            Ok(ComputeContext {
                source: context.source.as_ref(),
                target_type: TargetType::Entity,
                block_pos: None,
                target_entity: Some(entity),
                datapack_manager,
                depth: Cell::new(0),
            })
        }
    }
}

fn resolve_target_name(target_tag: &NbtTag, ctx: &ComputeContext) -> Option<String> {
    match target_tag {
        NbtTag::String(s) => Some(s.to_string()),
        NbtTag::Compound(c) => {
            let t = c.get_string("target")?;
            match t {
                "target_entity" => ctx.target_entity.as_ref().map(|e| e.get_scoreboard_name()),
                "this" => ctx.source.entity.as_ref().map(|e| e.get_scoreboard_name()),
                _ => None,
            }
        }
        _ => None,
    }
}

fn evaluate_score_provider_int(
    compound: &NbtCompound,
    ctx: &ComputeContext,
) -> Result<i64, CommandSyntaxError> {
    let score_name = compound
        .get_string("score")
        .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?;
    let target_name = compound
        .get("target")
        .and_then(|t| resolve_target_name(t, ctx));

    if let (Some(target), Some(world)) = (target_name, ctx.source.world.as_ref()) {
        let scoreboard = world.scoreboard.lock().unwrap();
        if let Some(score) = scoreboard.get_score(&target, score_name) {
            return Ok(i64::from(score.value.0));
        }
    }

    compound
        .get("fallback")
        .map_or(Ok(0), |fallback| evaluate_tag_int(fallback, ctx))
}

fn evaluate_score_provider_float(
    compound: &NbtCompound,
    ctx: &ComputeContext,
) -> Result<f64, CommandSyntaxError> {
    let score_name = compound
        .get_string("score")
        .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?;
    let target_name = compound
        .get("target")
        .and_then(|t| resolve_target_name(t, ctx));

    if let (Some(target), Some(world)) = (target_name, ctx.source.world.as_ref()) {
        let scoreboard = world.scoreboard.lock().unwrap();
        if let Some(score) = scoreboard.get_score(&target, score_name) {
            return Ok(f64::from(score.value.0));
        }
    }

    compound
        .get("fallback")
        .map_or(Ok(0.0), |fallback| evaluate_tag_float(fallback, ctx))
}

fn resolve_int_provider_id(id: &str, ctx: &ComputeContext) -> Result<i64, CommandSyntaxError> {
    if ctx.depth.get() >= MAX_PROVIDER_DEPTH {
        return Err(ERROR_EVALUATION_FAILED.create_without_context());
    }

    let tag = ctx
        .datapack_manager
        .and_then(|dm| {
            dm.get_context_int_provider(id).or_else(|| {
                if id.contains(':') {
                    None
                } else {
                    dm.get_context_int_provider(&format!("minecraft:{id}"))
                }
            })
        })
        .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?;

    ctx.depth.set(ctx.depth.get() + 1);
    let res = evaluate_tag_int(&tag, ctx);
    ctx.depth.set(ctx.depth.get() - 1);
    res
}

fn evaluate_tag_int(tag: &NbtTag, ctx: &ComputeContext) -> Result<i64, CommandSyntaxError> {
    match tag {
        NbtTag::Byte(b) => Ok(i64::from(*b)),
        NbtTag::Short(s) => Ok(i64::from(*s)),
        NbtTag::Int(i) => Ok(i64::from(*i)),
        NbtTag::Long(l) => Ok(*l),
        #[allow(clippy::cast_possible_truncation)]
        NbtTag::Float(f) => Ok(*f as i64),
        #[allow(clippy::cast_possible_truncation)]
        NbtTag::Double(d) => Ok(*d as i64),
        NbtTag::String(s) => s
            .parse::<i64>()
            .map_or_else(|_| resolve_int_provider_id(s, ctx), Ok),
        NbtTag::Compound(compound) => evaluate_compound_int(compound, ctx),
        _ => Err(ERROR_EVALUATION_FAILED.create_without_context()),
    }
}

#[allow(clippy::too_many_lines, clippy::match_same_arms)]
fn evaluate_compound_int(
    compound: &NbtCompound,
    ctx: &ComputeContext,
) -> Result<i64, CommandSyntaxError> {
    let op_type = compound
        .get_string("type")
        .map(|s| s.strip_prefix("minecraft:").unwrap_or(s));

    match op_type {
        Some("constant") => compound.get("value").map_or_else(
            || Err(ERROR_EVALUATION_FAILED.create_without_context()),
            |val_tag| evaluate_tag_int(val_tag, ctx),
        ),
        Some("add" | "sum") => {
            if let Some(NbtTag::List(inputs)) = compound.get("inputs") {
                let mut sum = 0i64;
                for tag in inputs {
                    sum = sum.wrapping_add(evaluate_tag_int(tag, ctx)?);
                }
                Ok(sum)
            } else {
                let left = evaluate_tag_int(
                    compound
                        .get("left")
                        .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                    ctx,
                )?;
                let right = evaluate_tag_int(
                    compound
                        .get("right")
                        .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                    ctx,
                )?;
                Ok(left.wrapping_add(right))
            }
        }
        Some("sub") => {
            let left = evaluate_tag_int(
                compound
                    .get("left")
                    .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                ctx,
            )?;
            let right = evaluate_tag_int(
                compound
                    .get("right")
                    .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                ctx,
            )?;
            Ok(left.wrapping_sub(right))
        }
        Some("mul" | "product") => {
            if let Some(NbtTag::List(inputs)) = compound.get("inputs") {
                let mut prod = 1i64;
                for tag in inputs {
                    prod = prod.wrapping_mul(evaluate_tag_int(tag, ctx)?);
                }
                Ok(prod)
            } else {
                let left = evaluate_tag_int(
                    compound
                        .get("left")
                        .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                    ctx,
                )?;
                let right = evaluate_tag_int(
                    compound
                        .get("right")
                        .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                    ctx,
                )?;
                Ok(left.wrapping_mul(right))
            }
        }
        Some("div") => {
            let left = evaluate_tag_int(
                compound
                    .get("left")
                    .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                ctx,
            )?;
            let right = evaluate_tag_int(
                compound
                    .get("right")
                    .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                ctx,
            )?;
            if right == 0 {
                return Err(ERROR_DIV_BY_ZERO.create_without_context());
            }
            Ok(left / right)
        }
        Some("floor_div") => {
            let left = evaluate_tag_int(
                compound
                    .get("left")
                    .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                ctx,
            )?;
            let right = evaluate_tag_int(
                compound
                    .get("right")
                    .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                ctx,
            )?;
            if right == 0 {
                return Err(ERROR_DIV_BY_ZERO.create_without_context());
            }
            #[allow(clippy::cast_possible_truncation)]
            Ok(((left as f64) / (right as f64)).floor() as i64)
        }
        Some("floor_mod") => {
            let left = evaluate_tag_int(
                compound
                    .get("left")
                    .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                ctx,
            )?;
            let right = evaluate_tag_int(
                compound
                    .get("right")
                    .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                ctx,
            )?;
            if right == 0 {
                return Err(ERROR_DIV_BY_ZERO.create_without_context());
            }
            Ok(((left % right) + right) % right)
        }
        Some("mod") => {
            let left = evaluate_tag_int(
                compound
                    .get("left")
                    .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                ctx,
            )?;
            let right = evaluate_tag_int(
                compound
                    .get("right")
                    .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                ctx,
            )?;
            if right == 0 {
                return Err(ERROR_DIV_BY_ZERO.create_without_context());
            }
            Ok(left % right)
        }
        Some("pow") => {
            let base = evaluate_tag_int(
                compound
                    .get("base")
                    .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                ctx,
            )?;
            let exponent = evaluate_tag_int(
                compound
                    .get("exponent")
                    .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                ctx,
            )?;
            if base == 0 && exponent == 0 {
                return Err(ERROR_POW_ZERO_ZERO.create_without_context());
            }
            if exponent < 0 {
                return Ok(0);
            }
            #[allow(clippy::cast_sign_loss)]
            Ok(base.wrapping_pow(exponent as u32))
        }
        Some("abs") => {
            let input = evaluate_tag_int(
                compound
                    .get("input")
                    .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                ctx,
            )?;
            Ok(input.abs())
        }
        Some("negate") => {
            let input = evaluate_tag_int(
                compound
                    .get("input")
                    .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                ctx,
            )?;
            Ok(-input)
        }
        Some("min") => {
            if let Some(NbtTag::List(inputs)) = compound.get("inputs") {
                if inputs.is_empty() {
                    return Err(ERROR_EVALUATION_FAILED.create_without_context());
                }
                let mut min_val = i64::MAX;
                for tag in inputs {
                    min_val = min_val.min(evaluate_tag_int(tag, ctx)?);
                }
                Ok(min_val)
            } else {
                let left = evaluate_tag_int(
                    compound
                        .get("left")
                        .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                    ctx,
                )?;
                let right = evaluate_tag_int(
                    compound
                        .get("right")
                        .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                    ctx,
                )?;
                Ok(left.min(right))
            }
        }
        Some("max") => {
            if let Some(NbtTag::List(inputs)) = compound.get("inputs") {
                if inputs.is_empty() {
                    return Err(ERROR_EVALUATION_FAILED.create_without_context());
                }
                let mut max_val = i64::MIN;
                for tag in inputs {
                    max_val = max_val.max(evaluate_tag_int(tag, ctx)?);
                }
                Ok(max_val)
            } else {
                let left = evaluate_tag_int(
                    compound
                        .get("left")
                        .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                    ctx,
                )?;
                let right = evaluate_tag_int(
                    compound
                        .get("right")
                        .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                    ctx,
                )?;
                Ok(left.max(right))
            }
        }
        Some("avg" | "average") => {
            if let Some(NbtTag::List(inputs)) = compound.get("inputs") {
                if inputs.is_empty() {
                    return Err(ERROR_EVALUATION_FAILED.create_without_context());
                }
                let mut sum = 0i64;
                for tag in inputs {
                    sum = sum.wrapping_add(evaluate_tag_int(tag, ctx)?);
                }
                #[allow(clippy::cast_possible_wrap)]
                Ok(sum / (inputs.len() as i64))
            } else {
                Err(ERROR_EVALUATION_FAILED.create_without_context())
            }
        }
        Some("uniform") => {
            let min = evaluate_tag_int(
                compound
                    .get("min")
                    .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                ctx,
            )?;
            let max = evaluate_tag_int(
                compound
                    .get("max")
                    .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?,
                ctx,
            )?;
            if min > max {
                return Err(ERROR_EVALUATION_FAILED.create_without_context());
            }
            Ok(rand::rng().random_range(min..=max))
        }
        Some("score") => evaluate_score_provider_int(compound, ctx),
        _ => compound.get("value").map_or_else(
            || Err(ERROR_EVALUATION_FAILED.create_without_context()),
            |val_tag| evaluate_tag_int(val_tag, ctx),
        ),
    }
}

fn resolve_float_provider_id(id: &str, ctx: &ComputeContext) -> Result<f64, CommandSyntaxError> {
    if ctx.depth.get() >= MAX_PROVIDER_DEPTH {
        return Err(ERROR_EVALUATION_FAILED.create_without_context());
    }

    let tag = ctx
        .datapack_manager
        .and_then(|dm| {
            dm.get_context_float_provider(id).or_else(|| {
                if id.contains(':') {
                    None
                } else {
                    dm.get_context_float_provider(&format!("minecraft:{id}"))
                }
            })
        })
        .ok_or_else(|| ERROR_EVALUATION_FAILED.create_without_context())?;

    ctx.depth.set(ctx.depth.get() + 1);
    let res = evaluate_tag_float(&tag, ctx);
    ctx.depth.set(ctx.depth.get() - 1);
    res
}

fn evaluate_tag_float(tag: &NbtTag, ctx: &ComputeContext) -> Result<f64, CommandSyntaxError> {
    match tag {
        NbtTag::Byte(b) => Ok(f64::from(*b)),
        NbtTag::Short(s) => Ok(f64::from(*s)),
        NbtTag::Int(i) => Ok(f64::from(*i)),
        NbtTag::Long(l) =>
        {
            #[allow(clippy::cast_precision_loss)]
            Ok(*l as f64)
        }
        NbtTag::Float(f) => Ok(f64::from(*f)),
        NbtTag::Double(d) => Ok(*d),
        NbtTag::String(s) => s
            .parse::<f64>()
            .map_or_else(|_| resolve_float_provider_id(s, ctx), Ok),
        NbtTag::Compound(compound) => evaluate_compound_float(compound, ctx),
        _ => Err(ERROR_EVALUATION_FAILED.create_without_context()),
    }
}

fn evaluate_int(
    provider: &ProviderArg,
    ctx: &ComputeContext,
) -> Result<(i64, Option<String>), CommandSyntaxError> {
    match provider {
        #[allow(clippy::cast_possible_truncation)]
        ProviderArg::Number(n) => Ok((*n as i64, None)),
        ProviderArg::Nbt(compound) => {
            let val = evaluate_compound_int(compound, ctx)?;
            Ok((val, None))
        }
        ProviderArg::Id(id) => {
            let id_str = id.to_string();
            let val = resolve_int_provider_id(&id_str, ctx)?;
            Ok((val, Some(id_str)))
        }
    }
}

fn evaluate_float(
    provider: &ProviderArg,
    ctx: &ComputeContext,
) -> Result<(f64, Option<String>), CommandSyntaxError> {
    match provider {
        ProviderArg::Number(n) => Ok((*n, None)),
        ProviderArg::Nbt(compound) => {
            let val = evaluate_compound_float(compound, ctx)?;
            Ok((val, None))
        }
        ProviderArg::Id(id) => {
            let id_str = id.to_string();
            let val = resolve_float_provider_id(&id_str, ctx)?;
            Ok((val, Some(id_str)))
        }
    }
}

struct ComputeIntegerExecutor {
    target_type: TargetType,
}

impl CommandExecutor for ComputeIntegerExecutor {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let provider = ProviderArgumentType::get(context, ARG_INT_PROVIDER)?;
        let ctx = build_context(context, self.target_type)?;

        let (value, name) = evaluate_int(provider, &ctx)?;
        #[allow(clippy::cast_possible_truncation)]
        let result = value as i32;

        let msg = name.map_or_else(
            || {
                TextComponent::translate_cross(
                    translation::java::COMMAND_COMPUTE_RESULT_UNNAMED_EXACT,
                    translation::java::COMMAND_COMPUTE_RESULT_UNNAMED_EXACT,
                    [TextComponent::text(result.to_string())],
                )
            },
            |name| {
                TextComponent::translate_cross(
                    translation::java::COMMAND_COMPUTE_RESULT_NAMED_EXACT,
                    translation::java::COMMAND_COMPUTE_RESULT_NAMED_EXACT,
                    [
                        TextComponent::text(name),
                        TextComponent::text(result.to_string()),
                    ],
                )
            },
        );

        context.source.send_feedback(msg, false);
        Ok(result)
    }
}

struct ComputeFloatExecutor {
    target_type: TargetType,
    has_scale: bool,
}

impl CommandExecutor for ComputeFloatExecutor {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let provider = ProviderArgumentType::get(context, ARG_FLOAT_PROVIDER)?;
        let scale = if self.has_scale {
            DoubleArgumentType::get(context, ARG_SCALE)?
        } else {
            1.0
        };

        let ctx = build_context(context, self.target_type)?;
        let (raw_value, name) = evaluate_float(provider, &ctx)?;

        if raw_value.is_nan() || raw_value.is_infinite() {
            let msg = name.as_deref().map_or_else(
                || {
                    TextComponent::translate_cross(
                        translation::java::COMMAND_COMPUTE_RESULT_UNNAMED_INVALID,
                        translation::java::COMMAND_COMPUTE_RESULT_UNNAMED_INVALID,
                        [TextComponent::text(raw_value.to_string())],
                    )
                },
                |name| {
                    TextComponent::translate_cross(
                        translation::java::COMMAND_COMPUTE_RESULT_NAMED_INVALID,
                        translation::java::COMMAND_COMPUTE_RESULT_NAMED_INVALID,
                        [
                            TextComponent::text(name.to_string()),
                            TextComponent::text(raw_value.to_string()),
                        ],
                    )
                },
            );
            context.source.send_feedback(msg, false);
            return Err(ERROR_EVALUATION_FAILED.create_without_context());
        }

        let scaled = raw_value * scale;
        #[allow(clippy::cast_possible_truncation)]
        let rounded = scaled.floor() as i32;
        let is_exact = scaled == f64::from(rounded);

        let msg = match (name, is_exact) {
            (Some(name), true) => TextComponent::translate_cross(
                translation::java::COMMAND_COMPUTE_RESULT_NAMED_EXACT,
                translation::java::COMMAND_COMPUTE_RESULT_NAMED_EXACT,
                [
                    TextComponent::text(name),
                    TextComponent::text(scaled.to_string()),
                ],
            ),
            (Some(name), false) => TextComponent::translate_cross(
                translation::java::COMMAND_COMPUTE_RESULT_NAMED_ROUNDED,
                translation::java::COMMAND_COMPUTE_RESULT_NAMED_ROUNDED,
                [
                    TextComponent::text(name),
                    TextComponent::text(scaled.to_string()),
                    TextComponent::text(rounded.to_string()),
                ],
            ),
            (None, true) => TextComponent::translate_cross(
                translation::java::COMMAND_COMPUTE_RESULT_UNNAMED_EXACT,
                translation::java::COMMAND_COMPUTE_RESULT_UNNAMED_EXACT,
                [TextComponent::text(scaled.to_string())],
            ),
            (None, false) => TextComponent::translate_cross(
                translation::java::COMMAND_COMPUTE_RESULT_UNNAMED_ROUNDED,
                translation::java::COMMAND_COMPUTE_RESULT_UNNAMED_ROUNDED,
                [
                    TextComponent::text(scaled.to_string()),
                    TextComponent::text(rounded.to_string()),
                ],
            ),
        };

        context.source.send_feedback(msg, false);
        Ok(rounded)
    }
}

struct ContextIntProviderSuggestionProvider;

impl SuggestionProvider for ContextIntProviderSuggestionProvider {
    fn suggest(
        &self,
        context: &CommandContext,
        mut builder: SuggestionsBuilder,
    ) -> SuggestionProviderResult {
        if let Some(server) = context.source.server.as_ref() {
            for name in server.datapack_manager.get_context_int_provider_names() {
                builder = builder.suggest(name);
            }
        }
        builder.build()
    }
}

struct ContextFloatProviderSuggestionProvider;

impl SuggestionProvider for ContextFloatProviderSuggestionProvider {
    fn suggest(
        &self,
        context: &CommandContext,
        mut builder: SuggestionsBuilder,
    ) -> SuggestionProviderResult {
        if let Some(server) = context.source.server.as_ref() {
            for name in server.datapack_manager.get_context_float_provider_names() {
                builder = builder.suggest(name);
            }
        }
        builder.build()
    }
}

fn add_provider_nodes<B: ArgumentBuilder>(builder: B, target_type: TargetType) -> B {
    builder
        .then(
            literal("integer").then(
                argument(
                    ARG_INT_PROVIDER,
                    ClientParsed::new(
                        ProviderArgumentType,
                        JavaClientArgumentType::ContextIntProvider,
                    ),
                )
                .suggests(ContextIntProviderSuggestionProvider)
                .executes(ComputeIntegerExecutor { target_type }),
            ),
        )
        .then(
            literal("float").then(
                argument(
                    ARG_FLOAT_PROVIDER,
                    ClientParsed::new(
                        ProviderArgumentType,
                        JavaClientArgumentType::ContextFloatProvider,
                    ),
                )
                .suggests(ContextFloatProviderSuggestionProvider)
                .executes(ComputeFloatExecutor {
                    target_type,
                    has_scale: false,
                })
                .then(
                    argument(
                        ARG_SCALE,
                        // Vanilla's scale is a float.
                        ClientParsed::new(
                            DoubleArgumentType::any(),
                            JavaClientArgumentType::Float {
                                min: None,
                                max: None,
                            },
                        ),
                    )
                    .executes(ComputeFloatExecutor {
                        target_type,
                        has_scale: true,
                    }),
                ),
            ),
        )
}

pub fn register(dispatcher: &mut CommandDispatcher, registry: &PermissionRegistry) {
    registry.register_permission_or_panic(Permission::new(
        PERMISSION,
        DESCRIPTION,
        PermissionDefault::Op(PermissionLvl::Two),
    ));

    let default_node = add_provider_nodes(literal("default"), TargetType::Default);
    let block_arg = add_provider_nodes(argument(ARG_POS, BlockPosArgumentType), TargetType::Block);
    let block_node = literal("block").then(block_arg);
    let entity_arg = add_provider_nodes(
        argument(ARG_TARGET, EntityArgumentType::Entity),
        TargetType::Entity,
    );
    let entity_node = literal("entity").then(entity_arg);

    dispatcher.register(
        command("compute", DESCRIPTION)
            .requires(PERMISSION)
            .then(default_node)
            .then(block_node)
            .then(entity_node),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn parse_compute() {
        let mut dispatcher = CommandDispatcher::new();
        let registry = PermissionRegistry::default();
        register(&mut dispatcher, &registry);

        let source = Arc::new(CommandSource::dummy());

        let res = dispatcher.parse_input("compute default integer 42", &source);
        assert!(res.errors.is_empty(), "{:?}", res.errors);

        let res = dispatcher.parse_input("compute default float 3.14", &source);
        assert!(res.errors.is_empty(), "{:?}", res.errors);

        let res = dispatcher.parse_input("compute default float 3.14 2.0", &source);
        assert!(res.errors.is_empty(), "{:?}", res.errors);

        let res = dispatcher.parse_input("compute block ~ ~ ~ integer 42", &source);
        assert!(res.errors.is_empty(), "{:?}", res.errors);

        let res = dispatcher.parse_input("compute block ~ ~ ~ float 3.14", &source);
        assert!(res.errors.is_empty(), "{:?}", res.errors);

        let res = dispatcher.parse_input("compute block ~ ~ ~ float 3.14 2.0", &source);
        assert!(res.errors.is_empty(), "{:?}", res.errors);

        let res = dispatcher.parse_input("compute entity @s integer 42", &source);
        assert!(res.errors.is_empty(), "{:?}", res.errors);

        let res = dispatcher.parse_input("compute entity @s float 3.14", &source);
        assert!(res.errors.is_empty(), "{:?}", res.errors);

        let res = dispatcher.parse_input("compute entity @s float 3.14 2.0", &source);
        assert!(res.errors.is_empty(), "{:?}", res.errors);

        let res = dispatcher.parse_input(
            "compute default integer {type:\"constant\",value:5}",
            &source,
        );
        assert!(res.errors.is_empty(), "{:?}", res.errors);

        let res = dispatcher.parse_input(
            "compute default float {type:\"add\",left:1.5,right:2.5} 1.0",
            &source,
        );
        assert!(res.errors.is_empty(), "{:?}", res.errors);
    }

    fn parse_compound(snbt: &str) -> NbtCompound {
        let tag = SnbtParser::parse_for_commands(&mut StringReader::new(snbt)).unwrap();
        match tag {
            NbtTag::Compound(c) => c,
            _ => panic!("Expected compound"),
        }
    }

    #[test]
    fn evaluate_int_works() {
        let source = CommandSource::dummy();
        let ctx = ComputeContext {
            source: &source,
            target_type: TargetType::Default,
            block_pos: None,
            target_entity: None,
            datapack_manager: None,
            depth: Cell::new(0),
        };

        let c = parse_compound("{type:\"constant\",value:42}");
        assert_eq!(evaluate_compound_int(&c, &ctx).unwrap(), 42);

        let c = parse_compound("{type:\"add\",left:10,right:32}");
        assert_eq!(evaluate_compound_int(&c, &ctx).unwrap(), 42);

        let c = parse_compound("{type:\"add\",inputs:[1,2,3,4]}");
        assert_eq!(evaluate_compound_int(&c, &ctx).unwrap(), 10);

        let c = parse_compound("{type:\"sub\",left:50,right:8}");
        assert_eq!(evaluate_compound_int(&c, &ctx).unwrap(), 42);

        let c = parse_compound("{type:\"mul\",left:6,right:7}");
        assert_eq!(evaluate_compound_int(&c, &ctx).unwrap(), 42);

        let c = parse_compound("{type:\"mul\",inputs:[2,3,4]}");
        assert_eq!(evaluate_compound_int(&c, &ctx).unwrap(), 24);

        let c = parse_compound("{type:\"div\",left:84,right:2}");
        assert_eq!(evaluate_compound_int(&c, &ctx).unwrap(), 42);

        let c = parse_compound("{type:\"div\",left:84,right:0}");
        assert!(evaluate_compound_int(&c, &ctx).is_err());

        let c = parse_compound("{type:\"floor_div\",left:7,right:2}");
        assert_eq!(evaluate_compound_int(&c, &ctx).unwrap(), 3);

        let c = parse_compound("{type:\"floor_mod\",left:-1,right:5}");
        assert_eq!(evaluate_compound_int(&c, &ctx).unwrap(), 4);

        let c = parse_compound("{type:\"mod\",left:17,right:5}");
        assert_eq!(evaluate_compound_int(&c, &ctx).unwrap(), 2);

        let c = parse_compound("{type:\"pow\",base:2,exponent:3}");
        assert_eq!(evaluate_compound_int(&c, &ctx).unwrap(), 8);

        let c = parse_compound("{type:\"pow\",base:0,exponent:0}");
        assert!(evaluate_compound_int(&c, &ctx).is_err());

        let c = parse_compound("{type:\"abs\",input:-15}");
        assert_eq!(evaluate_compound_int(&c, &ctx).unwrap(), 15);

        let c = parse_compound("{type:\"negate\",input:42}");
        assert_eq!(evaluate_compound_int(&c, &ctx).unwrap(), -42);

        let c = parse_compound("{type:\"min\",inputs:[10,5,20]}");
        assert_eq!(evaluate_compound_int(&c, &ctx).unwrap(), 5);

        let c = parse_compound("{type:\"max\",inputs:[10,5,20]}");
        assert_eq!(evaluate_compound_int(&c, &ctx).unwrap(), 20);
    }

    #[test]
    fn evaluate_float_works() {
        let source = CommandSource::dummy();
        let ctx = ComputeContext {
            source: &source,
            target_type: TargetType::Default,
            block_pos: None,
            target_entity: None,
            datapack_manager: None,
            depth: Cell::new(0),
        };

        let c = parse_compound("{type:\"constant\",value:3.5}");
        assert!((evaluate_compound_float(&c, &ctx).unwrap() - 3.5).abs() < 1e-6);

        let c = parse_compound("{type:\"add\",left:1.5,right:2.0}");
        assert!((evaluate_compound_float(&c, &ctx).unwrap() - 3.5).abs() < 1e-6);

        let c = parse_compound("{type:\"sub\",left:5.5,right:2.0}");
        assert!((evaluate_compound_float(&c, &ctx).unwrap() - 3.5).abs() < 1e-6);

        let c = parse_compound("{type:\"mul\",left:2.0,right:3.5}");
        assert!((evaluate_compound_float(&c, &ctx).unwrap() - 7.0).abs() < 1e-6);

        let c = parse_compound("{type:\"div\",left:7.0,right:2.0}");
        assert!((evaluate_compound_float(&c, &ctx).unwrap() - 3.5).abs() < 1e-6);

        let c = parse_compound("{type:\"div\",left:7.0,right:0.0}");
        assert!(evaluate_compound_float(&c, &ctx).is_err());

        let c = parse_compound("{type:\"pow\",base:0.0,exponent:0.0}");
        assert!(evaluate_compound_float(&c, &ctx).is_err());

        let c = parse_compound("{type:\"sqrt\",input:16.0}");
        assert!((evaluate_compound_float(&c, &ctx).unwrap() - 4.0).abs() < 1e-6);

        let c = parse_compound("{type:\"ceil\",input:3.2}");
        assert_eq!(evaluate_compound_float(&c, &ctx).unwrap(), 4.0);

        let c = parse_compound("{type:\"floor\",input:3.8}");
        assert_eq!(evaluate_compound_float(&c, &ctx).unwrap(), 3.0);

        let c = parse_compound("{type:\"round\",input:3.5}");
        assert_eq!(evaluate_compound_float(&c, &ctx).unwrap(), 4.0);

        let c = parse_compound("{type:\"truncate\",input:3.8}");
        assert_eq!(evaluate_compound_float(&c, &ctx).unwrap(), 3.0);

        let c = parse_compound("{type:\"from_int\",input:42}");
        assert_eq!(evaluate_compound_float(&c, &ctx).unwrap(), 42.0);
    }

    #[test]
    fn datapack_providers() {
        let dm = DatapackManager::new();
        dm.insert_context_int_provider(
            "my_pack:calc_base".to_string(),
            NbtTag::Compound(parse_compound("{type:\"constant\",value:20}")),
        );
        dm.insert_context_int_provider(
            "my_pack:calc_add".to_string(),
            NbtTag::Compound(parse_compound(
                "{type:\"add\",left:\"my_pack:calc_base\",right:22}",
            )),
        );
        dm.insert_context_float_provider(
            "my_pack:float_calc".to_string(),
            NbtTag::Compound(parse_compound("{type:\"add\",left:2.5,right:3.5}")),
        );

        let source = CommandSource::dummy();
        let ctx = ComputeContext {
            source: &source,
            target_type: TargetType::Default,
            block_pos: None,
            target_entity: None,
            datapack_manager: Some(&dm),
            depth: Cell::new(0),
        };

        let arg_int = ProviderArg::Id(Identifier::parse("my_pack:calc_add").unwrap());
        let (val, name) = evaluate_int(&arg_int, &ctx).unwrap();
        assert_eq!(val, 42);
        assert_eq!(name.as_deref(), Some("my_pack:calc_add"));

        let arg_float = ProviderArg::Id(Identifier::parse("my_pack:float_calc").unwrap());
        let (val, name) = evaluate_float(&arg_float, &ctx).unwrap();
        assert!((val - 6.0).abs() < 1e-6);
        assert_eq!(name.as_deref(), Some("my_pack:float_calc"));

        // Circular reference detection
        dm.insert_context_int_provider(
            "my_pack:loop_a".to_string(),
            NbtTag::Compound(parse_compound(
                "{type:\"add\",left:\"my_pack:loop_b\",right:1}",
            )),
        );
        dm.insert_context_int_provider(
            "my_pack:loop_b".to_string(),
            NbtTag::Compound(parse_compound(
                "{type:\"add\",left:\"my_pack:loop_a\",right:1}",
            )),
        );
        let arg_loop = ProviderArg::Id(Identifier::parse("my_pack:loop_a").unwrap());
        assert!(evaluate_int(&arg_loop, &ctx).is_err());
    }
}

/// A record of what `evaluate_compound_float` answers for a corpus of expressions, made before the
/// function was split into small ones (`tests/golden/compute_float.txt`; rewrite it on purpose with
/// `KVASIR_BLESS=1`). `uniform` is random and `score` needs a scoreboard, so only their
/// deterministic cases are in it.
#[cfg(test)]
mod float_golden {
    use super::*;
    use std::fmt::Write;

    fn expressions() -> Vec<String> {
        let mut all = binary_expressions();
        all.extend(list_and_power_expressions());
        all.extend(unary_and_other_expressions());
        all
    }

    fn binary_expressions() -> Vec<String> {
        let mut all = Vec::new();
        for op in [
            "add",
            "sum",
            "sub",
            "mul",
            "product",
            "div",
            "floor_div",
            "floor_mod",
            "mod",
            "min",
            "max",
        ] {
            for (left, right) in [
                ("7", "2"),
                ("-7", "2"),
                ("7", "-2"),
                ("0", "0"),
                ("5", "0"),
                ("1.5d", "2.5d"),
                ("\"3\"", "4"),
                ("\"abc\"", "1"),
                ("1e308d", "1e308d"),
                ("-0.0d", "0"),
            ] {
                all.push(format!("{{type:\"{op}\",left:{left},right:{right}}}"));
            }
            all.push(format!("{{type:\"{op}\",left:1}}"));
            all.push(format!("{{type:\"{op}\",right:1}}"));
            all.push(format!("{{type:\"minecraft:{op}\",left:9,right:4}}"));
            all.push(format!("{{type:\"{op}\",left:{{type:\"constant\",value:6}},right:{{type:\"mul\",left:2,right:3}}}}"));
        }
        all
    }

    fn list_and_power_expressions() -> Vec<String> {
        let mut all = Vec::new();
        for op in [
            "add", "sum", "mul", "product", "min", "max", "avg", "average",
        ] {
            for inputs in [
                "[]",
                "[5]",
                "[1,2,3]",
                "[1.5d,\"2\"]",
                "[1,\"abc\"]",
                "[-1,-2,-3]",
                "[0.1d,0.2d,0.3d]",
                "[1e308d,1e308d]",
            ] {
                all.push(format!("{{type:\"{op}\",inputs:{inputs}}}"));
            }
            all.push(format!("{{type:\"{op}\",inputs:5}}"));
        }
        for op in ["pow"] {
            for (base, exponent) in [
                ("2", "3"),
                ("0", "0"),
                ("-8", "0.5d"),
                ("2", "-1"),
                ("0", "-1"),
                ("10", "400"),
            ] {
                all.push(format!("{{type:\"{op}\",base:{base},exponent:{exponent}}}"));
            }
            all.push(format!("{{type:\"{op}\",base:2}}"));
            all.push(format!("{{type:\"{op}\",exponent:2}}"));
        }
        all
    }

    fn unary_and_other_expressions() -> Vec<String> {
        let numbers = [
            "7", "-7", "2", "-2", "0", "1.5d", "2.5d", "\"3\"", "\"abc\"", "1e308d", "-0.0d",
        ];
        let mut all = Vec::new();
        for op in [
            "abs", "negate", "sqrt", "sin", "cos", "ceil", "floor", "round", "truncate",
        ] {
            for input in numbers {
                all.push(format!("{{type:\"{op}\",input:{input}}}"));
            }
            all.push(format!("{{type:\"{op}\"}}"));
        }
        for input in [
            "5",
            "5.7d",
            "-5.7d",
            "\"12\"",
            "\"abc\"",
            "{type:\"constant\",value:3}",
        ] {
            all.push(format!("{{type:\"from_int\",input:{input}}}"));
        }
        all.push("{type:\"from_int\"}".to_owned());
        for (min, max) in [("1", "1"), ("2", "1"), ("1.5d", "1.5d"), ("1", "\"abc\"")] {
            all.push(format!("{{type:\"uniform\",min:{min},max:{max}}}"));
        }
        all.push("{type:\"uniform\",min:1}".to_owned());
        all.push("{type:\"uniform\",max:1}".to_owned());
        for value in [
            "5",
            "2.5d",
            "\"2.5\"",
            "\"abc\"",
            "{type:\"add\",left:1,right:2}",
            "[1,2]",
        ] {
            all.push(format!("{{type:\"constant\",value:{value}}}"));
            all.push(format!("{{value:{value}}}"));
            all.push(format!("{{type:\"unknown_kind\",value:{value}}}"));
        }
        all.push("{type:\"constant\"}".to_owned());
        all.push("{type:\"unknown_kind\"}".to_owned());
        all.push("{}".to_owned());
        all.push("{type:5,value:1}".to_owned());
        all
    }

    #[test]
    fn evaluate_compound_float_answers_as_before() -> Result<(), Box<dyn std::error::Error>> {
        let source = CommandSource::dummy();
        let ctx = ComputeContext {
            source: &source,
            target_type: TargetType::Default,
            block_pos: None,
            target_entity: None,
            datapack_manager: None,
            depth: Cell::new(0),
        };
        let mut now = String::new();
        for text in expressions() {
            let compound = parse_compound_for_golden(&text)?;
            match evaluate_compound_float(&compound, &ctx) {
                Ok(value) => writeln!(now, "{text} => ok {:016x}", value.to_bits())?,
                Err(error) => writeln!(now, "{text} => err {:?}", error.message)?,
            }
        }
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/compute_float.txt");
        if std::env::var_os("KVASIR_BLESS").is_some() {
            std::fs::write(&path, &now)?;
            return Ok(());
        }
        assert_eq!(std::fs::read_to_string(&path)?, now);
        Ok(())
    }

    fn parse_compound_for_golden(text: &str) -> Result<NbtCompound, String> {
        match SnbtParser::parse_for_commands(&mut StringReader::new(text)) {
            Ok(NbtTag::Compound(compound)) => Ok(compound),
            Ok(_) => Err(format!("not a compound: {text}")),
            Err(_) => Err(format!("does not parse: {text}")),
        }
    }
}
