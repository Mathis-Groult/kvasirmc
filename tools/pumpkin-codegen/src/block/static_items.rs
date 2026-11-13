//! The fixed part of the generated block file.

use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn items_tokens() -> TokenStream {
    quote! {
        #[allow(clippy::wildcard_imports, clippy::enum_glob_use, clippy::too_many_lines, clippy::match_same_arms)]
        use pumpkin_util::math::boundingbox::BoundingBox;

        use crate::{
            BlockState, BlockStateId, Block, BlockId,
            blocks::{Flammable, ShapeOffset, ShapeOffsetType, SpawnFloorPredicate, SuffocationPredicate},
        };
        use crate::block_state::{PistonBehavior, SoundType};
        use crate::sound::Sound;
        use pumpkin_util::math::int_provider::{UniformIntProvider, IntProvider, NormalIntProvider};
        use pumpkin_util::math::experience::Experience;
        use pumpkin_util::math::vector3::Vector3;
        use std::collections::BTreeMap;
    }
}

pub(super) fn block_property_tokens() -> TokenStream {
    quote! {
        #[derive(Clone, Copy, Debug)]
        pub struct BlockProperty {
            pub name: &'static str,
            pub values: &'static [&'static str],
        }

        pub trait BlockProperties where Self: 'static {
            fn to_index(&self) -> u16;
            fn from_index(index: u16) -> Self where Self: Sized;
            fn handles_block_id(id: BlockId) -> bool where Self: Sized;
            fn to_state_id(&self, block: &Block) -> BlockStateId;
            fn from_state_id(id: BlockStateId, block: &Block) -> Self where Self: Sized;
            fn default(block: &Block) -> Self where Self: Sized;
            fn to_props(&self) -> Vec<Property>;
            fn from_props(props: &[Property], block: &Block) -> Self where Self: Sized;
        }

        pub trait EnumVariants {
            fn variant_count() -> u16;
            fn to_index(&self) -> u16;
            fn from_index(index: u16) -> Self;
            fn to_value(&self) -> &'static str;
            fn from_value(value: &str) -> Self;
        }
    }
}

pub(super) fn from_id_tokens() -> TokenStream {
    quote! {
        /// Get a [`BlockState`] from a [`BlockStateId`].
        /// If you need access to the block use `BlockState::from_id_with_block` instead.
        #[inline]
        #[must_use]
        pub const fn from_id(id: BlockStateId) -> &'static Self {
            // Safety: We always check this condition when creating a BlockStateId.
            // the u16 field is private and immutable. BlockStateId::STATE_COUNT is a const u16.
            // If the condition held once, it will always hold.
            unsafe { std::hint::assert_unchecked(id.as_u16() < BlockStateId::STATE_COUNT) }
            // This hint guarantees that bound checks can be optimized away in release builds.
            // Due to debug_assertions forcing -Zub_checks=yes (rust-lang/rust#123499)
            // bound checks-panics are replaced with ub-checks on profile.dev
            // https://doc.rust-lang.org/nightly/unstable-book/compiler-flags/ub-checks.html

            mappings::STATE_FROM_STATE_ID[id.as_u16() as usize]
        }

        #[doc = r" Get a block state from a state id and the corresponding block."]
        #[inline]
        #[must_use]
        pub const fn from_id_with_block(id: BlockStateId) -> (&'static Block, &'static Self) {
            let block = Block::from_state_id(id);
            let state = Self::from_id(id);
            (block, state)
        }

        #[must_use]
        pub const fn to_be_network_id(id: BlockStateId) -> u32 {
            // Safety: We always check this condition when creating a BlockStateId.
            // the u16 field is private and immutable. BlockStateId::STATE_COUNT is a const u16.
            // If the condition held once, it will always hold.
            unsafe { std::hint::assert_unchecked(id.as_u16() < BlockStateId::STATE_COUNT) }
            // This hint guarantees that bound checks can be optimized away in release builds.
            // Due to debug_assertions forcing -Zub_checks=yes (rust-lang/rust#123499)
            // bound checks-panics are replaced with ub-checks on profile.dev
            // https://doc.rust-lang.org/nightly/unstable-book/compiler-flags/ub-checks.html

            Self::STATE_ID_TO_BEDROCK[id.as_u16() as usize]
        }
    }
}

pub(super) fn from_registry_key_tokens() -> TokenStream {
    quote! {
        #[doc = r" Try to parse a block from a resource location string."]
        #[inline]
        #[must_use]
        pub fn from_registry_key(name: &str) -> Option<&'static Self> {
            mappings::BLOCK_FROM_NAME_MAP.get(name)
        }

        #[doc = r" Try to get a block from a namespace prefixed name."]
        #[must_use]
        pub fn from_name(name: &str) -> Option<&'static Self> {
            let key = name.strip_prefix("minecraft:").unwrap_or(name);
            mappings::BLOCK_FROM_NAME_MAP.get(key)
        }

        /// Get a [`Block`] from a [`BlockId`]
        #[inline]
        #[must_use]
        pub const fn from_id(id: BlockId) -> &'static Self {
            // Safety: We always check this condition when creating a BlockId.
            // the u16 field is private and immutable. BlockId::BLOCK_COUNT is a const u16.
            // If the condition held once, it will always hold.
            unsafe { std::hint::assert_unchecked(id.as_u16() < BlockId::BLOCK_COUNT) }
            // This hint guarantees that bound checks can be optimized away in release builds.
            // Due to debug_assertions forcing -Zub_checks=yes (rust-lang/rust#123499)
            // bound checks-panics are replaced with ub-checks on profile.dev
            // https://doc.rust-lang.org/nightly/unstable-book/compiler-flags/ub-checks.html

            mappings::TYPE_FROM_RAW_ID[id.as_u16() as usize]
        }

        /// Get a [`Block`] from a state id
        #[inline]
        #[must_use]
        pub const fn from_state_id(id: BlockStateId) -> &'static Self {
            Self::from_id(BlockId::from_state_id(id))
        }
    }
}

pub(super) fn facing_tokens() -> TokenStream {
    quote! {
        impl Facing {
            #[must_use]
            pub const fn opposite(&self) -> Self {
                match self {
                    Self::North => Self::South,
                    Self::South => Self::North,
                    Self::East => Self::West,
                    Self::West => Self::East,
                    Self::Up => Self::Down,
                    Self::Down => Self::Up,
                }
            }
        }

        impl HorizontalFacing {
            #[must_use]
            pub fn all() -> [Self; 4] {
                [
                    Self::North,
                    Self::South,
                    Self::West,
                    Self::East,
                ]
            }

            #[must_use]
            pub fn to_offset(&self) -> Vector3<i32> {
                match self {
                    Self::North => (0, 0, -1),
                    Self::South => (0, 0, 1),
                    Self::West => (-1, 0, 0),
                    Self::East => (1, 0, 0),
                }
                .into()
            }
            #[must_use]
            pub const fn to_axis(&self) -> HorizontalAxis {
                match self {
                    Self::North | Self::South => HorizontalAxis::Z,
                    Self::West | Self::East => HorizontalAxis::X,
                }
            }
             #[must_use]
            pub const fn to_facing(&self) -> HorizontalFacing {
                match self {
                    Self::North => HorizontalFacing::North,
                    Self::South => HorizontalFacing::South,
                    Self::West => HorizontalFacing::West,
                    Self::East => HorizontalFacing::East,
                }
            }
            #[must_use]
            pub const fn opposite(&self) -> Self {
                match self {
                    Self::North => Self::South,
                    Self::South => Self::North,
                    Self::West => Self::East,
                    Self::East => Self::West,
                }
            }

            #[must_use]
            pub const fn rotate_clockwise(&self) -> Self {
                match self {
                    Self::North => Self::East,
                    Self::South => Self::West,
                    Self::West => Self::North,
                    Self::East => Self::South,
                }
            }

            #[must_use]
            pub const fn rotate_counter_clockwise(&self) -> Self {
                match self {
                    Self::North => Self::West,
                    Self::South => Self::East,
                    Self::West => Self::South,
                    Self::East => Self::North,
                }
            }
        }
    }
}

pub(super) fn rail_shape_tokens() -> TokenStream {
    quote! {
        impl RailShape {
            #[must_use]
            pub const fn is_ascending(&self) -> bool {
                matches!(self, Self::AscendingEast | Self::AscendingWest | Self::AscendingNorth | Self::AscendingSouth)
            }
        }

        impl RailShapeStraight {
            #[must_use]
            pub const fn is_ascending(&self) -> bool {
                matches!(self, Self::AscendingEast | Self::AscendingWest | Self::AscendingNorth | Self::AscendingSouth)
            }
        }
    }
}
