use pumpkin_data::translation;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_util::PermissionLvl;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::permission::{Permission, PermissionDefault, PermissionRegistry};
use pumpkin_util::text::TextComponent;
use pumpkin_world::world::BlockFlags;

use crate::command::argument_builder::{
    ArgumentBuilder, RequiredArgumentBuilder, argument, command, literal,
};
use crate::command::argument_types::block::BlockStateArgumentType;
use crate::command::argument_types::block_predicate::{BlockPredicate, BlockPredicateArgumentType};
use crate::command::argument_types::coordinates::block_pos::BlockPosArgumentType;
use crate::command::context::command_context::CommandContext;
use crate::command::context::command_source::CommandSource;
use crate::command::errors::command_syntax_error::CommandSyntaxError;
use crate::command::errors::error_types::CommandErrorType;
use crate::command::node::dispatcher::CommandDispatcher;
use crate::command::node::{CommandExecutor, CommandExecutorResult};

const DESCRIPTION: &str = "Fills all or parts of a region with a specific block.";
const PERMISSION: &str = "minecraft:command.fill";

const ERROR_AREA_TOO_LARGE: CommandErrorType<2> = CommandErrorType::new(
    translation::java::COMMANDS_FILL_TOOBIG,
    translation::java::COMMANDS_FILL_TOOBIG,
);

const ERROR_FAILED: CommandErrorType<0> = CommandErrorType::new(
    translation::java::COMMANDS_FILL_FAILED,
    translation::java::COMMANDS_FILL_FAILED,
);

#[derive(Clone, Copy, PartialEq, Eq)]
enum FillMode {
    Replace,
    Outline,
    Hollow,
    Destroy,
    Keep,
}

#[derive(Clone, Copy)]
enum FilterMode {
    None,
    WithFilter,
    KeepAir,
}

/// The positions where the new block lets light through, or gives it off, differently from the
/// old one. `fill` writes blocks straight into the chunks, so the light engine, which
/// `World::set_block_state` tells about each change, has to be told about these separately.
fn positions_needing_light_update(
    changed: &[(BlockPos, BlockStateId, BlockStateId)],
) -> Vec<BlockPos> {
    changed
        .iter()
        .filter(|(_, old, new)| {
            pumpkin_world::lighting::LightEngine::has_different_light_properties(
                old.to_state(),
                new.to_state(),
            )
        })
        .map(|(position, _, _)| *position)
        .collect()
}

/// The blocks whose state changed, each with the state it now has: they tell their neighbours, as
/// vanilla's `blockUpdated` does after a fill. `fill` writes blocks straight into the chunks, so
/// without this a lamp next to a redstone block placed by `fill` is never told about it.
fn positions_needing_neighbour_update(
    changed: &[(BlockPos, BlockStateId, BlockStateId)],
) -> Vec<(BlockPos, BlockStateId)> {
    changed
        .iter()
        .filter(|(_, old, new)| old != new)
        .map(|(position, _, new)| (*position, *new))
        .collect()
}

#[expect(clippy::too_many_lines)]
fn fill_blocks(
    source: &CommandSource,
    from: BlockPos,
    to: BlockPos,
    target_state_id: BlockStateId,
    mode: FillMode,
    filter: Option<&BlockPredicate>,
    strict: bool,
) -> Result<i32, CommandSyntaxError> {
    let min_x = from.0.x.min(to.0.x);
    let min_y = from.0.y.min(to.0.y);
    let min_z = from.0.z.min(to.0.z);
    let max_x = from.0.x.max(to.0.x);
    let max_y = from.0.y.max(to.0.y);
    let max_z = from.0.z.max(to.0.z);

    let x_span = i64::from(max_x - min_x + 1);
    let y_span = i64::from(max_y - min_y + 1);
    let z_span = i64::from(max_z - min_z + 1);
    let area = x_span * y_span * z_span;

    let world = source.world().clone();
    let max_block_modifications = {
        let level_info = world.level_info.load();
        level_info.game_rules.max_block_modifications
    };

    if area > max_block_modifications {
        return Err(ERROR_AREA_TOO_LARGE.create_without_context(
            TextComponent::text(max_block_modifications.to_string()),
            TextComponent::text(area.to_string()),
        ));
    }

    let mut changed_positions = Vec::new();

    let min_chunk_x = min_x >> 4;
    let max_chunk_x = max_x >> 4;
    let min_chunk_z = min_z >> 4;
    let max_chunk_z = max_z >> 4;

    for chunk_x in min_chunk_x..=max_chunk_x {
        for chunk_z in min_chunk_z..=max_chunk_z {
            let chunk_pos = Vector2::new(chunk_x, chunk_z);
            let local_start_x = (min_x - (chunk_x << 4)).clamp(0, 15) as usize;
            let local_end_x = (max_x - (chunk_x << 4)).clamp(0, 15) as usize;
            let local_start_z = (min_z - (chunk_z << 4)).clamp(0, 15) as usize;
            let local_end_z = (max_z - (chunk_z << 4)).clamp(0, 15) as usize;

            let result = world.level.read_chunk_sync(&chunk_pos, |chunk| {
                let mut updates_for_chunk = Vec::new();
                let mut block_entities_to_remove = Vec::new();
                let min_y_chunk = chunk.section.min_y;

                {
                    let block_sections = chunk
                        .section
                        .block_sections
                        .read()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);

                    for y in min_y..=max_y {
                        let y_rel = y - min_y_chunk;
                        if y_rel < 0 {
                            continue;
                        }
                        let rel_y = y_rel as usize;
                        let section_index = rel_y / pumpkin_world::chunk::CHUNK_WIDTH;
                        let sub_y = rel_y % pumpkin_world::chunk::CHUNK_WIDTH;

                        let Some(section) = block_sections.get(section_index) else {
                            continue;
                        };

                        for rel_z in local_start_z..=local_end_z {
                            for rel_x in local_start_x..=local_end_x {
                                let global_x = (chunk_x << 4) + rel_x as i32;
                                let global_z = (chunk_z << 4) + rel_z as i32;
                                let pos = BlockPos(Vector3::new(global_x, y, global_z));

                                let current_state_id = section.get(rel_x, sub_y, rel_z);
                                let current_block = Block::from_state_id(current_state_id);

                                match mode {
                                    FillMode::Keep => {
                                        if !current_block.is_air() {
                                            continue;
                                        }
                                    }
                                    _ => {
                                        if let Some(f) = filter
                                            && !f.test(current_state_id, || {
                                                world.get_block_entity_nbt_in_chunk(chunk, &pos)
                                            })
                                        {
                                            continue;
                                        }
                                    }
                                }

                                let is_edge = global_x == min_x
                                    || global_x == max_x
                                    || y == min_y
                                    || y == max_y
                                    || global_z == min_z
                                    || global_z == max_z;

                                let block_to_place = match mode {
                                    FillMode::Outline => is_edge.then_some(target_state_id),
                                    FillMode::Hollow => {
                                        if is_edge {
                                            Some(target_state_id)
                                        } else {
                                            Some(BlockStateId::AIR)
                                        }
                                    }
                                    FillMode::Destroy => {
                                        if current_state_id != target_state_id {
                                            world.break_block(
                                                &pos,
                                                None,
                                                BlockFlags::SKIP_DROPS
                                                    | BlockFlags::NOTIFY_ALL
                                                    | BlockFlags::FORCE_STATE,
                                            );
                                        }
                                        Some(target_state_id)
                                    }
                                    FillMode::Replace | FillMode::Keep => Some(target_state_id),
                                };

                                if let Some(state_id) = block_to_place {
                                    if current_block.default_state.block_entity_type != u16::MAX {
                                        block_entities_to_remove.push(pos);
                                    }
                                    updates_for_chunk.push((rel_x, y, rel_z, state_id, pos));
                                }
                            }
                        }
                    }
                }

                if updates_for_chunk.is_empty() {
                    return (Vec::new(), block_entities_to_remove);
                }

                let batch_inputs = updates_for_chunk
                    .iter()
                    .map(|&(rx, y, rz, sid, _)| (rx, y, rz, sid));
                // One result per input, since every queued position is inside an existing section.
                let replaced = chunk.set_blocks_batch(batch_inputs);

                let chunk_changed = updates_for_chunk
                    .into_iter()
                    .zip(replaced)
                    .map(|((_, _, _, new_id, pos), (_, _, _, old_id))| (pos, old_id, new_id))
                    .collect::<Vec<_>>();

                (chunk_changed, block_entities_to_remove)
            });

            if let Some((chunk_changed, be_to_remove)) = result {
                for pos in be_to_remove {
                    world.remove_block_entity(&pos);
                }
                changed_positions.extend(chunk_changed);
            }
        }
    }

    let count = changed_positions.len() as i32;
    if count == 0 {
        return Err(ERROR_FAILED.create_without_context());
    }

    // Queued before the callbacks so blocks they change, such as a golem clearing its
    // pattern, reach clients in their final state.
    let client_updates = changed_positions
        .iter()
        .map(|&(pos, _, new_id)| (pos, new_id))
        .collect::<Vec<_>>();
    world.queue_block_updates(&client_updates);
    for position in positions_needing_light_update(&changed_positions) {
        world
            .level
            .light_engine
            .update_lighting_at(&world.level, position);
    }

    let to_tell = positions_needing_neighbour_update(&changed_positions);

    // Vanilla runs onPlace for every placed block unless strict sets UPDATE_SKIP_ON_PLACE.
    if !strict {
        for (pos, old_id, new_id) in changed_positions {
            let new_block = Block::from_state_id(new_id);
            if Block::from_state_id(old_id) != new_block && world.get_block(&pos) == new_block {
                world.on_block_placed(&pos, new_block, new_id, old_id, false);
            }
        }
    }
    // Then every changed block tells its neighbours (vanilla `blockUpdated`), strict or not.
    for (position, state_id) in to_tell {
        world.update_neighbors_at(&position, Block::from_state_id(state_id), None);
    }
    world.flush_block_updates();

    source.send_feedback(
        TextComponent::translate_cross(
            translation::java::COMMANDS_FILL_SUCCESS,
            translation::java::COMMANDS_FILL_SUCCESS,
            [TextComponent::text(count.to_string())],
        ),
        true,
    );

    Ok(count)
}

struct FillExecutor {
    mode: FillMode,
    filter_mode: FilterMode,
    strict: bool,
}

impl CommandExecutor for FillExecutor {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let from = BlockPosArgumentType::get_loaded_block_pos(context, "from")?;
        let to = BlockPosArgumentType::get_loaded_block_pos(context, "to")?;
        let block = BlockStateArgumentType::get(context, "block")?.state_id;

        let filter = if matches!(self.filter_mode, FilterMode::WithFilter) {
            Some(BlockPredicateArgumentType::get(context, "filter")?)
        } else {
            None
        };

        fill_blocks(
            &context.source,
            from,
            to,
            block,
            self.mode,
            filter.as_ref(),
            self.strict,
        )
    }
}

fn wrap_with_mode(builder: RequiredArgumentBuilder, has_filter: bool) -> RequiredArgumentBuilder {
    let filter_mode = if has_filter {
        FilterMode::WithFilter
    } else {
        FilterMode::None
    };

    builder
        .executes(FillExecutor {
            mode: FillMode::Replace,
            filter_mode,
            strict: false,
        })
        .then(literal("outline").executes(FillExecutor {
            mode: FillMode::Outline,
            filter_mode,
            strict: false,
        }))
        .then(literal("hollow").executes(FillExecutor {
            mode: FillMode::Hollow,
            filter_mode,
            strict: false,
        }))
        .then(literal("destroy").executes(FillExecutor {
            mode: FillMode::Destroy,
            filter_mode,
            strict: false,
        }))
        .then(literal("strict").executes(FillExecutor {
            mode: FillMode::Replace,
            filter_mode,
            strict: true,
        }))
}

pub fn register(dispatcher: &mut CommandDispatcher, registry: &PermissionRegistry) {
    registry.register_permission_or_panic(Permission::new(
        PERMISSION,
        DESCRIPTION,
        PermissionDefault::Op(PermissionLvl::Two),
    ));

    let filter_arg = wrap_with_mode(argument("filter", BlockPredicateArgumentType), true);

    let replace_literal = literal("replace")
        .executes(FillExecutor {
            mode: FillMode::Replace,
            filter_mode: FilterMode::None,
            strict: false,
        })
        .then(filter_arg);

    let keep_literal = literal("keep").executes(FillExecutor {
        mode: FillMode::Keep,
        filter_mode: FilterMode::KeepAir,
        strict: false,
    });

    let block_arg = wrap_with_mode(argument("block", BlockStateArgumentType), false)
        .then(replace_literal)
        .then(keep_literal);

    dispatcher.register(
        command("fill", DESCRIPTION).requires(PERMISSION).then(
            argument("from", BlockPosArgumentType)
                .then(argument("to", BlockPosArgumentType).then(block_arg)),
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(x: i32, old: &Block, new: &Block) -> (BlockPos, BlockStateId, BlockStateId) {
        (
            BlockPos::new(x, 64, 0),
            old.default_state.id,
            new.default_state.id,
        )
    }

    #[test]
    fn only_blocks_that_change_light_are_passed_to_the_light_engine() {
        let changed = [
            change(0, &Block::AIR, &Block::STONE),
            change(1, &Block::STONE, &Block::ANDESITE),
            change(2, &Block::AIR, &Block::SEA_LANTERN),
            change(3, &Block::STONE, &Block::STONE),
            change(4, &Block::STONE, &Block::AIR),
        ];
        let positions = positions_needing_light_update(&changed);
        let xs: Vec<i32> = positions.iter().map(|p| p.0.x).collect();
        assert_eq!(
            xs,
            [0, 2, 4],
            "stone to andesite and stone to stone change no light"
        );
    }

    #[test]
    fn every_block_whose_state_changed_tells_its_neighbours_and_no_other_does() {
        let changed = [
            change(0, &Block::AIR, &Block::REDSTONE_BLOCK),
            change(1, &Block::STONE, &Block::STONE),
            change(2, &Block::REDSTONE_BLOCK, &Block::AIR),
            change(3, &Block::STONE, &Block::ANDESITE),
        ];
        let told = positions_needing_neighbour_update(&changed);
        let summary: Vec<(i32, BlockStateId)> = told
            .iter()
            .map(|(position, new)| (position.0.x, *new))
            .collect();
        assert_eq!(
            summary,
            [
                (0, Block::REDSTONE_BLOCK.default_state.id),
                (2, Block::AIR.default_state.id),
                (3, Block::ANDESITE.default_state.id),
            ],
            "in the order they were placed, with the state they now have"
        );
        assert!(positions_needing_neighbour_update(&[]).is_empty());
    }
}
