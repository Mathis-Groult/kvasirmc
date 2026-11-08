//! Evaluation of float number providers written as compounds (`{type:"add",left:1,right:2}`).
//!
//! `evaluate_compound_float` picks the operation by its `type`; each operation reads its operands
//! with the same few helpers, so a new operation is one line in the dispatcher and one small
//! function.

use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;
use rand::RngExt;

use super::operands::{failed, inputs, operand};
use super::{
    ComputeContext, ERROR_DIV_BY_ZERO, ERROR_POW_ZERO_ZERO, evaluate_score_provider_float,
    evaluate_tag_float, evaluate_tag_int,
};
use crate::command::errors::command_syntax_error::CommandSyntaxError;

type FloatResult = Result<f64, CommandSyntaxError>;

pub(super) fn evaluate_compound_float(compound: &NbtCompound, ctx: &ComputeContext) -> FloatResult {
    let op_type = compound
        .get_string("type")
        .map(|s| s.strip_prefix("minecraft:").unwrap_or(s));

    match op_type {
        Some("add" | "sum") => add(compound, ctx),
        Some("sub") => binary(compound, ctx, |left, right| left - right),
        Some("mul" | "product") => multiply(compound, ctx),
        Some("div") => divide(compound, ctx, |left, right| left / right),
        Some("floor_div") => divide(compound, ctx, |left, right| (left / right).floor()),
        Some("floor_mod") => divide(compound, ctx, |left, right| {
            ((left % right) + right) % right
        }),
        Some("mod") => divide(compound, ctx, |left, right| left % right),
        Some("pow") => power(compound, ctx),
        Some("abs") => unary(compound, ctx, f64::abs),
        Some("negate") => unary(compound, ctx, |input| -input),
        Some("min") => extremum(compound, ctx, f64::INFINITY, f64::min),
        Some("max") => extremum(compound, ctx, f64::NEG_INFINITY, f64::max),
        Some("avg" | "average") => average(compound, ctx),
        Some("uniform") => uniform(compound, ctx),
        Some("sqrt") => square_root(compound, ctx),
        Some("sin") => unary(compound, ctx, f64::sin),
        Some("cos") => unary(compound, ctx, f64::cos),
        Some("ceil") => unary(compound, ctx, f64::ceil),
        Some("floor") => unary(compound, ctx, f64::floor),
        Some("round") => unary(compound, ctx, f64::round),
        Some("truncate") => unary(compound, ctx, f64::trunc),
        Some("from_int") => from_int(compound, ctx),
        Some("score") => evaluate_score_provider_float(compound, ctx),
        // `constant` and every unknown type read the `value` key.
        _ => value(compound, ctx),
    }
}

/// The float the operand under `key` evaluates to.
fn evaluate_operand(compound: &NbtCompound, key: &str, ctx: &ComputeContext) -> FloatResult {
    evaluate_tag_float(operand(compound, key)?, ctx)
}

/// The `left` operand, then the `right` operand (in that order).
fn operands(
    compound: &NbtCompound,
    ctx: &ComputeContext,
) -> Result<(f64, f64), CommandSyntaxError> {
    let left = evaluate_operand(compound, "left", ctx)?;
    let right = evaluate_operand(compound, "right", ctx)?;
    Ok((left, right))
}

fn evaluate_all(inputs: &[NbtTag], ctx: &ComputeContext) -> Result<Vec<f64>, CommandSyntaxError> {
    inputs
        .iter()
        .map(|tag| evaluate_tag_float(tag, ctx))
        .collect()
}

fn value(compound: &NbtCompound, ctx: &ComputeContext) -> FloatResult {
    evaluate_tag_float(operand(compound, "value")?, ctx)
}

fn binary(compound: &NbtCompound, ctx: &ComputeContext, op: fn(f64, f64) -> f64) -> FloatResult {
    let (left, right) = operands(compound, ctx)?;
    Ok(op(left, right))
}

/// A division-like operation: a zero `right` is an error.
fn divide(compound: &NbtCompound, ctx: &ComputeContext, op: fn(f64, f64) -> f64) -> FloatResult {
    let (left, right) = operands(compound, ctx)?;
    if right == 0.0 {
        return Err(ERROR_DIV_BY_ZERO.create_without_context());
    }
    Ok(op(left, right))
}

fn unary(compound: &NbtCompound, ctx: &ComputeContext, op: fn(f64) -> f64) -> FloatResult {
    Ok(op(evaluate_operand(compound, "input", ctx)?))
}

fn add(compound: &NbtCompound, ctx: &ComputeContext) -> FloatResult {
    match inputs(compound) {
        Some(list) => Ok(evaluate_all(list, ctx)?
            .into_iter()
            .fold(0.0, |sum, x| sum + x)),
        None => binary(compound, ctx, |left, right| left + right),
    }
}

fn multiply(compound: &NbtCompound, ctx: &ComputeContext) -> FloatResult {
    match inputs(compound) {
        Some(list) => Ok(evaluate_all(list, ctx)?
            .into_iter()
            .fold(1.0, |product, x| product * x)),
        None => binary(compound, ctx, |left, right| left * right),
    }
}

/// `min` or `max`: over the `inputs` list (which must not be empty), else over `left` and `right`.
fn extremum(
    compound: &NbtCompound,
    ctx: &ComputeContext,
    identity: f64,
    pick: fn(f64, f64) -> f64,
) -> FloatResult {
    match inputs(compound) {
        Some(list) if list.is_empty() => Err(failed()),
        Some(list) => Ok(evaluate_all(list, ctx)?.into_iter().fold(identity, pick)),
        None => binary(compound, ctx, pick),
    }
}

fn average(compound: &NbtCompound, ctx: &ComputeContext) -> FloatResult {
    let list = inputs(compound)
        .filter(|list| !list.is_empty())
        .ok_or_else(failed)?;
    let sum = evaluate_all(list, ctx)?
        .into_iter()
        .fold(0.0, |sum, x| sum + x);
    #[allow(clippy::cast_precision_loss)]
    Ok(sum / (list.len() as f64))
}

fn power(compound: &NbtCompound, ctx: &ComputeContext) -> FloatResult {
    let base = evaluate_operand(compound, "base", ctx)?;
    let exponent = evaluate_operand(compound, "exponent", ctx)?;
    if base == 0.0 && exponent == 0.0 {
        return Err(ERROR_POW_ZERO_ZERO.create_without_context());
    }
    Ok(base.powf(exponent))
}

fn uniform(compound: &NbtCompound, ctx: &ComputeContext) -> FloatResult {
    let min = evaluate_operand(compound, "min", ctx)?;
    let max = evaluate_operand(compound, "max", ctx)?;
    if min > max {
        return Err(failed());
    }
    Ok(rand::rng().random_range(min..=max))
}

fn square_root(compound: &NbtCompound, ctx: &ComputeContext) -> FloatResult {
    let input = evaluate_operand(compound, "input", ctx)?;
    if input < 0.0 {
        return Err(failed());
    }
    Ok(input.sqrt())
}

fn from_int(compound: &NbtCompound, ctx: &ComputeContext) -> FloatResult {
    let input = operand(compound, "input")?;
    #[allow(clippy::cast_precision_loss)]
    Ok(evaluate_tag_int(input, ctx)? as f64)
}
