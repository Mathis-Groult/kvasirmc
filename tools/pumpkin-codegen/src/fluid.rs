mod registry;
mod static_items;

use heck::{ToShoutySnakeCase, ToUpperCamelCase};
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, format_ident, quote};
use registry::{
    FluidTables, partial_state_tokens, property_group_tokens, push_fluid, state_id_arm_tokens,
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
};
use syn::{Ident, LitInt, LitStr};

use crate::block::BlockStateId;

/// Converts a fluid name (e.g. `water`) into its SCREAMING_SNAKE_CASE constant identifier.
fn const_fluid_name_from_fluid_name(fluid: &str) -> String {
    fluid.to_shouty_snake_case()
}

/// Derives the PascalCase property-group struct name from a fluid derived name.
fn property_group_name_from_derived_name(name: &str) -> String {
    format!("{name}_fluid_properties").to_upper_camel_case()
}

/// Maps a fluid property's original snake_case name to the generated enum type name.
struct PropertyVariantMapping {
    /// Original property name as it appears in the JSON (e.g. `"level"`).
    original_name: String,
    /// PascalCase name of the generated enum for this property (e.g. `"Level"`).
    property_enum: String,
}

/// Aggregated data for a group of fluids that share the same set of block properties.
struct PropertyCollectionData {
    /// The list of property-to-enum mappings shared by all fluids in this group.
    variant_mappings: Vec<PropertyVariantMapping>,
    /// Names of the fluids that belong to this property group.
    fluid_names: Vec<String>,
}

impl PropertyCollectionData {
    /// Appends a fluid name to this property group.
    pub fn add_fluid_name(&mut self, fluid_name: String) {
        self.fluid_names.push(fluid_name);
    }

    /// Creates a new `PropertyCollectionData` from a set of variant mappings with no fluids yet.
    pub const fn from_mappings(variant_mappings: Vec<PropertyVariantMapping>) -> Self {
        Self {
            variant_mappings,
            fluid_names: Vec::new(),
        }
    }

    /// Derives a representative name for this property group from the first fluid's name.
    pub fn derive_name(&self) -> String {
        format!("{}_like", self.fluid_names[0])
    }
}

/// A fluid block property descriptor holding the property name and its allowed values.
#[derive(Deserialize, Clone, Debug)]
pub struct PropertyStruct {
    /// Name of the generated enum (PascalCase, e.g. `"Level"`).
    pub name: String,
    /// Allowed string values for this property (e.g. `["0", "1", …, "8"]`).
    pub values: Vec<String>,
}

impl ToTokens for PropertyStruct {
    /// Emits an enum definition with `EnumVariants` impl for this property.
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let name = Ident::new(&self.name, Span::call_site());

        let variant_count = self.values.len() as u16;
        let values_index = (0..self.values.len() as u16).collect::<Vec<_>>();

        let ident_values = self.values.iter().map(|value| {
            let value_str = if value.chars().all(char::is_numeric) {
                format!("L{value}")
            } else {
                value.clone()
            };
            Ident::new(&value_str.to_upper_camel_case(), Span::call_site())
        });

        let values_2 = ident_values.clone();
        let values_3 = ident_values.clone();

        let from_values = self.values.iter().map(|value| {
            let value_str = if value.chars().all(char::is_numeric) {
                format!("L{value}")
            } else {
                value.clone()
            };
            let ident = Ident::new(&value_str.to_upper_camel_case(), Span::call_site());
            quote! {
                #value => Self::#ident
            }
        });
        let to_values = self.values.iter().map(|value| {
            let value_str = if value.chars().all(char::is_numeric) {
                format!("L{value}")
            } else {
                value.clone()
            };
            let ident = Ident::new(&value_str.to_upper_camel_case(), Span::call_site());
            quote! {
                Self::#ident => #value
            }
        });

        tokens.extend(quote! {
            #[derive(Clone, Copy, Eq, PartialEq)]
            pub enum #name {
                #(#ident_values),*
            }

            impl EnumVariants for #name {
                fn variant_count() -> u16 {
                    #variant_count
                }

                fn to_index(&self) -> u16 {
                    match self {
                        #(Self::#values_2 => #values_index),*
                    }
                }

                fn from_index(index: u16) -> Self {
                    match index {
                        #(#values_index => Self::#values_3,)*
                        _ => panic!("Invalid index: {index}"),
                    }
                }

                fn to_value(&self) -> &str {
                    match self {
                        #(#to_values),*
                    }
                }

                fn from_value(value: &str) -> Self {
                    match value {
                        #(#from_values),*,
                        _ => panic!("Invalid value: {value:?}"),
                    }
                }
            }
        });
    }
}

/// A fully resolved property group struct ready to emit as a `FluidProperties` impl.
struct FluidPropertyStruct {
    /// The underlying property collection data for this fluid group.
    data: PropertyCollectionData,
}

impl ToTokens for FluidPropertyStruct {
    /// Emits a struct definition and its `FluidProperties` impl for this property group.
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let struct_name = property_group_name_from_derived_name(&self.data.derive_name());
        let name = Ident::new(&struct_name, Span::call_site());

        let values = self.data.variant_mappings.iter().map(|entry| {
            let key = Ident::new_raw(&entry.original_name, Span::call_site());
            let value = Ident::new(&entry.property_enum, Span::call_site());

            quote! {
                #key: #value
            }
        });

        let fluid_names = &self.data.fluid_names;

        let field_names: Vec<_> = self
            .data
            .variant_mappings
            .iter()
            .rev()
            .map(|entry| Ident::new_raw(&entry.original_name, Span::call_site()))
            .collect();

        let field_types: Vec<_> = self
            .data
            .variant_mappings
            .iter()
            .rev()
            .map(|entry| Ident::new(&entry.property_enum, Span::call_site()))
            .collect();

        let to_props_values = self.data.variant_mappings.iter().map(|entry| {
            let key = &entry.original_name;
            let key2 = Ident::new_raw(&entry.original_name, Span::call_site());

            quote! {
                (#key.to_string(), self.#key2.to_value().to_string()),
            }
        });

        let from_props_values = self.data.variant_mappings.iter().map(|entry| {
            let key = &entry.original_name;
            let key2 = Ident::new_raw(&entry.original_name, Span::call_site());
            let value = Ident::new(&entry.property_enum, Span::call_site());

            quote! {
                #key => fluid_props.#key2 = #value::from_value(&value)
            }
        });

        tokens.extend(quote! {
            #[derive(Clone, Copy, Eq, PartialEq)]
            pub struct #name {
                #(pub #values),*
            }

            impl FluidProperties for #name {
                #[allow(unused_assignments)]
                fn to_index(&self) -> u16 {
                    let mut index = 0;
                    let mut multiplier = 1;

                    #(
                        index += self.#field_names.to_index() * multiplier;
                        multiplier *= #field_types::variant_count();
                    )*

                    index
                }

                #[allow(unused_assignments)]
                fn from_index(mut index: u16) -> Self {
                    Self {
                        #(
                            #field_names: {
                                let value = index % #field_types::variant_count();
                                index /= #field_types::variant_count();
                                #field_types::from_index(value)
                            }
                        ),*
                    }
                }

                fn to_state_id(&self, fluid: &Fluid) -> BlockStateId {
                    if ![#(#fluid_names),*].contains(&fluid.name) {
                        panic!("{} is not a valid fluid for {}", fluid.name, #struct_name);
                    }

                    let prop_index = self.to_index();
                    if prop_index < fluid.states.len() as u16 {
                        fluid.states[prop_index as usize].block_state_id
                    } else {
                        fluid.states[fluid.default_state_index as usize].block_state_id
                    }
                }

                fn from_state_id(id: BlockStateId, fluid: &Fluid) -> Self {
                    if ![#(#fluid_names),*].contains(&fluid.name) {
                        panic!("{} is not a valid fluid for {}", &fluid.name, #struct_name);
                    }

                    for (idx, state) in fluid.states.iter().enumerate() {
                        if state.block_state_id == id {
                            return Self::from_index(idx as u16);
                        }
                    }

                    Self::from_index(fluid.default_state_index)
                }

                fn default(fluid: &Fluid) -> Self {
                    if ![#(#fluid_names),*].contains(&fluid.name) {
                        panic!("{} is not a valid fluid for {}", &fluid.name, #struct_name);
                    }

                    Self::from_index(fluid.default_state_index)
                }

                fn to_props(&self) -> Vec<(String, String)> {
                   vec![#(#to_props_values)*]
                }

                fn from_props(props: Vec<(String, String)>, fluid: &Fluid) -> Self {
                    if ![#(#fluid_names),*].contains(&fluid.name) {
                        panic!("{} is not a valid fluid for {}", &fluid.name, #struct_name);
                    }

                    let mut fluid_props = Self::default(fluid);

                    for (key, value) in props {
                        match key.as_str() {
                            #(#from_props_values),*,
                            _ => panic!("Invalid key: {key}"),
                        }
                    }

                    fluid_props
                }
            }
        });
    }
}

/// Raw deserialization shape for a single fluid state entry from `fluids.json`.
#[derive(Deserialize, Clone)]
struct FluidState {
    /// Fraction of a full block that this fluid fills (0.0–1.0).
    height: f32,
    /// Numeric fluid level (0 = source, 1–7 = flowing).
    level: i16,
    /// Whether this state represents an empty (air) fluid slot.
    is_empty: bool,
    /// Blast resistance of the fluid in this state.
    blast_resistance: f32,
    /// Block state ID used to identify this fluid state in the world.
    block_state_id: BlockStateId,
    /// Whether the fluid is still (not flowing).
    is_still: bool,
    // We'll derive is_source and falling from existing fields instead of requiring them in JSON
}

/// A lightweight reference to a fluid state, pairing its fluid-relative index with a
/// deduplicated partial-state index.
#[derive(Clone, Debug)]
struct FluidStateRef {
    /// Index of this state within its fluid's state array.
    pub id: u16,
    /// Index into the global `FLUID_STATES` deduplication table.
    pub state_idx: u16,
}
impl ToTokens for FluidStateRef {
    /// Emits a `FluidStateRef { id, state_idx }` struct literal.
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let id = LitInt::new(&self.id.to_string(), Span::call_site());
        let state_idx = LitInt::new(&self.state_idx.to_string(), Span::call_site());
        tokens.extend(quote! {
            FluidStateRef {
                id: #id,
                state_idx: #state_idx,
            }
        });
    }
}

/// A single block-state property for a fluid, as listed in `fluids.json`.
#[derive(Deserialize, Clone)]
struct Property {
    /// Name of the property (e.g. `"level"`).
    name: String,
    /// List of valid string values for this property.
    values: Vec<String>,
}

/// Raw deserialization shape for a single fluid entry from `fluids.json`.
#[derive(Deserialize, Clone)]
pub struct Fluid {
    /// Registry name of the fluid (e.g. `"water"`).
    pub name: String,
    /// Numeric ID identifying this fluid type.
    pub id: u16,
    /// Block-state properties this fluid exposes (e.g. `level`, `falling`).
    properties: Vec<Property>,
    /// Index within `states` of the default fluid state.
    default_state_index: u16,
    /// All possible states for this fluid, one per property combination.
    states: Vec<FluidState>,
    /// Ticks between each flow-spread step (lower = faster).
    #[serde(default = "default_flow_speed")]
    flow_speed: u32,
    /// Maximum number of blocks this fluid can flow horizontally from a source.
    #[serde(default = "default_flow_distance")]
    flow_distance: u32,
    /// Whether two adjacent source blocks can create a new source block.
    #[serde(default)]
    can_convert_to_source: bool,
}

/// Default flow speed (ticks between spread steps) matching vanilla water.
const fn default_flow_speed() -> u32 {
    5 // Default to water's speed
}

/// Default horizontal flow distance in blocks, matching vanilla water.
const fn default_flow_distance() -> u32 {
    4 // Default to water's distance
}

/// Generates the `TokenStream` for the `Fluid` struct, `FluidState`, `FluidProperties` trait,
/// per-fluid property enums, and all lookup functions.
pub fn build() -> TokenStream {
    let fluids: Vec<Fluid> =
        match serde_json::from_str(&fs::read_to_string("../../assets/fluids.json").unwrap()) {
            Ok(fluids) => fluids,
            Err(e) => panic!("Failed to parse fluids.json: {e}"),
        };

    let mut tables = FluidTables::default();
    for fluid in fluids {
        push_fluid(fluid, &mut tables);
    }
    let FluidTables {
        constants,
        id_matches,
        type_from_name,
        type_from_raw_id_arms,
        state_id_arms,
        property_enums,
        property_collection_map,
        unique_states,
        ..
    } = tables;

    let unique_fluid_states = partial_state_tokens(&unique_states);
    let fluid_from_state_id = state_id_arm_tokens(state_id_arms);
    let (
        fluid_properties_from_state_and_name,
        fluid_properties_from_props_and_name,
        fluid_properties,
    ) = property_group_tokens(property_collection_map);

    let fluid_props = fluid_properties.iter().map(ToTokens::to_token_stream);
    let properties = property_enums.values().map(ToTokens::to_token_stream);

    let imports_and_partial_fluid_state_tokens =
        static_items::imports_and_partial_fluid_state_tokens();
    let enum_variants_tokens = static_items::enum_variants_tokens();
    let same_fluid_type_tokens = static_items::same_fluid_type_tokens();
    let to_resource_location_tokens = static_items::to_resource_location_tokens();
    quote! {
        #imports_and_partial_fluid_state_tokens

        pub const FLUID_STATES: &[PartialFluidState] = &[
            #(#unique_fluid_states),*
        ];

        #enum_variants_tokens

        impl Fluid {
            #constants

            pub fn from_registry_key(name: &str) -> Option<&'static Self> {
                match name {
                    #type_from_name
                    _ => None
                }
            }

            pub const fn from_id(id: u16) -> Option<&'static Self> {
                match id {
                    #type_from_raw_id_arms
                    _ => None
                }
            }

            #[allow(unreachable_patterns, clippy::match_overlapping_arm)]
            pub const fn from_state_id(id: BlockStateId) -> Option<&'static Self> {
                match id.as_u16() {
                    #fluid_from_state_id
                    _ => None
                }
            }

            pub fn ident_to_fluid_id(name: &str) -> Option<u8> {
                match name {
                    #(#id_matches)*
                    _ => None
                }
            }

            #[track_caller]
            #[doc = r" Get the properties of the fluid."]
            pub fn properties(&self, id: BlockStateId) -> Box<dyn FluidProperties> {
                match self.name {
                    #fluid_properties_from_state_and_name
                    _ => panic!("Invalid state_id")
                }
            }

            #[track_caller]
            #[doc = r" Get the properties of the fluid."]
            pub fn from_properties(&self, props: Vec<(String, String)>) -> Box<dyn FluidProperties> {
                match self.name {
                    #fluid_properties_from_props_and_name
                    _ => panic!("Invalid props")
                }
            }

            #same_fluid_type_tokens
        }

        #to_resource_location_tokens

        #(#properties)*

        #(#fluid_props)*
    }
}
