//! Arms of `convert_raw_processor` that are more than a line.

use pumpkin_data::{Block, BlockId, Property};
use pumpkin_nbt::{compound::NbtCompound, tag::NbtTag};

use super::{
    BlockEntityModifier, BlockTagFilter, IgnoredBlock, ProcessorRule, RawBlockEntityModifier,
    RawBlockStateOrName, RawRottableBlocks, RawRule, RottableBlocks, RuleTest, StructureProcessor,
    convert_raw_pos_rule_test, convert_raw_rule_test, json_to_nbt, resolve_output_state,
};

/// A block rot processor; the rottable blocks given as one name or tag, or as a list of names
/// (unknown names dropped).
pub(super) fn block_rot(
    integrity: f32,
    rottable_blocks: Option<RawRottableBlocks>,
) -> StructureProcessor {
    let rottable = rottable_blocks.and_then(|r| match r {
        RawRottableBlocks::Single(s) => {
            if s.starts_with('#') {
                Some(RottableBlocks::Tag(BlockTagFilter::from_tag_str(&s)))
            } else {
                let name = s.strip_prefix("minecraft:").unwrap_or(&s);
                Block::from_name(name)
                    .or_else(|| Block::from_registry_key(name))
                    .map(|b| RottableBlocks::Block(b.id))
                    .or_else(|| Some(RottableBlocks::Tag(BlockTagFilter::from_tag_str(&s))))
            }
        }
        RawRottableBlocks::List(list) => {
            let ids: Vec<BlockId> = list
                .into_iter()
                .filter_map(|s| {
                    let name = s.strip_prefix("minecraft:").unwrap_or(&s);
                    Block::from_name(name)
                        .or_else(|| Block::from_registry_key(name))
                        .map(|b| b.id)
                })
                .collect();
            Some(RottableBlocks::List(ids))
        }
    });
    StructureProcessor::BlockRot {
        integrity,
        rottable_blocks: rottable,
    }
}

/// A block ignore processor over the listed blocks (unknown names dropped), each with its
/// properties when given.
pub(super) fn block_ignore(blocks: Vec<RawBlockStateOrName>) -> StructureProcessor {
    let ignored: Vec<IgnoredBlock> = blocks
        .into_iter()
        .filter_map(|b| match b {
            RawBlockStateOrName::State(raw_state) => {
                let name = raw_state
                    .name
                    .strip_prefix("minecraft:")
                    .unwrap_or(&raw_state.name);
                let block = Block::from_name(name).or_else(|| Block::from_registry_key(name))?;
                Some(IgnoredBlock {
                    block_id: block.id,
                    properties: if raw_state.properties.is_empty() {
                        None
                    } else {
                        Some(
                            raw_state
                                .properties
                                .into_iter()
                                .filter_map(|(k, v)| Property::from_str_pair(&k, &v))
                                .collect(),
                        )
                    },
                })
            }
            RawBlockStateOrName::Name(name_str) => {
                let name = name_str.strip_prefix("minecraft:").unwrap_or(&name_str);
                let block = Block::from_name(name).or_else(|| Block::from_registry_key(name))?;
                Some(IgnoredBlock {
                    block_id: block.id,
                    properties: None,
                })
            }
        })
        .collect();
    StructureProcessor::BlockIgnore(ignored)
}

/// A rule processor; rules whose output state or input test cannot be read are dropped.
pub(super) fn rules(rules: Vec<RawRule>) -> StructureProcessor {
    let converted_rules: Vec<ProcessorRule> = rules
        .into_iter()
        .filter_map(|rule| {
            let output_state = resolve_output_state(&rule.output_state)?;
            let input_predicate = convert_raw_rule_test(rule.input_predicate)?;
            let location_predicate = rule
                .location_predicate
                .and_then(convert_raw_rule_test)
                .unwrap_or(RuleTest::AlwaysTrue);
            let position_predicate = convert_raw_pos_rule_test(rule.position_predicate);
            let block_entity_modifier = rule.block_entity_modifier.map(|m| match m {
                RawBlockEntityModifier::AppendLoot { loot_table } => {
                    BlockEntityModifier::AppendLoot { loot_table }
                }
                RawBlockEntityModifier::AppendStatic { data } => {
                    if let NbtTag::Compound(compound) = json_to_nbt(data) {
                        BlockEntityModifier::AppendStatic { data: compound }
                    } else {
                        BlockEntityModifier::AppendStatic {
                            data: NbtCompound::new(),
                        }
                    }
                }
                RawBlockEntityModifier::Clear => BlockEntityModifier::Clear,
                RawBlockEntityModifier::Passthrough => BlockEntityModifier::Passthrough,
            });

            Some(ProcessorRule {
                position_predicate,
                input_predicate,
                location_predicate,
                output_state,
                block_entity_modifier,
            })
        })
        .collect();
    StructureProcessor::Rule(converted_rules)
}
