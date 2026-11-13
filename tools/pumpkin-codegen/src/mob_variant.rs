mod sound_enum;

use sound_enum::{
    SoundArms, cat_fns_tokens, eat_fn_tokens, push_variant, sound_enum_tokens, step_fn_tokens,
    wolf_fns_tokens,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use heck::ToPascalCase;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use serde::Deserialize;

fn sound_ident_from_str(s: &str) -> proc_macro2::Ident {
    let bare = s.strip_prefix("minecraft:").unwrap_or(s);
    let pascal = bare.replace('.', "_").to_pascal_case();
    format_ident!("{pascal}")
}

// ══════════════════════════════════════════════════════════════════
// 1. Entity Variants (Cow, Pig, Chicken, Zombie Nautilus)
// ══════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
struct EntityVariantJson {
    asset_id: String,
    #[serde(default)]
    baby_asset_id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    spawn_conditions: Vec<SpawnConditionJson>,
}

#[derive(Deserialize)]
struct SpawnConditionJson {
    #[serde(default)]
    priority: i32,
    #[serde(default)]
    condition: Option<BiomeConditionJson>,
}

#[derive(Deserialize)]
struct BiomeConditionJson {
    #[serde(default)]
    biomes: Option<String>,
}

fn build_entity_variant(
    dir_name: &str,
    enum_name_str: &str,
    is_farm_animal: bool,
    is_nautilus: bool,
) -> TokenStream {
    let dir = Path::new("../../assets/datapack/data/minecraft").join(dir_name);
    let mut variants: BTreeMap<String, EntityVariantJson> = BTreeMap::new();

    let mut entries = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read dir {}: {}", dir.display(), e))
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        if !path.extension().is_some_and(|ext| ext == "json") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let content = fs::read_to_string(&path).expect("read file");
        let json: EntityVariantJson = serde_json::from_str(&content).expect("parse json");
        variants.insert(stem, json);
    }

    let enum_ident = format_ident!("{enum_name_str}");

    let mut enum_variants = Vec::new();
    let mut from_name_arms = Vec::new();
    let mut to_name_arms = Vec::new();
    let mut from_id_arms = Vec::new();
    let mut asset_id_arms = Vec::new();
    let mut baby_asset_id_arms = Vec::new();
    let mut model_arms = Vec::new();
    let mut all_variants = Vec::new();

    for (idx, (name, json)) in variants.iter().enumerate() {
        let variant_ident = format_ident!("{}", name.to_pascal_case());
        let namespaced = format!("minecraft:{name}");
        let asset_id = &json.asset_id;
        let id_u8 = idx as u8;

        let baby_tokens = match &json.baby_asset_id {
            Some(b) => quote! { Some(#b) },
            None => quote! { None },
        };
        let model_tokens = match &json.model {
            Some(m) => quote! { Some(#m) },
            None => quote! { None },
        };

        if name == "temperate" {
            enum_variants.push(quote! {
                #[default]
                #variant_ident = #id_u8
            });
        } else {
            enum_variants.push(quote! {
                #variant_ident = #id_u8
            });
        }

        from_name_arms.push(quote! {
            #namespaced | #name => Some(Self::#variant_ident),
        });
        to_name_arms.push(quote! {
            Self::#variant_ident => #name,
        });
        from_id_arms.push(quote! {
            #id_u8 => Some(Self::#variant_ident),
        });
        asset_id_arms.push(quote! {
            Self::#variant_ident => #asset_id,
        });
        baby_asset_id_arms.push(quote! {
            Self::#variant_ident => #baby_tokens,
        });
        model_arms.push(quote! {
            Self::#variant_ident => #model_tokens,
        });
        all_variants.push(quote! { Self::#variant_ident });
    }

    let biome_selection_fn = if is_farm_animal {
        quote! {
            #[doc = "Selects the appropriate variant based on the biome name, using vanilla farm animal biome tags."]
            #[must_use]
            pub fn select_for_biome(biome_name: &str) -> Self {
                let bare = biome_name.strip_prefix("minecraft:").unwrap_or(biome_name);
                if crate::tag::WorldgenBiome::MINECRAFT_SPAWNS_COLD_VARIANT_FARM_ANIMALS.0.contains(&bare) {
                    Self::Cold
                } else if crate::tag::WorldgenBiome::MINECRAFT_SPAWNS_WARM_VARIANT_FARM_ANIMALS.0.contains(&bare) {
                    Self::Warm
                } else {
                    Self::Temperate
                }
            }
        }
    } else if is_nautilus {
        quote! {
            #[doc = "Selects the appropriate variant based on the biome name, using vanilla zombie nautilus biome tags."]
            #[must_use]
            pub fn select_for_biome(biome_name: &str) -> Self {
                let bare = biome_name.strip_prefix("minecraft:").unwrap_or(biome_name);
                if crate::tag::WorldgenBiome::MINECRAFT_SPAWNS_CORAL_VARIANT_ZOMBIE_NAUTILUS.0.contains(&bare) {
                    Self::Warm
                } else {
                    Self::Temperate
                }
            }
        }
    } else {
        quote! {}
    };

    quote! {
        /* This file is generated. Do not edit manually. */

        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
        #[repr(u8)]
        pub enum #enum_ident {
            #( #enum_variants, )*
        }

        impl #enum_ident {
            pub const ALL: &'static [Self] = &[
                #( #all_variants, )*
            ];

            #[doc = "Returns the variant from a resource name (bare or namespaced)."]
            #[must_use]
            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    #( #from_name_arms )*
                    _ => None,
                }
            }

            #[doc = "Returns the numeric ID of the variant in the synced registry."]
            #[must_use]
            pub const fn id(&self) -> u8 {
                *self as u8
            }

            #[must_use]
            pub const fn from_id(id: u8) -> Option<Self> {
                match id {
                    #( #from_id_arms )*
                    _ => None,
                }
            }

            #[doc = "Returns the bare string name of the variant."]
            #[must_use]
            pub const fn to_name(&self) -> &'static str {
                match self {
                    #( #to_name_arms )*
                }
            }

            #[must_use]
            pub const fn asset_id(&self) -> &'static str {
                match self {
                    #( #asset_id_arms )*
                }
            }

            #[must_use]
            pub const fn baby_asset_id(&self) -> Option<&'static str> {
                match self {
                    #( #baby_asset_id_arms )*
                }
            }

            #[must_use]
            pub const fn model(&self) -> Option<&'static str> {
                match self {
                    #( #model_arms )*
                }
            }

            #[must_use]
            pub const fn all() -> &'static [Self] {
                Self::ALL
            }

            #biome_selection_fn
        }
    }
}

pub fn build_cow() -> TokenStream {
    build_entity_variant("cow_variant", "CowVariant", true, false)
}

pub fn build_pig() -> TokenStream {
    build_entity_variant("pig_variant", "PigVariant", true, false)
}

pub fn build_chicken() -> TokenStream {
    build_entity_variant("chicken_variant", "ChickenVariant", true, false)
}

pub fn build_zombie_nautilus() -> TokenStream {
    build_entity_variant(
        "zombie_nautilus_variant",
        "ZombieNautilusVariant",
        false,
        true,
    )
}

// ══════════════════════════════════════════════════════════════════
// 2. Sound Variants (Cow, Pig, Chicken, Cat, Wolf)
// ══════════════════════════════════════════════════════════════════

#[derive(Deserialize, Clone, Default)]
struct SoundGroupJson {
    #[serde(default)]
    ambient_sound: Option<String>,
    #[serde(default)]
    death_sound: Option<String>,
    #[serde(default)]
    hurt_sound: Option<String>,
    #[serde(default)]
    step_sound: Option<String>,
    #[serde(default)]
    eat_sound: Option<String>,
    #[serde(default)]
    hiss_sound: Option<String>,
    #[serde(default)]
    purr_sound: Option<String>,
    #[serde(default)]
    purreow_sound: Option<String>,
    #[serde(default)]
    beg_for_food_sound: Option<String>,
    #[serde(default)]
    stray_ambient_sound: Option<String>,
    #[serde(default)]
    growl_sound: Option<String>,
    #[serde(default)]
    pant_sound: Option<String>,
    #[serde(default)]
    whine_sound: Option<String>,
}

#[derive(Deserialize)]
struct SoundVariantJson {
    #[serde(default)]
    adult_sounds: Option<SoundGroupJson>,
    #[serde(default)]
    baby_sounds: Option<SoundGroupJson>,

    // Fallbacks for flat structure (like cow)
    #[serde(default)]
    ambient_sound: Option<String>,
    #[serde(default)]
    death_sound: Option<String>,
    #[serde(default)]
    hurt_sound: Option<String>,
    #[serde(default)]
    step_sound: Option<String>,
}

impl SoundVariantJson {
    fn adult_group(&self) -> SoundGroupJson {
        if let Some(adult) = &self.adult_sounds {
            adult.clone()
        } else {
            SoundGroupJson {
                ambient_sound: self.ambient_sound.clone(),
                death_sound: self.death_sound.clone(),
                hurt_sound: self.hurt_sound.clone(),
                step_sound: self.step_sound.clone(),
                ..Default::default()
            }
        }
    }

    fn baby_group(&self) -> SoundGroupJson {
        if let Some(baby) = &self.baby_sounds {
            baby.clone()
        } else {
            self.adult_group()
        }
    }
}

fn load_sound_variants(dir_name: &str) -> BTreeMap<String, SoundVariantJson> {
    let dir = Path::new("../../assets/datapack/data/minecraft").join(dir_name);
    let mut variants: BTreeMap<String, SoundVariantJson> = BTreeMap::new();

    let mut entries = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read dir {}: {}", dir.display(), e))
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        if !path.extension().is_some_and(|ext| ext == "json") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let content = fs::read_to_string(&path).expect("read file");
        let json: SoundVariantJson = serde_json::from_str(&content).expect("parse json");
        variants.insert(stem, json);
    }
    variants
}

pub fn build_cow_sound() -> TokenStream {
    let variants = load_sound_variants("cow_sound_variant");
    build_common_sound_enum("CowSoundVariant", &variants, false, false, false)
}

pub fn build_pig_sound() -> TokenStream {
    let variants = load_sound_variants("pig_sound_variant");
    build_common_sound_enum("PigSoundVariant", &variants, true, false, false)
}

pub fn build_chicken_sound() -> TokenStream {
    let variants = load_sound_variants("chicken_sound_variant");
    build_common_sound_enum("ChickenSoundVariant", &variants, false, false, false)
}

pub fn build_cat_sound() -> TokenStream {
    let variants = load_sound_variants("cat_sound_variant");
    build_common_sound_enum("CatSoundVariant", &variants, true, true, false)
}

pub fn build_wolf_sound() -> TokenStream {
    let variants = load_sound_variants("wolf_sound_variant");
    build_common_sound_enum("WolfSoundVariant", &variants, false, false, true)
}

fn build_common_sound_enum(
    enum_name_str: &str,
    variants: &BTreeMap<String, SoundVariantJson>,
    has_eat: bool,
    has_cat_specifics: bool,
    has_wolf_specifics: bool,
) -> TokenStream {
    let enum_ident = format_ident!("{enum_name_str}");

    let mut arms = SoundArms::default();
    for (idx, (name, json)) in variants.iter().enumerate() {
        push_variant(
            &mut arms,
            idx,
            name,
            json,
            has_eat,
            has_cat_specifics,
            has_wolf_specifics,
        );
    }

    let step_fn = step_fn_tokens(&arms);
    let eat_fn = eat_fn_tokens(&arms, has_eat);
    let cat_fns = cat_fns_tokens(&arms, has_cat_specifics);
    let wolf_fns = wolf_fns_tokens(&arms, has_wolf_specifics);

    sound_enum_tokens(&enum_ident, &arms, &step_fn, &eat_fn, &cat_fns, &wolf_fns)
}
