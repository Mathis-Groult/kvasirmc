//! The fixed part of the generated fluid file.

use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn imports_and_partial_fluid_state_tokens() -> TokenStream {
    quote! {
        use std::hash::{Hash, Hasher};
        use crate::tag::{Taggable, RegistryKey};
        use crate::BlockStateId;
        use pumpkin_util::resource_location::{FromResourceLocation, ResourceLocation, ToResourceLocation};

        #[derive(Clone)]
        pub struct PartialFluidState {
            pub height: f32,
            pub level: i16,
            pub is_empty: bool,
            pub blast_resistance: f32,
            pub block_state_id: BlockStateId,
            pub is_still: bool,
            pub is_source: bool,
            pub falling: bool,
        }

        #[derive(Clone)]
        pub struct FluidState {
            pub height: f32,
            pub level: i16,
            pub is_empty: bool,
            pub blast_resistance: f32,
            pub block_state_id: BlockStateId,
            pub is_still: bool,
            pub is_source: bool,
            pub falling: bool,
        }

        #[derive(Clone)]
        pub struct FluidStateRef {
            pub id: u16,
            pub state_idx: u16,
        }

        #[derive(Clone)]
        pub struct Fluid {
            pub id: u16,
            pub name: &'static str,
            pub properties: Option<&'static [(&'static str, &'static [&'static str])]>,
            pub states: &'static [FluidState],
            pub default_state_index: u16,
            pub flow_speed: u32,
            pub flow_distance: u32,
            pub can_convert_to_source: bool,
        }

        impl Hash for Fluid {
            fn hash<H: Hasher>(&self, state: &mut H) {
                self.id.hash(state);
            }
        }

        impl PartialEq for Fluid {
            fn eq(&self, other: &Self) -> bool {
                self.id == other.id
            }
        }

        impl Eq for Fluid {}
    }
}

pub(super) fn enum_variants_tokens() -> TokenStream {
    quote! {
        pub trait EnumVariants {
            fn variant_count() -> u16;
            fn to_index(&self) -> u16;
            fn from_index(index: u16) -> Self;
            fn to_value(&self) -> &str;
            fn from_value(value: &str) -> Self;
        }

        pub trait FluidProperties where Self: 'static {
            // Convert properties to an index (`0` to `N-1`).
            fn to_index(&self) -> u16;
            // Convert an index back to properties.
            fn from_index(index: u16) -> Self where Self: Sized;

            // Convert properties to a state id.
            fn to_state_id(&self, fluid: &Fluid) -> BlockStateId;
            // Convert a state id back to properties.
            fn from_state_id(id: BlockStateId, fluid: &Fluid) -> Self where Self: Sized;
            // Get the default properties.
            fn default(fluid: &Fluid) -> Self where Self: Sized;

            // Convert properties to a `Vec` of `(name, value)`
            fn to_props(&self) -> Vec<(String, String)>;

            // Convert properties to a fluid state, and add them onto the default state.
            fn from_props(props: Vec<(String, String)>, fluid: &Fluid) -> Self where Self: Sized;
        }

        pub fn get_fluid(registry_id: &str) -> Option<&'static Fluid> {
           let key = registry_id.strip_prefix("minecraft:").unwrap_or(registry_id);
           Fluid::from_registry_key(key)
        }
    }
}

pub(super) fn same_fluid_type_tokens() -> TokenStream {
    quote! {
        pub fn same_fluid_type(a: u16, b: u16) -> bool {
            a == b
                || (a == 1 && b == 2)
                || (a == 2 && b == 1)
                || (a == 3 && b == 4)
                || (a == 4 && b == 3)
        }
        pub fn matches_type(&self, other: &Fluid) -> bool {
            Self::same_fluid_type(self.id, other.id)
        }
        pub fn to_flowing(&self) -> &'static Fluid {
            match self.id {
                2 => &Fluid::FLOWING_WATER,
                4 => &Fluid::FLOWING_LAVA,
                _ => Fluid::from_id(self.id).unwrap_or(&Fluid::EMPTY),
            }
        }

        // Added helper methods for fluid behavior
        pub fn is_source(&self, state_id: BlockStateId) -> bool {
            let idx = (state_id.as_u16() as usize) % self.states.len();
            self.states[idx].is_source
        }

        pub fn is_falling(&self, state_id: BlockStateId) -> bool {
            let idx = (state_id.as_u16() as usize) % self.states.len();
            self.states[idx].falling
        }

        pub fn get_level(&self, state_id: BlockStateId) -> i16 {
            let idx = (state_id.as_u16() as usize) % self.states.len();
            self.states[idx].level
        }

        pub fn get_height(&self, state_id: BlockStateId) -> f32 {
            let idx = (state_id.as_u16() as usize) % self.states.len();
            self.states[idx].height
        }
    }
}

pub(super) fn to_resource_location_tokens() -> TokenStream {
    quote! {
        impl ToResourceLocation for &'static Fluid {
            fn to_resource_location(&self) -> ResourceLocation {
                format!("minecraft:{}", self.name)
            }
        }

        impl FromResourceLocation for &'static Fluid {
            fn from_resource_location(resource_location: &ResourceLocation) -> Option<Self> {
                Fluid::from_registry_key(resource_location.strip_prefix("minecraft:").unwrap_or(resource_location))
            }
        }

        impl FluidStateRef {
            pub fn get_state(&self) -> FluidState {
                let partial_state = &FLUID_STATES[self.state_idx as usize];
                FluidState {
                    height: partial_state.height,
                    level: partial_state.level,
                    is_empty: partial_state.is_empty,
                    blast_resistance: partial_state.blast_resistance,
                    block_state_id: partial_state.block_state_id,
                    is_still: partial_state.is_still,
                    is_source: partial_state.is_source,
                    falling: partial_state.falling,
                }
            }
        }

        impl Taggable for Fluid {
            #[inline]
            fn tag_key() -> RegistryKey {
                RegistryKey::Fluid
            }

            #[inline]
            fn registry_key(&self) -> &str {
                self.name
            }

            #[inline]
            fn registry_id(&self) -> u16 {
                self.id
            }
        }

        // Added FluidLevel enum and required constants
        pub const FLUID_LEVEL_SOURCE: i32 = 0;
        pub const FLUID_LEVEL_FLOWING_MAX: i32 = 8;
        pub const FLUID_MIN_HEIGHT: f32 = 0.0;
        pub const FLUID_MAX_HEIGHT: f32 = 1.0;
    }
}
