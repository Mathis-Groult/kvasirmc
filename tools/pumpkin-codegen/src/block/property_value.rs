//! The `Property` enum arms and enum metadata, gathered per property key.

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

/// How one enum property is named inside `Property`, and which other enums share its `None`.
pub(super) struct EnumPropMeta {
    pub(super) prop_variant_name: String,
    pub(super) canonical_enum_name: String,
    pub(super) shared_none_variants: Vec<(String, String)>,
}

/// The `Property` enum arms gathered per property key, and the enum metadata by property hash.
#[derive(Default)]
pub(super) struct PropertyValueArms {
    pub(super) enum_prop_meta: BTreeMap<i32, EnumPropMeta>,
    pub(super) property_enum_variants: Vec<TokenStream>,
    pub(super) property_key_arms: Vec<TokenStream>,
    pub(super) property_value_str_arms: Vec<TokenStream>,
    pub(super) property_from_key_and_int_arms: Vec<TokenStream>,
    pub(super) property_from_key_value_str_arms: Vec<TokenStream>,
    pub(super) property_matches_shared_none_arms: Vec<TokenStream>,
}

/// Groups the generated properties by key and builds the `Property` enum arms.
pub(super) fn property_value_arms(generated_properties: &[GeneratedProperty]) -> PropertyValueArms {
    let mut props_by_key: BTreeMap<String, Vec<&GeneratedProperty>> = BTreeMap::new();
    for prop in generated_properties {
        props_by_key
            .entry(prop.serialized_name.clone())
            .or_default()
            .push(prop);
    }

    let mut arms = PropertyValueArms::default();
    for (key_name, group) in &props_by_key {
        push_property_key(key_name, group, &mut arms);
    }
    arms
}

/// Records the enum metadata of one property key and pushes its `Property` arms.
fn push_property_key(key_name: &str, group: &[&GeneratedProperty], arms: &mut PropertyValueArms) {
    let PropertyValueArms {
        enum_prop_meta,
        property_matches_shared_none_arms,
        ..
    } = arms;
    let key_ident = Ident::new(&key_name.to_upper_camel_case(), Span::call_site());
    let has_bool = group
        .iter()
        .any(|p| matches!(p.property_type, GeneratedPropertyType::Boolean));
    let has_int = group
        .iter()
        .any(|p| matches!(p.property_type, GeneratedPropertyType::Int { .. }));

    let enum_entries: Vec<(&GeneratedProperty, String, &[String])> = group
        .iter()
        .filter_map(|p| match &p.property_type {
            GeneratedPropertyType::Enum { values } => {
                Some((*p, p.enum_name.to_upper_camel_case(), values.as_slice()))
            }
            _ => None,
        })
        .collect();

    let mut canonical_for_entry: Vec<(String, &[String])> = Vec::with_capacity(enum_entries.len());
    let mut unique_canonical_enums: BTreeMap<String, &[String]> = BTreeMap::new();
    for (_, enum_camel, values) in &enum_entries {
        let canon_name = enum_entries
            .iter()
            .find(|(_, _, other_vals)| {
                other_vals.len() > values.len() && values.iter().all(|v| other_vals.contains(v))
            })
            .map_or_else(
                || enum_camel.clone(),
                |(_, other_camel, _)| other_camel.clone(),
            );
        let canon_vals_slice = enum_entries
            .iter()
            .find(|(_, c, _)| *c == canon_name)
            .map(|(_, _, v)| *v)
            .unwrap();
        unique_canonical_enums.insert(canon_name.clone(), canon_vals_slice);
        canonical_for_entry.push((canon_name, canon_vals_slice));
    }

    let total_distinct_types =
        usize::from(has_bool) + usize::from(has_int) + unique_canonical_enums.len();

    let mut canon_with_none: Vec<(String, String)> = Vec::new();
    for (canon_name, canon_vals) in &unique_canonical_enums {
        let prop_var = if total_distinct_types == 1 {
            key_name.to_upper_camel_case()
        } else {
            canon_name.clone()
        };
        if canon_vals.iter().any(|v| v == "none") {
            canon_with_none.push((prop_var, canon_name.clone()));
        }
    }

    for ((p, _, _), (canon_name, _)) in enum_entries.iter().zip(canonical_for_entry.iter()) {
        let prop_variant_name = if total_distinct_types == 1 {
            key_name.to_upper_camel_case()
        } else {
            canon_name.clone()
        };
        let shared_none_variants: Vec<(String, String)> = canon_with_none
            .iter()
            .filter(|(_, other_canon)| other_canon != canon_name)
            .cloned()
            .collect();
        enum_prop_meta.insert(
            p.hash_key,
            EnumPropMeta {
                prop_variant_name,
                canonical_enum_name: canon_name.clone(),
                shared_none_variants,
            },
        );
    }

    if canon_with_none.len() > 1 {
        push_shared_none_arms(property_matches_shared_none_arms, &canon_with_none);
    }

    push_value_arms(
        arms,
        key_name,
        &key_ident,
        has_bool,
        has_int,
        &unique_canonical_enums,
        total_distinct_types,
    );
}

/// The `matches_property` arms for every pair of distinct enums that both have `None`.
fn push_shared_none_arms(
    property_matches_shared_none_arms: &mut Vec<TokenStream>,
    canon_with_none: &[(String, String)],
) {
    for i in 0..canon_with_none.len() {
        for j in 0..canon_with_none.len() {
            if i != j {
                let (pv_a, ce_a) = &canon_with_none[i];
                let (pv_b, ce_b) = &canon_with_none[j];
                let pv_a_ident = Ident::new(pv_a, Span::call_site());
                let ce_a_ident = Ident::new(ce_a, Span::call_site());
                let pv_b_ident = Ident::new(pv_b, Span::call_site());
                let ce_b_ident = Ident::new(ce_b, Span::call_site());
                property_matches_shared_none_arms.push(quote! {
                    (Self::#pv_a_ident(#ce_a_ident::None), Self::#pv_b_ident(#ce_b_ident::None)) => true,
                });
            }
        }
    }
}

/// The variant, key, value-string and parse arms of one property key.
fn push_value_arms(
    arms: &mut PropertyValueArms,
    key_name: &str,
    key_ident: &Ident,
    has_bool: bool,
    has_int: bool,
    unique_canonical_enums: &BTreeMap<String, &[String]>,
    total_distinct_types: usize,
) {
    let PropertyValueArms {
        property_enum_variants,
        property_key_arms,
        property_value_str_arms,
        property_from_key_and_int_arms,
        property_from_key_value_str_arms,
        ..
    } = arms;
    if has_bool {
        property_enum_variants.push(quote! { #key_ident(bool) });
        property_key_arms.push(quote! { Self::#key_ident(_) => PropertyKey::#key_ident, });
        property_value_str_arms.push(quote! {
            Self::#key_ident(v) => if v { "true" } else { "false" },
        });
    }
    if has_int {
        property_enum_variants.push(quote! { #key_ident(u8) });
        property_key_arms.push(quote! { Self::#key_ident(_) => PropertyKey::#key_ident, });
        property_value_str_arms.push(quote! {
            Self::#key_ident(v) => if (v as usize) < INT_STRS.len() { INT_STRS[v as usize] } else { "0" },
        });
        property_from_key_and_int_arms.push(quote! {
            PropertyKey::#key_ident => Some(Self::#key_ident(value)),
        });
    }
    for (canon_name, _) in unique_canonical_enums {
        let prop_var = if total_distinct_types == 1 {
            key_name.to_upper_camel_case()
        } else {
            canon_name.clone()
        };
        let var_ident = Ident::new(&prop_var, Span::call_site());
        let canon_ident = Ident::new(canon_name, Span::call_site());
        property_enum_variants.push(quote! { #var_ident(#canon_ident) });
        property_key_arms.push(quote! { Self::#var_ident(_) => PropertyKey::#key_ident, });
        property_value_str_arms.push(quote! { Self::#var_ident(v) => v.to_value(), });
    }

    if has_int {
        property_from_key_value_str_arms.push(quote! {
            PropertyKey::#key_ident => value.parse::<u8>().ok().map(Self::#key_ident),
        });
    } else {
        let mut val_arms = Vec::new();
        let mut seen_vals = HashSet::new();
        if has_bool {
            seen_vals.insert("true");
            seen_vals.insert("false");
            val_arms.push(quote! { "true" => Some(Self::#key_ident(true)), });
            val_arms.push(quote! { "false" => Some(Self::#key_ident(false)), });
        }
        for (canon_name, canon_vals) in unique_canonical_enums {
            let prop_var = if total_distinct_types == 1 {
                key_name.to_upper_camel_case()
            } else {
                canon_name.clone()
            };
            let var_ident = Ident::new(&prop_var, Span::call_site());
            let canon_ident = Ident::new(canon_name, Span::call_site());
            for val_str in *canon_vals {
                if seen_vals.insert(val_str.as_str()) {
                    let val_ident = Ident::new(&val_str.to_upper_camel_case(), Span::call_site());
                    val_arms.push(quote! {
                        #val_str => Some(Self::#var_ident(#canon_ident::#val_ident)),
                    });
                }
            }
        }
        property_from_key_value_str_arms.push(quote! {
            PropertyKey::#key_ident => match value {
                #(#val_arms)*
                _ => None,
            },
        });
    }
}
