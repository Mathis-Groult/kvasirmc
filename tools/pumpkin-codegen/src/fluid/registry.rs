//! Reads the vanilla fluids and gathers their constants, states and property tables.

use super::{
    Fluid, FluidPropertyStruct, FluidState, FluidStateRef, PropertyCollectionData, PropertyStruct,
    PropertyVariantMapping, const_fluid_name_from_fluid_name,
    property_group_name_from_derived_name,
};
use crate::block::BlockStateId;
use heck::{ToShoutySnakeCase, ToUpperCamelCase};
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, format_ident, quote};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
};
use syn::{Ident, LitInt, LitStr};

/// Collect from_state_id arms to sort by range width (narrower first)
pub(super) struct StateIdArm {
    start: u16,
    end: u16,
    const_name: String,
}

/// What the per-fluid pass accumulates.
#[derive(Default)]
pub(super) struct FluidTables {
    pub(super) constants: TokenStream,
    pub(super) id_matches: Vec<TokenStream>,
    pub(super) type_from_name: TokenStream,
    pub(super) type_from_raw_id_arms: TokenStream,
    pub(super) state_id_arms: Vec<StateIdArm>,
    pub(super) property_enums: BTreeMap<String, PropertyStruct>,
    pub(super) property_collection_map: BTreeMap<Vec<String>, PropertyCollectionData>,
    pub(super) enum_to_values: BTreeMap<String, Vec<String>>,
    pub(super) unique_states: Vec<FluidState>,
    pub(super) optimized_fluids: Vec<(String, FluidStateRef)>,
}

/// Adds one fluid's constant, lookups, states and properties.
pub(super) fn push_fluid(fluid: Fluid, tables: &mut FluidTables) {
    let FluidTables {
        constants,
        id_matches,
        type_from_name,
        type_from_raw_id_arms,
        state_id_arms,
        property_enums,
        property_collection_map,
        enum_to_values,
        unique_states,
        optimized_fluids,
    } = tables;
    let id_name = LitStr::new(&fluid.name, Span::call_site());
    let const_ident = format_ident!("{}", fluid.name.to_shouty_snake_case());
    let state_id_start = fluid
        .states
        .iter()
        .map(|state| state.block_state_id.0)
        .min()
        .unwrap();
    let state_id_end = fluid
        .states
        .iter()
        .map(|state| state.block_state_id.0)
        .max()
        .unwrap();

    let id_lit = LitInt::new(&fluid.id.to_string(), Span::call_site());
    let properties = properties_tokens(&fluid);

    push_states(&fluid, unique_states, optimized_fluids);
    state_id_arms.push(StateIdArm {
        start: state_id_start,
        end: state_id_end,
        const_name: const_fluid_name_from_fluid_name(&fluid.name),
    });

    type_from_name.extend(quote! {
        #id_name => Some(&Self::#const_ident),
    });

    type_from_raw_id_arms.extend(quote! {
        #id_lit => Some(&Self::#const_ident),
    });

    let fluid_states = fluid_state_tokens(&fluid);
    let state_id = fluid.default_state_index;
    let flow_speed = fluid.flow_speed;
    let flow_distance = fluid.flow_distance;
    let can_convert_to_source = fluid.can_convert_to_source;

    id_matches.push(quote! {
        #id_name => Some(#id_lit),
    });

    constants.extend(quote! {
        pub const #const_ident: Fluid = Fluid {
            id: #id_lit,
            name: #id_name,
            properties: #properties,
            states: &[#(#fluid_states),*],
            default_state_index: #state_id,
            flow_speed: #flow_speed,
            flow_distance: #flow_distance,
            can_convert_to_source: #can_convert_to_source,
        };
    });

    push_fluid_properties(
        &fluid,
        enum_to_values,
        property_enums,
        property_collection_map,
    );
}

/// The `properties` value of one fluid (`None` or its property name/value table).
fn properties_tokens(fluid: &Fluid) -> TokenStream {
    let mut properties = TokenStream::new();
    if fluid.properties.is_empty() {
        properties.extend(quote!(None));
    } else {
        let internal_properties = fluid.properties.iter().map(|property| {
            let key = LitStr::new(&property.name, Span::call_site());
            let values = property
                .values
                .iter()
                .map(|value| LitStr::new(value, Span::call_site()));

            quote! {
                (#key, &[
                    #(#values),*
                ])
            }
        });
        properties.extend(quote! {
            Some(&[
                #(#internal_properties),*
            ])
        });
    }
    properties
}

/// Records each state of one fluid as a reference into the shared unique-state list.
fn push_states(
    fluid: &Fluid,
    unique_states: &mut Vec<FluidState>,
    optimized_fluids: &mut Vec<(String, FluidStateRef)>,
) {
    for (idx, state) in fluid.states.iter().enumerate() {
        // Check if this state is already in `unique_states` by comparing key fields
        let already_exists = unique_states.iter().any(|s: &FluidState| {
            s.height == state.height
                && s.level == state.level
                && s.is_empty == state.is_empty
                && s.blast_resistance == state.blast_resistance
                && s.is_still == state.is_still
        });
        if !already_exists {
            unique_states.push(state.clone());
        }
        // Create a reference to the state
        let state_idx = unique_states
            .iter()
            .position(|s| {
                s.height == state.height
                    && s.level == state.level
                    && s.is_empty == state.is_empty
                    && s.blast_resistance == state.blast_resistance
                    && s.is_still == state.is_still
            })
            .unwrap() as u16;
        optimized_fluids.push((
            fluid.name.clone(),
            FluidStateRef {
                id: idx as u16,
                state_idx,
            },
        ));
    }
}

/// The `FluidState` values of one fluid.
fn fluid_state_tokens(fluid: &Fluid) -> impl Iterator<Item = TokenStream> + '_ {
    fluid.states.iter().map(|state| {
        let height = state.height;
        let level = state.level;
        let is_empty = state.is_empty;
        let blast_resistance = state.blast_resistance;
        let block_state_id = state.block_state_id;
        let is_still = state.is_still;
        // Derive these values based on existing fields
        let is_source = is_still; // Level 0 and still means it's a source
        let falling = false; // Default to false - we'll handle falling in the fluid behavior code

        quote! {
            FluidState {
                height: #height,
                level: #level,
                is_empty: #is_empty,
                blast_resistance: #blast_resistance,
                block_state_id: #block_state_id,
                is_still: #is_still,
                is_source: #is_source,
                falling: #falling,
            }
        }
    })
}

/// Checks and records the property enums and the property group of one fluid.
fn push_fluid_properties(
    fluid: &Fluid,
    enum_to_values: &mut BTreeMap<String, Vec<String>>,
    property_enums: &mut BTreeMap<String, PropertyStruct>,
    property_collection_map: &mut BTreeMap<Vec<String>, PropertyCollectionData>,
) {
    let mut property_collection = HashSet::new();
    let mut property_mapping = Vec::new();
    for property in &fluid.properties {
        property_collection.insert(property.name.clone());

        // Get mapped property `enum` name
        let renamed_property = property.name.to_upper_camel_case();

        let expected_values = enum_to_values
            .entry(renamed_property.clone())
            .or_insert_with(|| property.values.clone());

        assert_eq!(
            expected_values, &property.values,
            "Enum overlap for '{}' ({:?} vs {:?})",
            property.name, property.values, expected_values
        );

        property_mapping.push(PropertyVariantMapping {
            original_name: property.name.clone(),
            property_enum: renamed_property.clone(),
        });

        // If this property doesn't have an `enum` yet, make one.
        let _ = property_enums
            .entry(renamed_property.clone())
            .or_insert_with(|| PropertyStruct {
                name: renamed_property,
                values: property.values.clone(),
            });
    }

    if !property_collection.is_empty() {
        let mut property_collection = Vec::from_iter(property_collection);
        property_collection.sort();
        property_collection_map
            .entry(property_collection)
            .or_insert_with(|| PropertyCollectionData::from_mappings(property_mapping))
            .add_fluid_name(fluid.name.clone());
    }
}

/// The `PartialFluidState` values of the unique states.
pub(super) fn partial_state_tokens(
    unique_states: &[FluidState],
) -> impl Iterator<Item = TokenStream> + '_ {
    unique_states.iter().map(|state| {
        let height = state.height;
        let level = state.level;
        let is_empty = state.is_empty;
        let blast_resistance = state.blast_resistance;
        let block_state_id = state.block_state_id;
        let is_still = state.is_still;
        let is_source = is_still;
        let falling = false;
        quote! {
            PartialFluidState {
                height: #height,
                level: #level,
                is_empty: #is_empty,
                blast_resistance: #blast_resistance,
                block_state_id: #block_state_id,
                is_still: #is_still,
                is_source: #is_source,
                falling: #falling,
            }
        }
    })
}

/// The `from_state_id` arms, narrower ranges first.
pub(super) fn state_id_arm_tokens(mut state_id_arms: Vec<StateIdArm>) -> TokenStream {
    let mut fluid_from_state_id = TokenStream::new();
    // Narrower ranges first so still fluids match before their flowing variant
    state_id_arms.sort_by_key(|arm| arm.end - arm.start);
    for arm in &state_id_arms {
        let start = LitInt::new(&arm.start.to_string(), Span::call_site());
        let end = LitInt::new(&arm.end.to_string(), Span::call_site());
        let const_ident = format_ident!("{}", arm.const_name);
        fluid_from_state_id.extend(quote! {
            #start..=#end => Some(&Fluid::#const_ident),
        });
    }
    fluid_from_state_id
}

/// The per-group property lookups and property structs.
pub(super) fn property_group_tokens(
    property_collection_map: BTreeMap<Vec<String>, PropertyCollectionData>,
) -> (TokenStream, TokenStream, Vec<FluidPropertyStruct>) {
    let mut fluid_properties_from_state_and_name = TokenStream::new();
    let mut fluid_properties_from_props_and_name = TokenStream::new();
    let mut fluid_properties: Vec<FluidPropertyStruct> = Vec::new();

    for property_group in property_collection_map.into_values() {
        for fluid_name in &property_group.fluid_names {
            let const_fluid_name = Ident::new(
                &const_fluid_name_from_fluid_name(fluid_name),
                Span::call_site(),
            );
            let property_name = Ident::new(
                &property_group_name_from_derived_name(&property_group.derive_name()),
                Span::call_site(),
            );

            fluid_properties_from_state_and_name.extend(quote! {
                #fluid_name => Box::new(#property_name::from_state_id(id, &Fluid::#const_fluid_name)),
            });

            fluid_properties_from_props_and_name.extend(quote! {
                #fluid_name => Box::new(#property_name::from_props(props, &Fluid::#const_fluid_name)),
            });
        }

        fluid_properties.push(FluidPropertyStruct {
            data: property_group,
        });
    }

    (
        fluid_properties_from_state_and_name,
        fluid_properties_from_props_and_name,
        fluid_properties,
    )
}
