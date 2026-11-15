//! Arms of [`StructureProcessor::process_with_context`] that are more than a line.

use pumpkin_data::{Block, BlockId, BlockState};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_util::{math::vector3::Vector3, random::RandomImpl};

use super::{BlockPlacer, BlockStateResolver, PaletteEntry, ProcessorContext, ProcessorRule};

/// The output of the first rule whose position, input and location tests pass (its block entity
/// modifier applied to `nbt`), else `state` unchanged.
pub(super) fn apply_rules(
    rules: &[ProcessorRule],
    placer: &impl BlockPlacer,
    world_pos: Vector3<i32>,
    state: &'static BlockState,
    nbt: &mut Option<NbtCompound>,
    context: &ProcessorContext,
    rng: &mut impl RandomImpl,
) -> &'static BlockState {
    let world_state_id = placer.get_block_state(&world_pos);
    let world_state = BlockState::from_id(world_state_id);
    for rule in rules {
        if rule
            .position_predicate
            .test(world_pos, context.structure_start, rng)
            && rule.input_predicate.test(state, rng)
            && rule.location_predicate.test(world_state, rng)
        {
            if let Some(modifier) = &rule.block_entity_modifier {
                modifier.apply(nbt, world_pos, rng);
            }
            return rule.output_state;
        }
    }
    state
}

/// A jigsaw block becomes its `final_state` (air when missing or unknown) and loses its NBT;
/// any other block stays.
pub(super) fn replace_jigsaw(
    state: &'static BlockState,
    nbt: &mut Option<NbtCompound>,
) -> &'static BlockState {
    if state.id.to_block_id() == BlockId::JIGSAW {
        let final_state_str = nbt
            .as_ref()
            .and_then(|n| n.get_string("final_state"))
            .map(str::to_string);
        *nbt = None;
        let final_str = final_state_str.as_deref().unwrap_or("minecraft:air");
        let palette_entry = PaletteEntry::from_string(final_str);
        BlockStateResolver::resolve_simple(&palette_entry).unwrap_or(Block::AIR.default_state)
    } else {
        state
    }
}
