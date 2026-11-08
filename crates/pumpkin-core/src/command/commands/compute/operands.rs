//! Operand helpers shared by the float and the integer number providers.

use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;

use super::ERROR_EVALUATION_FAILED;
use crate::command::errors::command_syntax_error::CommandSyntaxError;

pub(super) fn failed() -> CommandSyntaxError {
    ERROR_EVALUATION_FAILED.create_without_context()
}

/// The tag under `key`, or a failed evaluation when it is missing.
pub(super) fn operand<'a>(
    compound: &'a NbtCompound,
    key: &str,
) -> Result<&'a NbtTag, CommandSyntaxError> {
    compound.get(key).ok_or_else(failed)
}

/// The `inputs` list, when the compound has one.
pub(super) fn inputs(compound: &NbtCompound) -> Option<&Vec<NbtTag>> {
    match compound.get("inputs") {
        Some(NbtTag::List(inputs)) => Some(inputs),
        _ => None,
    }
}
