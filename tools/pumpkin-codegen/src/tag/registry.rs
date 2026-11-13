//! Loads the registry id maps and walks the datapack tags of every version.

use super::{load_datapack_registry_ids, load_datapack_tags};
use crate::block::BlockAssets;
use crate::entity_type::EntityType;
use crate::fluid::Fluid;
use crate::item::Item;
use heck::ToPascalCase;
use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
};

/// Name -> numeric id of every static registry the tags can point into.
pub(super) struct IdMaps {
    block_id_map: BTreeMap<String, u16>,
    item_id_map: BTreeMap<String, u16>,
    fluid_id_map: BTreeMap<String, u16>,
    entity_id_map: BTreeMap<String, u16>,
    game_event_id_map: BTreeMap<String, u16>,
    potion_id_map: BTreeMap<String, u16>,
    poi_id_map: BTreeMap<String, u16>,
}

/// Loads the id maps of the static registries (blocks, items, fluids, entities, game events, potions, points of interest).
pub(super) fn load_id_maps() -> IdMaps {
    let blocks_assets: BlockAssets =
        serde_json::from_str(&fs::read_to_string("../../assets/blocks.json").unwrap())
            .expect("Failed to parse blocks.json");
    let mut block_id_map: BTreeMap<String, u16> = BTreeMap::new();
    for b in &blocks_assets.blocks {
        block_id_map.insert(b.name.clone(), b.id.0);
        block_id_map.insert(format!("minecraft:{}", b.name), b.id.0);
    }

    let items: BTreeMap<String, Item> =
        serde_json::from_str(&fs::read_to_string("../../assets/items.json").unwrap())
            .expect("Failed to parse items.json");
    let mut item_id_map: BTreeMap<String, u16> = BTreeMap::new();
    for (name, item) in &items {
        item_id_map.insert(name.clone(), item.id);
        item_id_map.insert(format!("minecraft:{name}"), item.id);
    }

    let fluids: Vec<Fluid> =
        serde_json::from_str(&fs::read_to_string("../../assets/fluids.json").unwrap())
            .expect("Failed to parse fluids.json");
    let mut fluid_id_map: BTreeMap<String, u16> = BTreeMap::new();
    for f in &fluids {
        fluid_id_map.insert(f.name.clone(), f.id);
        fluid_id_map.insert(format!("minecraft:{}", f.name), f.id);
    }

    let entities: BTreeMap<String, EntityType> =
        serde_json::from_str(&fs::read_to_string("../../assets/entities.json").unwrap())
            .expect("Failed to parse entities.json");
    let mut entity_id_map: BTreeMap<String, u16> = BTreeMap::new();
    for (name, entity) in &entities {
        entity_id_map.insert(name.clone(), entity.id);
        entity_id_map.insert(format!("minecraft:{name}"), entity.id);
    }

    let game_events: Vec<crate::game_event::GameEvent> =
        serde_json::from_str(&fs::read_to_string("../../assets/game_event.json").unwrap())
            .expect("Failed to parse game_event.json");
    let mut game_event_id_map: BTreeMap<String, u16> = BTreeMap::new();
    for (i, event) in game_events.iter().enumerate() {
        let name = &event.name;
        game_event_id_map.insert(name.clone(), i as u16);
        game_event_id_map.insert(format!("minecraft:{name}"), i as u16);
    }

    #[derive(serde::Deserialize)]
    struct PotionEntry {
        id: u8,
    }
    let potions: BTreeMap<String, PotionEntry> =
        serde_json::from_str(&fs::read_to_string("../../assets/potion.json").unwrap())
            .expect("Failed to parse potion.json");
    let mut potion_id_map: BTreeMap<String, u16> = BTreeMap::new();
    for (name, potion) in &potions {
        potion_id_map.insert(name.clone(), u16::from(potion.id));
        potion_id_map.insert(format!("minecraft:{name}"), u16::from(potion.id));
    }

    const POI_TYPES: &[&str] = &[
        "armorer",
        "butcher",
        "cartographer",
        "cleric",
        "farmer",
        "fisherman",
        "fletcher",
        "leatherworker",
        "librarian",
        "mason",
        "shepherd",
        "toolsmith",
        "weaponsmith",
        "home",
        "meeting",
        "beehive",
        "bee_nest",
        "nether_portal",
        "lodestone",
        "lightning_rod",
        "trial_spawner",
        "vault",
    ];
    let mut poi_id_map: BTreeMap<String, u16> = BTreeMap::new();
    for (i, &name) in POI_TYPES.iter().enumerate() {
        poi_id_map.insert(name.to_string(), i as u16);
        poi_id_map.insert(format!("minecraft:{name}"), i as u16);
    }
    IdMaps {
        block_id_map,
        item_id_map,
        fluid_id_map,
        entity_id_map,
        game_event_id_map,
        potion_id_map,
        poi_id_map,
    }
}

impl IdMaps {
    /// The ids of a tag's values in the registry named by `key`.
    fn ids(
        &self,
        key: &str,
        values: &[String],
        datapack_id_maps: &BTreeMap<String, BTreeMap<String, u16>>,
    ) -> Vec<u16> {
        let IdMaps {
            block_id_map,
            item_id_map,
            fluid_id_map,
            entity_id_map,
            game_event_id_map,
            potion_id_map,
            poi_id_map,
        } = self;
        values
            .iter()
            .filter_map(|v| match key {
                "block" => block_id_map.get(v).copied(),
                "item" => item_id_map.get(v).copied(),
                "fluid" => fluid_id_map.get(v).copied(),
                "entity_type" => entity_id_map.get(v).copied(),
                "game_event" => game_event_id_map.get(v).copied(),
                "potion" => potion_id_map.get(v).copied(),
                "point_of_interest_type" => poi_id_map.get(v).copied(),
                _ => datapack_id_maps.get(key).and_then(|m| m.get(v).copied()),
            })
            .collect()
    }
}

/// What the per-version tag pass accumulates.
pub(super) struct TagState {
    pub(super) datapack_id_maps: BTreeMap<String, BTreeMap<String, u16>>,
    pub(super) all_registry_keys: HashSet<String>,
    pub(super) latest_tag_modules: Vec<TokenStream>,
    pub(super) latest_match_arms: Vec<TokenStream>,
    pub(super) all_version_code: Vec<TokenStream>,
}

/// Adds the tag constants, map and lookup arm of one registry key.
fn push_tag_key(
    key: String,
    tag_map: BTreeMap<String, Vec<String>>,
    is_latest: bool,
    ver_ident_str: &str,
    maps: &IdMaps,
    state: &mut TagState,
    ver_cat_match_arms: &mut Vec<TokenStream>,
) {
    let TagState {
        datapack_id_maps,
        all_registry_keys,
        latest_tag_modules,
        latest_match_arms,
        all_version_code,
    } = state;
    all_registry_keys.insert(key.clone());
    let key_pascal = format_ident!("{}", key.to_pascal_case());
    let dict_name = if is_latest {
        format_ident!("{}_TAGS", key.to_pascal_case().to_uppercase())
    } else {
        format_ident!(
            "TAGS_{}_{}",
            ver_ident_str,
            key.replace(['/', '.', '-'], "_").to_uppercase()
        )
    };

    let mut tag_entries = Vec::new();
    let mut tag_map_entries = Vec::new();

    if !datapack_id_maps.contains_key(&key) {
        let dir = std::path::Path::new("../../assets/datapack/data/minecraft").join(&key);
        if dir.is_dir() {
            datapack_id_maps.insert(key.clone(), load_datapack_registry_ids(&dir));
        }
    }

    for (tag_name, values) in tag_map {
        let ids: Vec<u16> = maps.ids(&key, &values, datapack_id_maps);

        let tag_const_name = format_ident!(
            "{}",
            tag_name.replace([':', '/', '.', '-'], "_").to_uppercase()
        );

        if is_latest {
            tag_entries.push(quote! {
                pub const #tag_const_name: Tag = (&[#(#values),*], &[#(#ids),*]);
            });
            tag_map_entries.push(quote! { #tag_name => &#key_pascal::#tag_const_name });
        } else {
            tag_map_entries.push(quote! { #tag_name => &(&[#(#values),*], &[#(#ids),*]) });
        }
    }

    if is_latest {
        latest_tag_modules.push(quote! {
            #[allow(non_snake_case)]
            pub mod #key_pascal {
                use super::Tag;
                #(#tag_entries)*
            }
            static #dict_name: phf::Map<&'static str, &'static Tag> = phf::phf_map! {
                #(#tag_map_entries),*
            };
        });
        latest_match_arms.push(quote! { RegistryKey::#key_pascal => &#dict_name });
    } else {
        all_version_code.push(quote! {
            static #dict_name: phf::Map<&'static str, &'static Tag> = phf::phf_map! {
                #(#tag_map_entries),*
            };
        });
        ver_cat_match_arms.push(quote! { RegistryKey::#key_pascal => Some(&#dict_name) });
    }
}

/// Walks the tags of every version, building the tag modules and lookup code.
pub(super) fn collect_version_tags(versions: [(&str, &str); 1], maps: &IdMaps) -> TagState {
    let mut state = TagState {
        datapack_id_maps: BTreeMap::new(),
        all_registry_keys: HashSet::new(),
        latest_tag_modules: Vec::new(),
        latest_match_arms: Vec::new(),
        all_version_code: Vec::new(),
    };

    for (ver_folder, ver_ident_str) in versions {
        let datapack_data_dir = std::path::Path::new("../../assets/datapack/data");
        let datapack_base = datapack_data_dir.join("minecraft");

        let tags = load_datapack_tags(&datapack_data_dir);
        let is_latest = ver_folder == "26_3";

        let mut ver_cat_match_arms = Vec::new();
        let fn_name = format_ident!("get_tags_{}", ver_ident_str);

        for (key, tag_map) in tags {
            push_tag_key(
                key,
                tag_map,
                is_latest,
                ver_ident_str,
                maps,
                &mut state,
                &mut ver_cat_match_arms,
            );
        }

        if !is_latest {
            state.all_version_code.push(quote! {
                #[allow(non_snake_case, unreachable_patterns)]
                const fn #fn_name(key: RegistryKey) -> Option<&'static phf::Map<&'static str, &'static Tag>> {
                    match key {
                        #(#ver_cat_match_arms,)*
                        _ => None,
                    }
                }
            });
        }
    }
    state
}
