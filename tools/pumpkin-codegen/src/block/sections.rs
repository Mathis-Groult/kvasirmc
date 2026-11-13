//! The interpolated sections of the generated block file (property enums, tables, mappings, `impl Block`).

use super::property_value::PropertyValueArms;
use super::registry::BlockLists;
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

/// The `PropertyKey` enum and its string conversions.
pub(super) fn property_key_tokens(
    property_key_variants: &[Ident],
    property_key_literals: &[&str],
) -> TokenStream {
    quote! {
    #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
    pub enum PropertyKey {
        #(#property_key_variants),*
    }

    impl PropertyKey {
        #[must_use]
        pub const fn to_str(self) -> &'static str {
            match self {
                #(Self::#property_key_variants => #property_key_literals),*
            }
        }

        #[must_use]
        #[allow(clippy::should_implement_trait)]
        pub fn from_str(name: &str) -> Option<Self> {
            match name {
                #(#property_key_literals => Some(Self::#property_key_variants),)*
                _ => None,
            }
        }
    }

    impl AsRef<str> for PropertyKey {
        fn as_ref(&self) -> &str {
            self.to_str()
        }
    }
    }
}

/// The `Property` enum and its key, value and parse functions.
pub(super) fn property_enum_tokens(arms: &PropertyValueArms) -> TokenStream {
    let PropertyValueArms {
        property_enum_variants,
        property_key_arms,
        property_value_str_arms,
        property_from_key_and_int_arms,
        property_from_key_value_str_arms,
        property_matches_shared_none_arms,
        ..
    } = arms;
    quote! {
    #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
    pub enum Property {
        #(#property_enum_variants),*
    }

    impl Property {
        #[must_use]
        pub const fn key(self) -> PropertyKey {
            match self {
                #(#property_key_arms)*
            }
        }

        #[must_use]
        pub const fn key_str(self) -> &'static str {
            self.key().to_str()
        }

        #[must_use]
        pub const fn value_str(self) -> &'static str {
            const INT_STRS: [&str; 26] = [
                "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
                "10", "11", "12", "13", "14", "15", "16", "17", "18", "19",
                "20", "21", "22", "23", "24", "25",
            ];
            match self {
                #(#property_value_str_arms)*
            }
        }

        #[must_use]
        pub const fn from_key_and_int(key: PropertyKey, value: u8) -> Option<Self> {
            match key {
                #(#property_from_key_and_int_arms)*
                _ => None,
            }
        }

        #[must_use]
        pub fn from_key_value_str(key: PropertyKey, value: &str) -> Option<Self> {
            match key {
                #(#property_from_key_value_str_arms)*
            }
        }

        #[must_use]
        pub fn from_str_pair(key: &str, value: &str) -> Option<Self> {
            PropertyKey::from_str(key).and_then(|k| Self::from_key_value_str(k, value))
        }

        #[must_use]
        pub fn matches_property(self, other: Self) -> bool {
            self == other || match (self, other) { #(#property_matches_shared_none_arms)* _ => false }
        }
    }
    }
}

/// The shape, sound and block entity tables and the air/liquid/random-tick predicates.
pub(super) fn state_table_tokens(
    shapes: impl Iterator<Item = TokenStream>,
    sound_types: impl Iterator<Item = TokenStream>,
    block_entity_types: impl Iterator<Item = LitStr>,
    air_state_ids: &TokenStream,
    liquid_state_ids: &TokenStream,
    mod_ident: &Ident,
    contains_ident: &Ident,
) -> TokenStream {
    quote! {
    pub const COLLISION_SHAPES: &[BoundingBox] = &[
        #(#shapes),*
    ];

    pub const SOUND_TYPES: &[SoundType] = &[
        #(#sound_types),*
    ];

    pub const BLOCK_ENTITY_TYPES: &[&str] = &[
        #(#block_entity_types),*
    ];

    #[inline(always)]
    #[must_use]
    pub const fn is_air(id: BlockStateId) -> bool {
        matches!(id.as_u16(), #air_state_ids)
    }

     #[inline(always)]
     #[must_use]
    pub const fn is_liquid(id: BlockStateId) -> bool {
        matches!(id.as_u16(), #liquid_state_ids)
    }

    #[inline(always)]
    #[must_use]
    pub fn has_random_ticks(id: BlockStateId) -> bool {
        #mod_ident::#contains_ident(id.as_u16())
    }

    #[must_use]
    pub const fn blocks_movement(block_state: &BlockState, id: BlockId) -> bool {
        block_state.is_solid() && !matches!(id, BlockId::COBWEB | BlockId::BAMBOO_SAPLING)
    }
    }
}

/// The `mappings` module: name map and the state/type id tables.
pub(super) fn mappings_tokens(
    block_from_name_entries: &[TokenStream],
    max_state_id: usize,
    raw_id_from_state_id: &TokenStream,
    max_type_id: usize,
    type_from_raw_id_items: &TokenStream,
    state_from_state_id: &TokenStream,
) -> TokenStream {
    quote! {
    mod mappings {
        use crate::{Block, BlockId, BlockState, BlockStateId};
        use phf;

        pub(super) static BLOCK_FROM_NAME_MAP: phf::Map<&'static str, Block> = phf::phf_map!{
            #(#block_from_name_entries)*
        };

        pub(super) static BLOCK_ID_FROM_STATE_ID: [BlockId; #max_state_id] = [
            #raw_id_from_state_id
        ];

        pub(super) static TYPE_FROM_RAW_ID: [&Block; #max_type_id] = [
            #type_from_raw_id_items
        ];

        pub(super) static STATE_FROM_STATE_ID: [&BlockState; #max_state_id] = [
            #state_from_state_id
        ];
    }
    }
}

/// The `impl Block` block: constants, predicates, lookups and property accessors.
pub(super) fn block_impl_tokens(
    lists: &BlockLists,
    shape_offset_arms: &[TokenStream],
    from_registry_key_tokens: &TokenStream,
    block_properties_from_state_and_block_id_arms: &[TokenStream],
    block_properties_from_props_and_name_arms: &[TokenStream],
) -> TokenStream {
    let BlockLists {
        constants_list,
        spawn_floor_predicate_arms,
        suffocation_predicate_arms,
        block_from_item_id_arms,
        ..
    } = lists;
    quote! {
    impl Block {
        #(#constants_list)*

        #[must_use]
        pub const fn spawn_floor_predicate(&self) -> SpawnFloorPredicate {
            match self.id {
                #(#spawn_floor_predicate_arms)*
                _ => SpawnFloorPredicate::Default,
            }
        }

        #[must_use]
        pub const fn suffocation_predicate(&self) -> SuffocationPredicate {
            match self.id {
                #(#suffocation_predicate_arms)*
                _ => SuffocationPredicate::Default,
            }
        }

        pub(crate) const fn shape_offset(&self) -> Option<ShapeOffset> {
            match self.id {
                #(#shape_offset_arms)*
                _ => None,
            }
        }

        #from_registry_key_tokens

        #[doc = r" Try to parse a block from an item id."]
        #[must_use]
        pub const fn from_item_id(id: u16) -> Option<&'static Self> {
            #[allow(unreachable_patterns)]
            match id {
                #(#block_from_item_id_arms)*
                _ => None
            }
        }

        #[track_caller]
        #[doc = r" Get the properties of the block."]
        pub fn properties(&self, state_id: BlockStateId) -> Option<Box<dyn BlockProperties>> {
            Some(match self.id {
                #(#block_properties_from_state_and_block_id_arms)*
                _ => return None,
            })
        }

        #[track_caller]
        #[doc = r" Get the properties of the block."]
        pub fn from_properties(&self, props: &[Property]) -> Box<dyn BlockProperties> {
            match self.id {
                #(#block_properties_from_props_and_name_arms)*
                _ => panic!("Invalid props")
            }
        }
    }
    }
}

/// The `impl BlockId` block: id constants and `from_state_id`.
pub(super) fn block_id_impl_tokens(block_id_constants: &[TokenStream]) -> TokenStream {
    quote! {
    impl BlockId {
        #(#block_id_constants)*

        pub(crate) const BLOCK_COUNT: u16 = mappings::TYPE_FROM_RAW_ID.len() as u16;

        /// Get a [`BlockId`] from a [`BlockStateId`]
        #[inline]
        #[must_use]
        pub const fn from_state_id(id: BlockStateId) -> BlockId {
            // Safety: We always check this condition when creating a BlockStateId.
            // the u16 field is private and immutable. BlockStateId::STATE_COUNT is a const u16.
            // If the condition held once, it will always hold.
            unsafe { std::hint::assert_unchecked(id.as_u16() < BlockStateId::STATE_COUNT) }
            // This hint guarantees that bound checks can be optimized away in release builds.
            // Due to debug_assertions forcing -Zub_checks=yes (rust-lang/rust#123499)
            // bound checks-panics are replaced with ub-checks on profile.dev
            // https://doc.rust-lang.org/nightly/unstable-book/compiler-flags/ub-checks.html

            mappings::BLOCK_ID_FROM_STATE_ID[id.as_u16() as usize]
        }
    }
    }
}
