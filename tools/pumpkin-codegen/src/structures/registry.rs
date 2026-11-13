//! Reads the vanilla structures and structure sets and builds their constants and lookups.

use super::{StructureSetStruct, StructureStruct, structure_key_to_token};
use heck::ToPascalCase;
use proc_macro2::{Ident, TokenStream};
use quote::{ToTokens, format_ident, quote};
use serde::Deserialize;
use std::{collections::BTreeMap, fs};

/// Reads every vanilla structure JSON, keyed by file stem.
pub(super) fn load_structures() -> BTreeMap<String, StructureStruct> {
    let structures_dir =
        std::path::Path::new("../../assets/datapack/data/minecraft/worldgen/structure");
    let mut structures_json: BTreeMap<String, StructureStruct> = BTreeMap::new();
    let mut s_entries: Vec<_> = fs::read_dir(structures_dir)
        .expect("Missing worldgen/structure directory")
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();
    s_entries.sort_by_key(|e| e.path());
    for entry in s_entries {
        let path = entry.path();
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let content = fs::read_to_string(&path).expect("Failed to read structure file");
        let structure: StructureStruct =
            serde_json::from_str(&content).expect("Failed to parse structure JSON");
        structures_json.insert(stem, structure);
    }
    structures_json
}

/// Reads every vanilla structure set JSON, keyed by file stem.
pub(super) fn load_structure_sets() -> BTreeMap<String, StructureSetStruct> {
    let structure_sets_dir =
        std::path::Path::new("../../assets/datapack/data/minecraft/worldgen/structure_set");
    let mut structure_sets_json: BTreeMap<String, StructureSetStruct> = BTreeMap::new();
    let mut ss_entries: Vec<_> = fs::read_dir(structure_sets_dir)
        .expect("Missing worldgen/structure_set directory")
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();
    ss_entries.sort_by_key(|e| e.path());
    for entry in ss_entries {
        let path = entry.path();
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let content = fs::read_to_string(&path).expect("Failed to read structure_set file");
        let set: StructureSetStruct =
            serde_json::from_str(&content).expect("Failed to parse structure_set JSON");
        structure_sets_json.insert(stem, set);
    }
    structure_sets_json
}

/// The structure constants, lookups and name lists.
pub(super) struct StructureTokens {
    pub(super) structure_const_defs: TokenStream,
    pub(super) structure_lookup_arms: TokenStream,
    pub(super) structure_from_name_arms: Vec<TokenStream>,
    pub(super) structure_to_name_arms: Vec<TokenStream>,
    pub(super) structure_all_names_tokens: Vec<TokenStream>,
}

/// Builds the per-structure constants and lookup arms.
pub(super) fn structure_lists(
    structures_json: &BTreeMap<String, StructureStruct>,
) -> StructureTokens {
    let mut structure_const_defs = TokenStream::new();
    let mut structure_lookup_arms = TokenStream::new();
    let mut structure_from_name_arms = Vec::new();
    let mut structure_to_name_arms = Vec::new();
    let mut structure_all_names: Vec<String> = Vec::new();

    for (name, structure) in structures_json {
        let stripped_name = name.strip_prefix("minecraft:").unwrap_or(name);
        let upper_name = stripped_name.to_uppercase();
        let const_name = format_ident!("{}", upper_name);
        let variant_ident = format_ident!("{}", stripped_name.to_pascal_case());
        let key_variant = structure_key_to_token(name);

        structure_const_defs.extend(quote!(
            pub const #const_name: Self = #structure;
        ));

        structure_lookup_arms.extend(quote!(
            #key_variant => &Self::#const_name,
        ));

        structure_from_name_arms.push(quote! {
            #stripped_name => Some(Self::#variant_ident),
        });
        structure_to_name_arms.push(quote! {
            Self::#variant_ident => #name,
        });
        structure_all_names.push(stripped_name.to_string());
    }

    let structure_all_names_tokens: Vec<TokenStream> = structure_all_names
        .iter()
        .map(|name| {
            let full = format!("minecraft:{name}");
            quote! { #full }
        })
        .collect();
    StructureTokens {
        structure_const_defs,
        structure_lookup_arms,
        structure_from_name_arms,
        structure_to_name_arms,
        structure_all_names_tokens,
    }
}

/// The structure set constants, lookups and name lists.
pub(super) struct StructureSetTokens {
    pub(super) structure_set_const_defs: TokenStream,
    pub(super) structure_set_lookup_arms: TokenStream,
    pub(super) all_structure_set_idents: Vec<Ident>,
    pub(super) all_structure_set_names: Vec<String>,
}

/// Builds the per-structure-set constants and lookup arms.
pub(super) fn structure_set_lists(
    structure_sets_json: &BTreeMap<String, StructureSetStruct>,
) -> StructureSetTokens {
    let mut structure_set_const_defs = TokenStream::new();
    let mut structure_set_lookup_arms = TokenStream::new();
    let mut all_structure_set_idents = Vec::new();
    let mut all_structure_set_names = Vec::new();

    for (name, structure_set) in structure_sets_json {
        let stripped_name = name.strip_prefix("minecraft:").unwrap_or(name);
        let upper_name = stripped_name.to_uppercase().replace('/', "_");
        let const_name = format_ident!("{}", upper_name);

        structure_set_const_defs.extend(quote!(
            pub const #const_name: Self = #structure_set;
        ));

        structure_set_lookup_arms.extend(quote!(
            #stripped_name => Some(&Self::#const_name),
        ));

        all_structure_set_idents.push(const_name);
        all_structure_set_names.push(stripped_name.to_string());
    }
    StructureSetTokens {
        structure_set_const_defs,
        structure_set_lookup_arms,
        all_structure_set_idents,
        all_structure_set_names,
    }
}
