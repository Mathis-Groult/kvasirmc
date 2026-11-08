//! Evaluation of integer number providers written as compounds (`{type:"add",left:1,right:2}`).
//!
//! The same shape as `float_ops`: `evaluate_compound_int` picks the operation by its `type`, and
//! each operation is one small function over the operand helpers of `operands`. Arithmetic wraps
//! where it did before.

use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;
use rand::RngExt;

use super::operands::{failed, inputs, operand};
use super::{
    ComputeContext, ERROR_DIV_BY_ZERO, ERROR_POW_ZERO_ZERO, evaluate_score_provider_int,
    evaluate_tag_int,
};
use crate::command::errors::command_syntax_error::CommandSyntaxError;

type IntResult = Result<i64, CommandSyntaxError>;

pub(super) fn evaluate_compound_int(compound: &NbtCompound, ctx: &ComputeContext) -> IntResult {
    let op_type = compound
        .get_string("type")
        .map(|s| s.strip_prefix("minecraft:").unwrap_or(s));

    match op_type {
        Some("add" | "sum") => add(compound, ctx),
        Some("sub") => binary(compound, ctx, i64::wrapping_sub),
        Some("mul" | "product") => multiply(compound, ctx),
        Some("div") => divide(compound, ctx, |left, right| left / right),
        Some("floor_div") => divide(compound, ctx, floor_div),
        Some("floor_mod") => divide(compound, ctx, |left, right| {
            ((left % right) + right) % right
        }),
        Some("mod") => divide(compound, ctx, |left, right| left % right),
        Some("pow") => power(compound, ctx),
        Some("abs") => unary(compound, ctx, i64::abs),
        Some("negate") => unary(compound, ctx, |input| -input),
        Some("min") => extremum(compound, ctx, i64::MAX, i64::min),
        Some("max") => extremum(compound, ctx, i64::MIN, i64::max),
        Some("avg" | "average") => average(compound, ctx),
        Some("uniform") => uniform(compound, ctx),
        Some("score") => evaluate_score_provider_int(compound, ctx),
        // `constant` and every unknown type read the `value` key.
        _ => value(compound, ctx),
    }
}

fn floor_div(left: i64, right: i64) -> i64 {
    #[allow(clippy::cast_possible_truncation)]
    {
        ((left as f64) / (right as f64)).floor() as i64
    }
}

/// The integer the operand under `key` evaluates to.
fn evaluate_operand(compound: &NbtCompound, key: &str, ctx: &ComputeContext) -> IntResult {
    evaluate_tag_int(operand(compound, key)?, ctx)
}

/// The `left` operand, then the `right` operand (in that order).
fn operands(
    compound: &NbtCompound,
    ctx: &ComputeContext,
) -> Result<(i64, i64), CommandSyntaxError> {
    let left = evaluate_operand(compound, "left", ctx)?;
    let right = evaluate_operand(compound, "right", ctx)?;
    Ok((left, right))
}

fn evaluate_all(inputs: &[NbtTag], ctx: &ComputeContext) -> Result<Vec<i64>, CommandSyntaxError> {
    inputs
        .iter()
        .map(|tag| evaluate_tag_int(tag, ctx))
        .collect()
}

fn value(compound: &NbtCompound, ctx: &ComputeContext) -> IntResult {
    evaluate_tag_int(operand(compound, "value")?, ctx)
}

fn binary(compound: &NbtCompound, ctx: &ComputeContext, op: fn(i64, i64) -> i64) -> IntResult {
    let (left, right) = operands(compound, ctx)?;
    Ok(op(left, right))
}

/// A division-like operation: a zero `right` is an error.
fn divide(compound: &NbtCompound, ctx: &ComputeContext, op: fn(i64, i64) -> i64) -> IntResult {
    let (left, right) = operands(compound, ctx)?;
    if right == 0 {
        return Err(ERROR_DIV_BY_ZERO.create_without_context());
    }
    Ok(op(left, right))
}

fn unary(compound: &NbtCompound, ctx: &ComputeContext, op: fn(i64) -> i64) -> IntResult {
    Ok(op(evaluate_operand(compound, "input", ctx)?))
}

fn add(compound: &NbtCompound, ctx: &ComputeContext) -> IntResult {
    match inputs(compound) {
        Some(list) => Ok(evaluate_all(list, ctx)?
            .into_iter()
            .fold(0, i64::wrapping_add)),
        None => binary(compound, ctx, i64::wrapping_add),
    }
}

fn multiply(compound: &NbtCompound, ctx: &ComputeContext) -> IntResult {
    match inputs(compound) {
        Some(list) => Ok(evaluate_all(list, ctx)?
            .into_iter()
            .fold(1, i64::wrapping_mul)),
        None => binary(compound, ctx, i64::wrapping_mul),
    }
}

/// `min` or `max`: over the `inputs` list (which must not be empty), else over `left` and `right`.
fn extremum(
    compound: &NbtCompound,
    ctx: &ComputeContext,
    identity: i64,
    pick: fn(i64, i64) -> i64,
) -> IntResult {
    match inputs(compound) {
        Some(list) if list.is_empty() => Err(failed()),
        Some(list) => Ok(evaluate_all(list, ctx)?.into_iter().fold(identity, pick)),
        None => binary(compound, ctx, pick),
    }
}

fn average(compound: &NbtCompound, ctx: &ComputeContext) -> IntResult {
    let list = inputs(compound)
        .filter(|list| !list.is_empty())
        .ok_or_else(failed)?;
    let sum = evaluate_all(list, ctx)?
        .into_iter()
        .fold(0, i64::wrapping_add);
    #[allow(clippy::cast_possible_wrap)]
    Ok(sum / (list.len() as i64))
}

fn power(compound: &NbtCompound, ctx: &ComputeContext) -> IntResult {
    let base = evaluate_operand(compound, "base", ctx)?;
    let exponent = evaluate_operand(compound, "exponent", ctx)?;
    if base == 0 && exponent == 0 {
        return Err(ERROR_POW_ZERO_ZERO.create_without_context());
    }
    if exponent < 0 {
        return Ok(0);
    }
    #[allow(clippy::cast_sign_loss)]
    Ok(base.wrapping_pow(exponent as u32))
}

fn uniform(compound: &NbtCompound, ctx: &ComputeContext) -> IntResult {
    let min = evaluate_operand(compound, "min", ctx)?;
    let max = evaluate_operand(compound, "max", ctx)?;
    if min > max {
        return Err(failed());
    }
    Ok(rand::rng().random_range(min..=max))
}
