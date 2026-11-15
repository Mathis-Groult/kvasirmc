use crate::command::argument_builder::{ArgumentBuilder, argument, command, literal};
use crate::command::argument_types::block_predicate::{BlockPredicate, BlockPredicateArgumentType};
use crate::command::argument_types::coordinates::block_pos::BlockPosArgumentType;
use crate::command::context::command_context::CommandContext;
use crate::command::errors::command_syntax_error::CommandSyntaxError;
use crate::command::errors::error_types::CommandErrorType;
use crate::command::node::dispatcher::CommandDispatcher;
use crate::command::node::{CommandExecutor, CommandExecutorResult};
use pumpkin_data::{BlockStateId, translation};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_util::PermissionLvl;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::permission::{Permission, PermissionDefault, PermissionRegistry};
use pumpkin_util::text::TextComponent;
use pumpkin_world::world::BlockFlags;
use std::sync::Arc;

use crate::world::World;

const DESCRIPTION: &str = "Clones blocks from one region to another.";
const PERMISSION: &str = "minecraft:command.clone";

const ARG_BEGIN: &str = "begin";
const ARG_END: &str = "end";
const ARG_DEST: &str = "destination";
const ARG_FILTER: &str = "filter";

const OVERLAP_ERROR: CommandErrorType<0> = CommandErrorType::new(
    translation::java::COMMANDS_CLONE_OVERLAP,
    translation::bedrock::COMMANDS_CLONE_NOOVERLAP,
);

const TOOBIG_ERROR: CommandErrorType<2> = CommandErrorType::new(
    translation::java::COMMANDS_CLONE_TOOBIG,
    translation::bedrock::COMMANDS_CLONE_TOOMANYBLOCKS,
);

const FAILED_ERROR: CommandErrorType<0> = CommandErrorType::new(
    translation::java::COMMANDS_CLONE_FAILED,
    translation::bedrock::COMMANDS_CLONE_FAILED,
);

const NOT_LOADED_ERROR: CommandErrorType<0> = CommandErrorType::new(
    translation::java::ARGUMENT_POS_UNLOADED,
    translation::java::ARGUMENT_POS_UNLOADED,
);

const OUT_OF_WORLD_ERROR: CommandErrorType<0> = CommandErrorType::new(
    translation::java::ARGUMENT_POS_OUTOFWORLD,
    translation::java::ARGUMENT_POS_OUTOFWORLD,
);

#[derive(Clone, Copy, PartialEq, Eq)]
enum MaskMode {
    Replace,
    Masked,
    Filtered,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CloneMode {
    Normal,
    Force,
    Move,
}

/// Flags for every block `/clone` writes or clears. Vanilla (`CloneCommands.clone`) first turns
/// each destination position, and for `move` each source position, into a barrier with flags 816
/// (skip on-place, skip block-entity side effects, suppress drops, known shape): a replaced
/// container loses its contents without dropping them. With `NOTIFY_ALL` alone a moved chest
/// dropped its items at the source while the copy kept them (item duplication).
const NO_DROPS: BlockFlags =
    BlockFlags::NOTIFY_ALL.union(BlockFlags::SKIP_BLOCK_ENTITY_REPLACED_CALLBACK);

struct CloneExecutor {
    mask_mode: MaskMode,
    clone_mode: CloneMode,
    has_filter: bool,
}

struct ClonedBlock {
    src_pos: BlockPos,
    dest_pos: BlockPos,
    state_id: BlockStateId,
    block_entity_nbt: Option<NbtCompound>,
}

/// A box of blocks given by its lowest and its highest corner.
#[derive(Clone, Copy)]
struct Region {
    min: BlockPos,
    max: BlockPos,
}

impl Region {
    const fn contains(&self, pos: &BlockPos) -> bool {
        pos.0.x >= self.min.0.x
            && pos.0.x <= self.max.0.x
            && pos.0.y >= self.min.0.y
            && pos.0.y <= self.max.0.y
            && pos.0.z >= self.min.0.z
            && pos.0.z <= self.max.0.z
    }

    /// Whether `other` shares at least one block with this box.
    const fn overlaps(&self, other: &Self) -> bool {
        !(other.max.0.x < self.min.0.x
            || other.min.0.x > self.max.0.x
            || other.max.0.y < self.min.0.y
            || other.min.0.y > self.max.0.y
            || other.max.0.z < self.min.0.z
            || other.min.0.z > self.max.0.z)
    }

    /// Whether every chunk column the box touches is loaded.
    fn chunks_loaded(&self, world: &Arc<World>) -> bool {
        for cx in (self.min.0.x >> 4)..=(self.max.0.x >> 4) {
            for cz in (self.min.0.z >> 4)..=(self.max.0.z >> 4) {
                let chunk_pos = Vector2::new(cx, cz);
                if world.level.read_chunk_sync(&chunk_pos, |_| ()).is_none() {
                    return false;
                }
            }
        }
        true
    }
}

/// The source box spanned by `begin` and `end`, and the destination box of the same size whose
/// lowest corner is `dest`. A source of more than 32768 blocks is refused.
fn regions(
    begin: BlockPos,
    end: BlockPos,
    dest: BlockPos,
) -> Result<(Region, Region), CommandSyntaxError> {
    let min_x = begin.0.x.min(end.0.x);
    let max_x = begin.0.x.max(end.0.x);
    let min_y = begin.0.y.min(end.0.y);
    let max_y = begin.0.y.max(end.0.y);
    let min_z = begin.0.z.min(end.0.z);
    let max_z = begin.0.z.max(end.0.z);

    let size_x = max_x - min_x + 1;
    let size_y = max_y - min_y + 1;
    let size_z = max_z - min_z + 1;

    let volume = size_x * size_y * size_z;

    if volume > 32768 {
        return Err(TOOBIG_ERROR.create_without_context_args_slice(&[
            TextComponent::text("32768"),
            TextComponent::text(volume.to_string()),
        ]));
    }

    let source = Region {
        min: BlockPos::new(min_x, min_y, min_z),
        max: BlockPos::new(max_x, max_y, max_z),
    };
    let target = Region {
        min: dest,
        max: BlockPos::new(
            dest.0.x + size_x - 1,
            dest.0.y + size_y - 1,
            dest.0.z + size_z - 1,
        ),
    };
    Ok((source, target))
}

/// Both boxes must lie inside the build limit and in loaded chunks.
fn check_in_world(
    world: &Arc<World>,
    source: &Region,
    target: &Region,
) -> Result<(), CommandSyntaxError> {
    if !world.is_in_build_limit(source.min)
        || !world.is_in_build_limit(source.max)
        || !world.is_in_build_limit(target.min)
        || !world.is_in_build_limit(target.max)
    {
        return Err(OUT_OF_WORLD_ERROR.create_without_context());
    }

    if !source.chunks_loaded(world) || !target.chunks_loaded(world) {
        return Err(NOT_LOADED_ERROR.create_without_context());
    }
    Ok(())
}

/// Writes every cloned block (and its block entity) at its destination; returns how many.
fn place_blocks(world: &Arc<World>, blocks_to_clone: &[ClonedBlock]) -> i32 {
    let mut count = 0;
    for block in blocks_to_clone {
        world.set_block_state(&block.dest_pos, block.state_id, NO_DROPS);

        if let Some(nbt) = &block.block_entity_nbt {
            let mut new_nbt = nbt.clone();
            new_nbt.put_int("x", block.dest_pos.0.x);
            new_nbt.put_int("y", block.dest_pos.0.y);
            new_nbt.put_int("z", block.dest_pos.0.z);

            if let Some(be) = crate::block::entities::block_entity_from_nbt(&new_nbt) {
                world.add_block_entity(be);
            }
        }

        count += 1;
    }
    count
}

/// `move`: clears every source block that the destination did not overwrite.
fn clear_moved_sources(world: &Arc<World>, blocks_to_clone: &[ClonedBlock], target: &Region) {
    for block in blocks_to_clone {
        if !target.contains(&block.src_pos) {
            world.set_block_state(&block.src_pos, BlockStateId::AIR, NO_DROPS);
        }
    }
}

impl CloneExecutor {
    /// Whether the mask mode copies a source block in this state.
    fn should_clone(
        &self,
        world: &Arc<World>,
        state_id: BlockStateId,
        src_pos: &BlockPos,
        filter_block: Option<&BlockPredicate>,
    ) -> bool {
        match self.mask_mode {
            MaskMode::Replace => true,
            MaskMode::Masked => !pumpkin_data::block_properties::is_air(state_id),
            MaskMode::Filtered => filter_block
                .is_some_and(|f| f.test(state_id, || world.get_block_entity_nbt(src_pos))),
        }
    }

    /// Reads every source block the mask mode copies, with its block entity, in x, y, z order.
    fn collect_blocks(
        &self,
        world: &Arc<World>,
        source: &Region,
        dest: BlockPos,
        filter_block: Option<&BlockPredicate>,
    ) -> Vec<ClonedBlock> {
        let mut blocks_to_clone = Vec::new();

        for x in 0..=(source.max.0.x - source.min.0.x) {
            for y in 0..=(source.max.0.y - source.min.0.y) {
                for z in 0..=(source.max.0.z - source.min.0.z) {
                    let src_pos =
                        BlockPos::new(source.min.0.x + x, source.min.0.y + y, source.min.0.z + z);
                    let dest_pos = BlockPos::new(dest.0.x + x, dest.0.y + y, dest.0.z + z);

                    let state_id = world.get_block_state_id(&src_pos);

                    if self.should_clone(world, state_id, &src_pos, filter_block) {
                        let block_entity_nbt = world.get_block_entity(&src_pos).map(|be| {
                            let mut nbt = NbtCompound::new();
                            be.write_internal(&mut nbt);
                            nbt
                        });

                        blocks_to_clone.push(ClonedBlock {
                            src_pos,
                            dest_pos,
                            state_id,
                            block_entity_nbt,
                        });
                    }
                }
            }
        }
        blocks_to_clone
    }
}

impl CommandExecutor for CloneExecutor {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let begin = BlockPosArgumentType::get_block_pos(context, ARG_BEGIN)?;
        let end = BlockPosArgumentType::get_block_pos(context, ARG_END)?;
        let dest = BlockPosArgumentType::get_block_pos(context, ARG_DEST)?;

        let filter_block = if self.has_filter {
            Some(BlockPredicateArgumentType::get(context, ARG_FILTER)?)
        } else {
            None
        };

        let (source, target) = regions(begin, end, dest)?;

        if source.overlaps(&target) && self.clone_mode != CloneMode::Force {
            return Err(OVERLAP_ERROR.create_without_context());
        }

        let world = context.source.world();
        check_in_world(world, &source, &target)?;

        let blocks_to_clone =
            self.collect_blocks(world, &source, target.min, filter_block.as_ref());
        let count = place_blocks(world, &blocks_to_clone);

        if count == 0 {
            return Err(FAILED_ERROR.create_without_context());
        }

        if self.clone_mode == CloneMode::Move {
            clear_moved_sources(world, &blocks_to_clone, &target);
        }

        world.flush_block_updates();

        context.source.send_feedback(
            TextComponent::translate_cross(
                translation::java::COMMANDS_CLONE_SUCCESS,
                translation::bedrock::COMMANDS_CLONE_SUCCESS,
                [TextComponent::text(count.to_string())],
            ),
            true,
        );

        Ok(count)
    }
}

#[cfg(test)]
mod drop_tests;
#[cfg(test)]
mod golden;

pub fn register(dispatcher: &mut CommandDispatcher, registry: &PermissionRegistry) {
    registry.register_permission_or_panic(Permission::new(
        PERMISSION,
        DESCRIPTION,
        PermissionDefault::Op(PermissionLvl::Two),
    ));

    dispatcher.register(
        command("clone", DESCRIPTION).requires(PERMISSION).then(
            argument(ARG_BEGIN, BlockPosArgumentType).then(
                argument(ARG_END, BlockPosArgumentType).then(
                    argument(ARG_DEST, BlockPosArgumentType)
                        .executes(CloneExecutor {
                            mask_mode: MaskMode::Replace,
                            clone_mode: CloneMode::Normal,
                            has_filter: false,
                        })
                        .then(
                            literal("replace")
                                .executes(CloneExecutor {
                                    mask_mode: MaskMode::Replace,
                                    clone_mode: CloneMode::Normal,
                                    has_filter: false,
                                })
                                .then(literal("force").executes(CloneExecutor {
                                    mask_mode: MaskMode::Replace,
                                    clone_mode: CloneMode::Force,
                                    has_filter: false,
                                }))
                                .then(literal("move").executes(CloneExecutor {
                                    mask_mode: MaskMode::Replace,
                                    clone_mode: CloneMode::Move,
                                    has_filter: false,
                                }))
                                .then(literal("normal").executes(CloneExecutor {
                                    mask_mode: MaskMode::Replace,
                                    clone_mode: CloneMode::Normal,
                                    has_filter: false,
                                })),
                        )
                        .then(
                            literal("masked")
                                .executes(CloneExecutor {
                                    mask_mode: MaskMode::Masked,
                                    clone_mode: CloneMode::Normal,
                                    has_filter: false,
                                })
                                .then(literal("force").executes(CloneExecutor {
                                    mask_mode: MaskMode::Masked,
                                    clone_mode: CloneMode::Force,
                                    has_filter: false,
                                }))
                                .then(literal("move").executes(CloneExecutor {
                                    mask_mode: MaskMode::Masked,
                                    clone_mode: CloneMode::Move,
                                    has_filter: false,
                                }))
                                .then(literal("normal").executes(CloneExecutor {
                                    mask_mode: MaskMode::Masked,
                                    clone_mode: CloneMode::Normal,
                                    has_filter: false,
                                })),
                        )
                        .then(
                            literal("filtered").then(
                                argument(ARG_FILTER, BlockPredicateArgumentType)
                                    .executes(CloneExecutor {
                                        mask_mode: MaskMode::Filtered,
                                        clone_mode: CloneMode::Normal,
                                        has_filter: true,
                                    })
                                    .then(literal("force").executes(CloneExecutor {
                                        mask_mode: MaskMode::Filtered,
                                        clone_mode: CloneMode::Force,
                                        has_filter: true,
                                    }))
                                    .then(literal("move").executes(CloneExecutor {
                                        mask_mode: MaskMode::Filtered,
                                        clone_mode: CloneMode::Move,
                                        has_filter: true,
                                    }))
                                    .then(literal("normal").executes(CloneExecutor {
                                        mask_mode: MaskMode::Filtered,
                                        clone_mode: CloneMode::Normal,
                                        has_filter: true,
                                    })),
                            ),
                        ),
                ),
            ),
        ),
    );
}
