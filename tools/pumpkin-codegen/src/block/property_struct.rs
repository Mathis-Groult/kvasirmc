//! Pieces of the token output of one block property struct.

use super::{Block, BlockId, BlockPropertyStruct, BlockStateId, Property, PropertyType};
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

impl BlockPropertyStruct {
    /// The struct fields, one per property.
    pub(super) fn field_tokens(&self) -> impl Iterator<Item = TokenStream> + '_ {
        self.data.variant_mappings.iter().map(|entry| {
            let key = Ident::new_raw(&entry.original_name, Span::call_site());
            match &entry.property_type {
                PropertyType::Bool => quote! { pub #key: bool },
                PropertyType::Int { .. } => quote! { pub #key: u8 },
                PropertyType::Enum { name, .. } => {
                    let value = Ident::new(name, Span::call_site());
                    quote! { pub #key: #value }
                }
            }
        })
    }

    /// The field initialisers of `from_index`, outermost property first.
    pub(super) fn from_index_body(&self) -> Vec<TokenStream> {
        self.data
            .variant_mappings
            .iter()
            .rev()
            .map(|entry| {
                let field_name = Ident::new_raw(&entry.original_name, Span::call_site());
                match &entry.property_type {
                    PropertyType::Bool => quote! {
                        #field_name: {
                            let value = index % 2;
                            index /= 2;
                            value == 0
                        }
                    },
                    PropertyType::Int { min, max } => {
                        let count = (max - min + 1) as u16;
                        let val = if *min > 0 {
                            quote! { value + #min }
                        } else {
                            quote! {value}
                        };
                        quote! {
                            #field_name: {
                                let value = (index % #count) as u8;
                                index /= #count;
                                #val
                            }
                        }
                    }
                    PropertyType::Enum { name, .. } => {
                        let enum_ident = Ident::new(name, Span::call_site());
                        quote! {
                            #field_name: {
                                let value = index % #enum_ident::variant_count();
                                index /= #enum_ident::variant_count();
                                #enum_ident::from_index(value)
                            }
                        }
                    }
                }
            })
            .collect::<Vec<_>>()
    }

    /// The `Property` values of `to_props`.
    pub(super) fn to_props_entries(&self) -> impl Iterator<Item = TokenStream> + '_ {
        self.data.variant_mappings.iter().map(|entry| {
            let key_variant = Ident::new(
                &entry.original_name.to_upper_camel_case(),
                Span::call_site(),
            );
            let field = Ident::new_raw(&entry.original_name, Span::call_site());
            match &entry.property_type {
                PropertyType::Bool | PropertyType::Int { .. } => quote! {
                    Property::#key_variant(self.#field)
                },
                PropertyType::Enum {
                    name,
                    values,
                    prop_variant_name,
                    canonical_enum_name,
                    ..
                } => {
                    let prop_var_ident = Ident::new(prop_variant_name, Span::call_site());
                    if name == canonical_enum_name {
                        quote! { Property::#prop_var_ident(self.#field) }
                    } else {
                        let enum_ident = Ident::new(name, Span::call_site());
                        let canon_ident = Ident::new(canonical_enum_name, Span::call_site());
                        let val_idents: Vec<Ident> = values
                            .iter()
                            .map(|v| Ident::new(&v.to_upper_camel_case(), Span::call_site()))
                            .collect();
                        quote! {
                            Property::#prop_var_ident(match self.#field {
                                #(#enum_ident::#val_idents => #canon_ident::#val_idents),*
                            })
                        }
                    }
                }
            }
        })
    }

    /// The match arms of `from_props`.
    pub(super) fn from_props_arms(&self) -> Vec<TokenStream> {
        let mut from_props_arms = Vec::new();
        for entry in &self.data.variant_mappings {
            let key_variant = Ident::new(
                &entry.original_name.to_upper_camel_case(),
                Span::call_site(),
            );
            let field_name = Ident::new_raw(&entry.original_name, Span::call_site());
            match &entry.property_type {
                PropertyType::Bool => {
                    from_props_arms.push(quote! {
                        Property::#key_variant(value) => block_props.#field_name = *value,
                    });
                }
                PropertyType::Int { min, max } => {
                    if *min == 0 {
                        from_props_arms.push(quote! {
                            Property::#key_variant(value) => {
                                block_props.#field_name = if *value <= #max { *value } else { 0 };
                            }
                        });
                    } else {
                        from_props_arms.push(quote! {
                            Property::#key_variant(value) => {
                                block_props.#field_name = if *value >= #min && *value <= #max { *value } else { #min };
                            }
                        });
                    }
                }
                PropertyType::Enum {
                    name,
                    values,
                    prop_variant_name,
                    canonical_enum_name,
                    shared_none_variants,
                } => {
                    let prop_var_ident = Ident::new(prop_variant_name, Span::call_site());
                    let enum_ident = Ident::new(name, Span::call_site());
                    if name == canonical_enum_name {
                        from_props_arms.push(quote! {
                            Property::#prop_var_ident(value) => block_props.#field_name = *value,
                        });
                        for (other_prop_var, other_enum) in shared_none_variants {
                            let other_prop_var_ident =
                                Ident::new(other_prop_var, Span::call_site());
                            let other_enum_ident = Ident::new(other_enum, Span::call_site());
                            from_props_arms.push(quote! {
                                Property::#other_prop_var_ident(#other_enum_ident::None) => {
                                    block_props.#field_name = #enum_ident::None;
                                }
                            });
                        }
                    } else {
                        let canon_ident = Ident::new(canonical_enum_name, Span::call_site());
                        let val_idents: Vec<Ident> = values
                            .iter()
                            .map(|v| Ident::new(&v.to_upper_camel_case(), Span::call_site()))
                            .collect();
                        from_props_arms.push(quote! {
                            Property::#prop_var_ident(value) => match *value {
                                #(#canon_ident::#val_idents => block_props.#field_name = #enum_ident::#val_idents,)*
                                _ => {}
                            },
                        });
                    }
                }
            }
        }
        from_props_arms
    }
}

/// The `BlockProperties` implementation of one property struct.
pub(super) fn block_properties_impl(
    name: &Ident,
    struct_name: &str,
    to_index_logic: impl Iterator<Item = TokenStream>,
    block_ids: impl Iterator<Item = Ident>,
    to_props_entries: impl Iterator<Item = TokenStream>,
    from_props_arms: &[TokenStream],
) -> TokenStream {
    quote! {
    impl BlockProperties for #name {
       fn to_index(&self) -> u16 {
            let (index, _) = [#(#to_index_logic),*]
                .iter()
                .fold((0, 1), |(curr, mul), &(val, count)| (curr + val * mul, mul * count));
            index
        }

        #[allow(unused_assignments)]
        fn from_index(index: u16) -> Self {
            Self::from_index(index)
        }

        #[inline]
        #[allow(clippy::manual_range_patterns)]
        fn handles_block_id(block_id: BlockId) -> bool where Self: Sized {
            matches!(block_id, #(BlockId::#block_ids)|*)
        }

        fn to_state_id(&self, block: &Block) -> BlockStateId {
            if !Self::handles_block_id(block.id) {
                panic!("{} is not a valid block for {}", block.name, #struct_name);
            }
            block.states[self.to_index() as usize].id
        }

        fn from_state_id(id: BlockStateId, block: &Block) -> Self {
            debug_assert!(
                Self::handles_block_id(block.id),
                "{} is not a valid block for {}", block.name, #struct_name
            );

            let min_id = block.states[0].id.as_u16();
            let max_id = block.states.last().map(|s| s.id.as_u16()).unwrap_or(min_id);

            if (min_id..=max_id).contains(&id.as_u16()) {
                Self::from_index(id.as_u16() - min_id)
            } else {
                #[cfg(debug_assertions)]
                panic!("State ID {} does not exist for {}", id, block.name);

                #[cfg(not(debug_assertions))]
                Self::from_index(0)
            }
        }

        fn default(block: &Block) -> Self {
            if !Self::handles_block_id(block.id) {
                panic!("{} is not a valid block for {}", block.name, #struct_name);
            }
            Self::from_state_id(block.default_state.id)
        }

        fn to_props(&self) -> Vec<Property> {
           vec![ #(#to_props_entries),* ]
        }

        #[allow(clippy::manual_range_patterns, clippy::single_match)]
        fn from_props(props: &[Property], block: &Block) -> Self {
            #[cfg(debug_assertions)]
            if !Self::handles_block_id(block.id) {
                panic!("{} is not a valid block for {}", block.name, #struct_name);
            }
            let mut block_props = Self::default(block);
            for prop in props {
                match prop {
                    #(#from_props_arms)*
                    _ => {}
                }
            }
            block_props
        }
    }
    }
}
