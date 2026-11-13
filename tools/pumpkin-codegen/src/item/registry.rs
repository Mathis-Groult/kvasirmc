//! Reads the Java and Bedrock item tables and builds the item constants and the Java/Bedrock mappings.

use super::{
    BedrockArmorType, BedrockItem, BedrockItemVersion, BedrockRemapEntry, BedrockRuntimeItemState,
    BedrockToolType, Item,
};
use crate::block::{Block, BlockAssets};
use crate::enchantments::AttributeModifierSlot;
use heck::{ToPascalCase, ToShoutySnakeCase};
use proc_macro2::{Span, TokenStream};
use pumpkin_nbt::{Nbt, compound::NbtCompound};
use pumpkin_util::registry::TagType;
use pumpkin_util::text::TextContent;
use pumpkin_util::{registry::RegistryEntryList, text::TextComponent};
use quote::{ToTokens, format_ident, quote};
use serde::Deserialize;
use serde_repr::Deserialize_repr;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::Cursor,
};
use syn::{Ident, LitBool, LitByteStr, LitFloat, LitInt, LitStr};

/// Reads the Bedrock item component table (gzip NBT), keyed by item name.
pub(super) fn load_bedrock_item_components() -> BTreeMap<String, Option<NbtCompound>> {
    let data = fs::read("../../assets/bedrock/item_components.nbt").unwrap();
    let mut cursor = Cursor::new(data);
    let nbt = pumpkin_nbt::nbt_compress::read_gzip_compound_tag(&mut cursor)
        .expect("Failed to parse bedrock/item_components.nbt");
    nbt.child_tags
        .into_iter()
        .map(|(k, v)| {
            let comp = match v {
                pumpkin_nbt::tag::NbtTag::Compound(c) => Some(c),
                _ => None,
            };
            (k.into_string(), comp)
        })
        .collect()
}

/// The Java-to-Bedrock remap of every Java item, in Java name order.
pub(super) fn java_to_bedrock_remaps<'a>(
    items: &'a BTreeMap<String, Item>,
    blocks_assets: &BlockAssets,
    be_valid_item_identifiers: &HashSet<&String>,
    be_item_data_overrides: &BTreeMap<String, u32>,
) -> indexmap::IndexMap<&'a String, BedrockRemapEntry> {
    let mut be_item_remaps = indexmap::IndexMap::new();

    let mut item_to_block: BTreeMap<u16, &Block> = BTreeMap::new();
    for block in &blocks_assets.blocks {
        if block.item_id != 0 {
            item_to_block.entry(block.item_id).or_insert(block);
        }
    }

    for (name, item) in items {
        let be_identifier = bedrock_identifier(name, be_valid_item_identifiers);
        be_item_remaps.insert(
            name,
            remap_entry(
                name,
                item,
                be_identifier,
                &item_to_block,
                be_item_data_overrides,
            ),
        );
    }
    be_item_remaps
}

/// The Bedrock identifier of a Java item (renames, then `minecraft:unknown` when Bedrock has none).
fn bedrock_identifier(name: &str, be_valid_item_identifiers: &HashSet<&String>) -> String {
    // THIS COERCES CODEGEN FOR JAVA 26.3
    // This is for coercing the codegen into working for java 26.3 temporarily.
    /// Remove `let mut be_identifier =` and replace with `let be_identifier =` to undo this.
    let mut be_identifier = "minecraft:".to_owned()
        + &match name {
            "bricks" => "brick_block".into(),
            "cobblestone_stairs" => "stone_stairs".into(),
            "cobweb" => "web".into(),
            "dead_bush" => "deadbush".into(),
            "debug_stick" => "stick".into(),
            "dirt_path" => "grass_path".into(),
            "end_stone_bricks" => "end_bricks".into(),
            "end_stone_brick_stairs" => "end_brick_stairs".into(),
            "flowering_azalea_leaves" => "azalea_leaves_flowered".into(),
            "frogspawn" => "frog_spawn".into(),
            "furnace_minecart" => "hopper_minecart".into(),
            "glow_item_frame" => "glow_frame".into(),
            "item_frame" => "frame".into(),
            "jack_o_lantern" => "lit_pumpkin".into(),
            "light" => "light_block".into(),
            "light_gray_glazed_terracotta" => "silver_glazed_terracotta".into(),
            "lily_pad" => "waterlily".into(),
            "knowledge_book" => "book".into(),
            "magma_block" => "magma".into(),
            "map" => "empty_map".into(),
            "melon" => "melon_block".into(),
            "music_disc_bounce" => "music_disc_13".into(),
            "nether_brick" => "netherbrick".into(),
            "nether_bricks" => "nether_brick".into(),
            "nether_quartz_ore" => "quartz_ore".into(),
            "note_block" => "noteblock".into(),
            "oak_button" => "wooden_button".into(),
            "oak_door" => "wooden_door".into(),
            "oak_fence_gate" => "fence_gate".into(),
            "oak_pressure_plate" => "wooden_pressure_plate".into(),
            "oak_trapdoor" => "trapdoor".into(),
            "powered_rail" => "golden_rail".into(),
            "prismarine_brick_stairs" => "prismarine_bricks_stairs".into(),
            "red_nether_bricks" => "red_nether_brick".into(),
            "rooted_dirt" => "dirt_with_roots".into(),
            "scute" => "turtle_scute".into(),
            "shulker_box" => "undyed_shulker_box".into(),
            "slime_block" => "slime".into(),
            "small_dripleaf" => "small_dripleaf_block".into(),
            "snow" => "snow_layer".into(),
            "snow_block" => "snow".into(),
            "spawner" => "mob_spawner".into(),
            "stonecutter" => "stonecutter_block".into(),
            "stone_stairs" => "normal_stone_stairs".into(),
            "terracotta" => "hardened_clay".into(),
            "test_block" => "unknown".into(),
            "test_instance_block" => "unknown".into(),
            "tipped_arrow" | "spectral_arrow" => "arrow".into(),
            "waxed_copper_block" => "waxed_copper".into(),
            "zombified_piglin_spawn_egg" => "zombie_pigman_spawn_egg".into(),
            n if n.contains("sulfur") => "unknown".into(),
            n => {
                if n.ends_with("banner") {
                    "banner".into()
                } else if n.ends_with("bed") {
                    "bed".into()
                } else if n.starts_with("stone_slab") {
                    n.replace("stone_slab", "stone_block_slab")
                } else if n.starts_with("double_stone_slab") {
                    n.replace("double_stone_slab", "stone_block_slab")
                } else if n.ends_with("_map") {
                    let candidate = format!("minecraft:{n}");
                    if be_valid_item_identifiers.contains(&candidate) {
                        n.into()
                    } else {
                        "filled_map".into()
                    }
                } else {
                    n.into()
                }
            }
        };

    // THIS COERCES CODEGEN FOR JAVA 26.3
    // This is for coercing the codegen into working for java 26.3 temporarily.
    /// Remove the if statement and replace with the assert statement below.
    /// replace with:
    /// assert!(
    ///     be_valid_item_identifiers.contains(&be_identifier),
    ///     "Invalid Bedrock identifier `{be_identifier}`. From Java name `{name}`"
    /// );
    if !be_valid_item_identifiers.contains(&be_identifier) {
        be_identifier = "minecraft:unknown".into();
    }
    be_identifier
}

/// The Bedrock remap entry of one Java item.
fn remap_entry(
    name: &str,
    item: &Item,
    be_identifier: String,
    item_to_block: &BTreeMap<u16, &Block>,
    be_item_data_overrides: &BTreeMap<String, u32>,
) -> BedrockRemapEntry {
    let block = item_to_block.get(&item.id);

    let type_suffix = name
        .split(':')
        .next_back()
        .unwrap()
        .split('_')
        .next_back()
        .unwrap();

    BedrockRemapEntry {
        identifier: be_identifier,
        data: *be_item_data_overrides.get(name).unwrap_or(&0),
        block_runtime_id_range: block.and_then(|b| {
            let min = b.states.first();
            let max = b.states.last();

            if let (Some(min), Some(max)) = (min, max) {
                Some((min.id.0 as i32, max.id.0 as i32))
            } else {
                None
            }
        }),
        tool_type: match type_suffix {
            "sword" => Some(BedrockToolType::Sword),
            "shovel" => Some(BedrockToolType::Shovel),
            "pickaxe" => Some(BedrockToolType::Pickaxe),
            "axe" => Some(BedrockToolType::Axe),
            "shears" => Some(BedrockToolType::Shears),
            "hoe" => Some(BedrockToolType::Hoe),
            _ => None,
        },
        armor_type: match type_suffix {
            "helmet" => Some(BedrockArmorType::Helmet),
            "chestplate" => Some(BedrockArmorType::Chestplate),
            "leggings" => Some(BedrockArmorType::Leggings),
            "boots" => Some(BedrockArmorType::Boots),
            _ => None,
        },
        protection_value: if let (Some(_), Some(_)) = (
            &item.components.equippable,
            &item.components.attribute_modifiers,
        ) {
            Some(1) // TODO: calculate correct value
        } else {
            None
        },
        is_entity_placer: item.components.entity_data.is_some()
            || matches!(
                name,
                "firework_rocket"
                    | "minecart"
                    | "tnt_minecart"
                    | "chest_minecart"
                    | "hopper_minecart"
                    | "furnace_minecart"
                    | "command_block_minecart"
                    | "oak_boat"
                    | "oak_chest_boat"
                    | "spruce_boat"
                    | "spruce_chest_boat"
                    | "birch_boat"
                    | "birch_chest_boat"
                    | "jungle_boat"
                    | "jungle_chest_boat"
                    | "acacia_boat"
                    | "acacia_chest_boat"
                    | "dark_oak_boat"
                    | "dark_oak_chest_boat"
                    | "mangrove_boat"
                    | "mangrove_chest_boat"
                    | "cherry_boat"
                    | "cherry_chest_boat"
                    | "pale_oak_boat"
                    | "pale_oak_chest_boat"
                    | "bamboo_raft"
                    | "bamboo_chest_raft"
            ),
        is_edible: item.components.food.is_some(),
    }
}

/// The Bedrock item definitions, with their component NBT when known.
pub(super) fn bedrock_item_definitions(
    be_runtime_item_states: Vec<BedrockRuntimeItemState>,
    be_item_components: &BTreeMap<String, Option<NbtCompound>>,
) -> Vec<BedrockItem> {
    let mut bedrock_item_definitions = Vec::new();
    for item in be_runtime_item_states {
        let components = be_item_components.get(&item.name).cloned().flatten();

        bedrock_item_definitions.push(BedrockItem {
            id: item.id,
            registry_key: item.name.clone(),
            version: item.version,
            component_based: item.component_based,
            components: if let Some(c) = components {
                Nbt::new("".to_string(), c.clone())
            } else {
                if item.component_based {
                    eprintln!(
                        "Couldn't find the components for a component based item: {}.",
                        item.name
                    );
                }
                Nbt::new("".to_string(), NbtCompound::new())
            },
        });
    }
    bedrock_item_definitions
}

/// The Java item constants, lookup arms and id lists.
pub(super) struct JavaItemTokens {
    pub(super) constants: TokenStream,
    pub(super) type_from_raw_id_arms: TokenStream,
    pub(super) type_from_name: TokenStream,
    pub(super) all_java_items: Vec<Ident>,
    pub(super) all_java_item_ids: Vec<LitInt>,
}

/// Builds the Java item constants and lookups.
pub(super) fn java_item_tokens(items: &BTreeMap<String, Item>) -> JavaItemTokens {
    let mut type_from_raw_id_arms = TokenStream::new();
    let mut type_from_name = TokenStream::new();

    let mut constants = TokenStream::new();

    let mut all_java_items = Vec::new();
    let mut all_java_item_ids = Vec::new();

    for (name, item) in items {
        let const_ident = format_ident!("{}", name.to_shouty_snake_case());

        let components = &item.components;
        let components_tokens = components.to_token_stream();

        let id_lit = LitInt::new(&item.id.to_string(), Span::call_site());

        constants.extend(quote! {
            pub const #const_ident: Self = Self {
                id: #id_lit,
                registry_key: #name,
                components: &[#components_tokens],
            };
        });

        type_from_raw_id_arms.extend(quote! {
            #id_lit => Some(&Self::#const_ident),
        });

        type_from_name.extend(quote! {
            #name => Some(&Self::#const_ident),
        });

        all_java_items.push(const_ident);
        all_java_item_ids.push(id_lit);
    }
    JavaItemTokens {
        constants,
        type_from_raw_id_arms,
        type_from_name,
        all_java_items,
        all_java_item_ids,
    }
}

/// The Bedrock item constants and their identifiers, in definition order.
pub(super) fn bedrock_item_constants(
    bedrock_item_definitions: Vec<BedrockItem>,
) -> (TokenStream, Vec<Ident>) {
    let mut bedrock_constants = TokenStream::new();
    let mut all_bedrock_items = Vec::new();

    for item in bedrock_item_definitions {
        let const_ident = format_ident!(
            "{}",
            item.registry_key
                .split(':')
                .next_back()
                .unwrap()
                .to_shouty_snake_case()
        );

        let id = LitInt::new(&item.id.to_string(), Span::call_site());
        let registry_key = item.registry_key;
        let version = format_ident!(
            "{}",
            match item.version {
                BedrockItemVersion::Legacy => "Legacy",
                BedrockItemVersion::DataDriven => "DataDriven",
                BedrockItemVersion::None => "None",
            }
        );
        let component_based = item.component_based;

        let components_bytes_lit =
            LitByteStr::new(&item.components.write_bedrock(), Span::call_site());

        bedrock_constants.extend(quote! {
            pub const #const_ident: Self = Self {
                id: #id,
                registry_key: #registry_key,
                version: BedrockItemVersion::#version,
                component_based: #component_based,
                definition_components: #components_bytes_lit
            };
        });

        all_bedrock_items.push(const_ident);
    }
    (bedrock_constants, all_bedrock_items)
}

/// The Java-to-Bedrock mapping constants.
pub(super) fn mapping_constants(
    be_item_remaps: &indexmap::IndexMap<&String, BedrockRemapEntry>,
) -> TokenStream {
    let mut mapping_constants = TokenStream::new();

    for (java_name, mapping) in be_item_remaps {
        let java_const_ident = format_ident!("{}", java_name.to_shouty_snake_case());
        let bedrock_const_ident = format_ident!(
            "{}",
            mapping
                .identifier
                .split(':')
                .next_back()
                .unwrap()
                .to_shouty_snake_case()
        );

        let data_lit = LitInt::new(&mapping.data.to_string(), Span::call_site());

        mapping_constants.extend(quote! {
            pub const #java_const_ident: Self = Self {
                java_item: &Item::#java_const_ident,

                bedrock_item: &BedrockItem::#bedrock_const_ident,
                bedrock_data: #data_lit,
                bedrock_block_state: 0
            };
        });
    }
    mapping_constants
}

/// The Bedrock-to-Java lookup arms: by (id, data) pair, and the by-id fallback.
pub(super) fn bedrock_to_java_arms(
    be_item_remaps: &indexmap::IndexMap<&String, BedrockRemapEntry>,
    bedrock_id_by_key: &std::collections::HashMap<String, i16>,
) -> (TokenStream, TokenStream) {
    let mut bedrock_to_java_arms = TokenStream::new();
    let mut seen_bedrock_pairs = std::collections::HashSet::new();

    let mut bedrock_id_fallback_arms = TokenStream::new();
    let mut seen_bedrock_ids = std::collections::HashSet::new();

    for (java_name, mapping) in be_item_remaps {
        let java_const_ident = format_ident!("{}", java_name.to_shouty_snake_case());
        let bedrock_id = *bedrock_id_by_key
            .get(&mapping.identifier)
            .unwrap_or_else(|| {
                panic!(
                    "Missing bedrock item for identifier: {}",
                    mapping.identifier
                )
            });
        let bedrock_data = mapping.data;

        if seen_bedrock_pairs.insert((bedrock_id, bedrock_data)) {
            let id_lit = syn::LitInt::new(&bedrock_id.to_string(), proc_macro2::Span::call_site());
            let data_lit =
                syn::LitInt::new(&bedrock_data.to_string(), proc_macro2::Span::call_site());
            bedrock_to_java_arms.extend(quote! {
                (#id_lit, #data_lit) => Some(&Self::#java_const_ident),
            });
        }

        if seen_bedrock_ids.insert(bedrock_id) {
            let id_lit = syn::LitInt::new(&bedrock_id.to_string(), proc_macro2::Span::call_site());
            bedrock_id_fallback_arms.extend(quote! {
                #id_lit => Some(&Self::#java_const_ident),
            });
        }
    }
    (bedrock_to_java_arms, bedrock_id_fallback_arms)
}
