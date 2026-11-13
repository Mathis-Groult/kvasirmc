//! Reads the block, property and Bedrock tables and gathers the per-block constants, states and property groups.

use super::property_value::EnumPropMeta;
use super::{
    Block, BlockAssets, BlockId, BlockPropertyStruct, BlockShapeOffsetType, BlockState,
    BlockStateId, BoundingBox, GeneratedProperty, GeneratedPropertyType, Property,
    PropertyCollectionData, PropertyStruct, PropertyType, PropertyVariantMapping, SoundType,
    SpawnFloorPredicate, SuffocationPredicate, common_suffix_group_alias,
    const_block_name_from_block_name, parse_geyser_entry, property_group_name_from_derived_name,
};
use crate::bitsets::{Bitset, gen_u16_bitset};
use heck::{ToPascalCase, ToShoutySnakeCase, ToUpperCamelCase};
use proc_macro2::{Span, TokenStream};
use pumpkin_nbt::deserializer::{NbtReadHelper, NbtReadHelperBedrock};
use pumpkin_nbt::tag::NbtTag;
use pumpkin_util::math::{experience::Experience, vector3::Vector3};
use quote::{ToTokens, format_ident, quote};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{Cursor, Read, Seek, SeekFrom},
    panic,
};
use syn::{Ident, LitInt, LitStr};

/// Per-state lists: random-tick, air and liquid states, Bedrock ids and id tables.
#[derive(Default)]
pub(super) struct StateLists {
    pub(super) random_tick_states: Vec<LitInt>,
    pub(super) air_states: Vec<LitInt>,
    pub(super) liquid_states: Vec<LitInt>,
    pub(super) block_state_to_bedrock: Vec<(u16, u32)>,
    pub(super) raw_id_from_state_id_array: Vec<(u16, TokenStream)>,
    pub(super) state_from_state_id_array: Vec<(Ident, usize, u16)>,
}

/// Per-block constants and match arms.
#[derive(Default)]
pub(super) struct BlockLists {
    pub(super) constants_list: Vec<TokenStream>,
    pub(super) block_id_constants: Vec<TokenStream>,
    pub(super) block_from_name_entries: Vec<TokenStream>,
    pub(super) block_from_item_id_arms: Vec<TokenStream>,
    pub(super) spawn_floor_predicate_arms: Vec<TokenStream>,
    pub(super) suffocation_predicate_arms: Vec<TokenStream>,
}

/// What the per-block pass accumulates.
#[derive(Default)]
pub(super) struct BlockTables {
    pub(super) lists: BlockLists,
    pub(super) states: StateLists,
    pub(super) type_from_raw_id_array: Vec<(u16, TokenStream)>,
    pub(super) property_enums: BTreeMap<String, PropertyStruct>,
    pub(super) property_collection_map: BTreeMap<Vec<i32>, PropertyCollectionData>,
    pub(super) existing_item_ids: HashSet<u16>,
}

/// The read-only lookups the per-block pass needs.
pub(super) struct BlockContext<'a> {
    pub(super) generated_prop_map: &'a BTreeMap<i32, &'a GeneratedProperty>,
    pub(super) enum_prop_meta: &'a BTreeMap<i32, EnumPropMeta>,
    pub(super) be_blocks: &'a BTreeMap<String, Vec<(u32, BTreeMap<String, String>)>>,
    pub(super) bedrock_mappings_list: &'a [NbtTag],
    pub(super) air_be_network_id: u32,
}

/// The `shape_offset` match arms of blocks with an offset.
pub(super) fn shape_offset_arms(blocks_assets: &BlockAssets) -> Vec<TokenStream> {
    blocks_assets
        .blocks
        .iter()
        .filter_map(|block| {
            let offset = block.shape_offset.as_ref()?;
            let block = format_ident!("{}", const_block_name_from_block_name(&block.name));
            let offset_type = match offset.offset_type {
                BlockShapeOffsetType::Xz => quote! { ShapeOffsetType::Xz },
                BlockShapeOffsetType::Xyz => quote! { ShapeOffsetType::Xyz },
            };
            let max_horizontal = offset.max_horizontal;
            let max_vertical = offset.max_vertical;

            Some(quote! {
                BlockId::#block => Some(ShapeOffset {
                    offset_type: #offset_type,
                    max_horizontal: #max_horizontal,
                    max_vertical: #max_vertical,
                }),
            })
        })
        .collect::<Vec<_>>()
}

/// The property enum structs of every enum property.
pub(super) fn property_enum_structs(
    generated_properties: &[GeneratedProperty],
) -> BTreeMap<String, PropertyStruct> {
    let mut property_enums: BTreeMap<String, PropertyStruct> = BTreeMap::new();
    for generated_property in generated_properties {
        if matches!(
            generated_property.property_type,
            GeneratedPropertyType::Enum { .. }
        ) {
            let property = generated_property.to_property();
            let renamed_property = property.enum_name.to_upper_camel_case();
            property_enums
                .entry(renamed_property.clone())
                .or_insert_with(|| PropertyStruct {
                    name: renamed_property,
                    values: property.values,
                });
        }
    }
    property_enums
}

/// The property mapping of one block; records its property hashes and enum structs.
fn property_mapping(
    block: &Block,
    ctx: &BlockContext<'_>,
    property_collection: &mut HashSet<i32>,
    property_enums: &mut BTreeMap<String, PropertyStruct>,
) -> Vec<PropertyVariantMapping> {
    let BlockContext {
        generated_prop_map,
        enum_prop_meta,
        ..
    } = ctx;
    let mut property_mapping = Vec::new();

    for property_hash in &block.properties {
        let generated_property = generated_prop_map
            .get(property_hash)
            .expect("Property hash not found in generated_properties");

        property_collection.insert(generated_property.hash_key);

        let property = generated_property.to_property();
        let renamed_property = property.enum_name.to_upper_camel_case();

        let property_type = match &generated_property.property_type {
            GeneratedPropertyType::Boolean => PropertyType::Bool,
            GeneratedPropertyType::Int { min, max } => PropertyType::Int {
                min: *min,
                max: *max,
            },
            GeneratedPropertyType::Enum { values } => {
                let meta = enum_prop_meta
                    .get(&generated_property.hash_key)
                    .expect("Enum metadata missing");
                PropertyType::Enum {
                    name: renamed_property.clone(),
                    values: values.clone(),
                    prop_variant_name: meta.prop_variant_name.clone(),
                    canonical_enum_name: meta.canonical_enum_name.clone(),
                    shared_none_variants: meta.shared_none_variants.clone(),
                }
            }
        };

        if let PropertyType::Enum { name, .. } = &property_type {
            property_enums
                .entry(name.clone())
                .or_insert_with(|| PropertyStruct {
                    name: name.clone(),
                    values: property.values.clone(),
                });
        }

        property_mapping.push(PropertyVariantMapping {
            original_name: property.serialized_name,
            property_type,
        });
    }
    property_mapping
}

/// Records every state of one block: random-tick/air/liquid lists, Bedrock id and id tables.
fn push_states(
    block: &Block,
    const_ident: &Ident,
    ctx: &BlockContext<'_>,
    states: &mut StateLists,
) {
    let BlockContext {
        be_blocks,
        bedrock_mappings_list,
        air_be_network_id,
        ..
    } = ctx;
    let air_be_network_id = *air_be_network_id;
    let StateLists {
        random_tick_states,
        air_states,
        liquid_states,
        block_state_to_bedrock,
        raw_id_from_state_id_array,
        state_from_state_id_array,
    } = states;
    for (i, state) in block.states.iter().enumerate() {
        if state.has_random_ticks() {
            let state_id = LitInt::new(&state.id.0.to_string(), Span::call_site());
            random_tick_states.push(state_id);
        }
        if state.is_air() {
            let state_id = LitInt::new(&state.id.0.to_string(), Span::call_site());
            air_states.push(state_id);
        }
        if state.is_liquid() {
            let state_id = LitInt::new(&state.id.0.to_string(), Span::call_site());
            liquid_states.push(state_id);
        }

        let mut matched_be_id = air_be_network_id;

        if let Some(geyser_entry) = bedrock_mappings_list.get(state.id.0 as usize) {
            let (bedrock_name, bedrock_props) = parse_geyser_entry(geyser_entry, &block.name);

            if let Some(be_variants) = be_blocks.get(&bedrock_name) {
                matched_be_id = be_variants
                    .iter()
                    .find(|(_, be_props)| {
                        bedrock_props
                            .iter()
                            .all(|(k, v)| be_props.get(k).is_some_and(|be_v| be_v == v))
                    })
                    .map_or_else(
                        || be_variants.first().map_or(air_be_network_id, |(id, _)| *id),
                        |(id, _)| *id,
                    );
            } else if let Some(be_variants) = be_blocks.get(&block.name) {
                matched_be_id = be_variants.first().map_or(air_be_network_id, |(id, _)| *id);
            }
        } else if let Some(be_variants) = be_blocks.get(&block.name) {
            matched_be_id = be_variants.first().map_or(air_be_network_id, |(id, _)| *id);
        }

        block_state_to_bedrock.push((state.id.0, matched_be_id));
        raw_id_from_state_id_array.push((state.id.0, quote! { BlockId::#const_ident }));
        state_from_state_id_array.push((const_ident.clone(), i, state.id.0));
    }
}

/// Adds one block's constants, arms, states and property group.
pub(super) fn push_block(block: Block, ctx: &BlockContext<'_>, tables: &mut BlockTables) {
    let BlockTables {
        lists:
            BlockLists {
                constants_list,
                block_id_constants,
                block_from_name_entries,
                block_from_item_id_arms,
                spawn_floor_predicate_arms,
                suffocation_predicate_arms,
            },
        states,
        type_from_raw_id_array,
        property_enums,
        property_collection_map,
        existing_item_ids,
    } = tables;
    let BlockContext {
        generated_prop_map, ..
    } = ctx;
    let mut property_collection = HashSet::new();
    let property_mapping = property_mapping(&block, ctx, &mut property_collection, property_enums);

    let mut multiplier = 1;
    let mut property_descriptors = Vec::new();
    for hash in block.properties.iter().rev() {
        let gen_prop = generated_prop_map.get(hash).unwrap();
        let variant_count = match &gen_prop.property_type {
            GeneratedPropertyType::Boolean => 2,
            GeneratedPropertyType::Int { min, max } => (max - min + 1) as u16,
            GeneratedPropertyType::Enum { values } => values.len() as u16,
        };
        property_descriptors.push(quote! {
            PropertyDescriptor {
                hash_key: #hash,
                multiplier: #multiplier,
                variant_count: #variant_count,
            }
        });
        multiplier *= variant_count;
    }

    let const_ident = format_ident!("{}", const_block_name_from_block_name(&block.name));
    let name_str = &block.name;
    let item_id = block.item_id;
    let block_id = block.id;

    if let Some(predicate) = block.spawn_floor_predicate {
        spawn_floor_predicate_arms.push(quote! {
            BlockId::#const_ident => #predicate,
        });
    }

    if let Some(predicate) = block.suffocation_predicate {
        suffocation_predicate_arms.push(quote! {
            BlockId::#const_ident => #predicate,
        });
    }

    // let mut block_with_descriptors = block.clone();
    // block_with_descriptors.property_descriptors = property_descriptors;

    constants_list.push(quote! {
        pub const #const_ident: Self = #block;
    });

    block_id_constants.push(quote! {
        pub const #const_ident: Self = #block_id;
    });

    type_from_raw_id_array.push((block.id.0, quote! { &Block::#const_ident }));

    block_from_name_entries.push(quote! {
        #name_str => Block::#const_ident,
    });

    push_states(&block, &const_ident, ctx, states);

    if !property_collection.is_empty() {
        let mut property_collection_vec: Vec<i32> = property_collection.into_iter().collect();
        property_collection_vec.sort_unstable();

        property_collection_map
            .entry(property_collection_vec)
            .or_insert_with(|| PropertyCollectionData::from_mappings(property_mapping))
            .add_block(block.name.clone(), block.id.0);
    }

    if existing_item_ids.insert(item_id) {
        block_from_item_id_arms.push(quote! {
            #item_id => Some(&Self::#const_ident),
        });
    }
}

/// The property structs and the two `Block` property lookups.
pub(super) struct PropertyGroupArms {
    pub(super) block_properties_from_state_and_block_id_arms: Vec<TokenStream>,
    pub(super) block_properties_from_props_and_name_arms: Vec<TokenStream>,
    pub(super) block_properties: Vec<BlockPropertyStruct>,
}

/// Builds one property struct per property group, with its aliases and lookup arms.
pub(super) fn property_group_arms(
    property_collection_map: BTreeMap<Vec<i32>, PropertyCollectionData>,
) -> PropertyGroupArms {
    let mut block_properties: Vec<BlockPropertyStruct> = Vec::new();
    let mut block_properties_from_state_and_block_id_arms = Vec::new();
    let mut block_properties_from_props_and_name_arms = Vec::new();

    let mut emitted_aliases = HashSet::new();
    for property_group in property_collection_map.values() {
        emitted_aliases.insert(property_group_name_from_derived_name(
            &property_group.derive_name(),
        ));
    }

    for property_group in property_collection_map.into_values() {
        let struct_name = property_group_name_from_derived_name(&property_group.derive_name());
        let property_name = Ident::new(&struct_name, Span::call_site());

        let mut group_aliases = Vec::new();

        if let Some(canonical) = common_suffix_group_alias(&property_group.blocks) {
            if emitted_aliases.insert(canonical.clone()) {
                group_aliases.push(Ident::new(&canonical, Span::call_site()));
            }
        }

        for (b_name, _) in &property_group.blocks {
            let alias_name = format!("{}_properties", b_name).to_upper_camel_case();
            if emitted_aliases.insert(alias_name.clone()) {
                group_aliases.push(Ident::new(&alias_name, Span::call_site()));
            }
        }

        let idents: Box<_> = property_group
            .blocks
            .iter()
            .map(|(name, _)| Ident::new(&const_block_name_from_block_name(name), Span::call_site()))
            .collect();

        block_properties_from_state_and_block_id_arms.push(quote! {
            #(BlockId::#idents)|* => Box::new(#property_name::from_state_id(state_id)),
        });
        block_properties_from_props_and_name_arms.push(quote! {
            #(BlockId::#idents)|* => Box::new(#property_name::from_props(props, self)),
        });

        block_properties.push(BlockPropertyStruct {
            data: property_group,
            aliases: group_aliases,
        });
    }
    PropertyGroupArms {
        block_properties_from_state_and_block_id_arms,
        block_properties_from_props_and_name_arms,
        block_properties,
    }
}
