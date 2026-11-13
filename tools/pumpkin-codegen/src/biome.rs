mod registry;
mod static_items;

use heck::ToPascalCase;
use registry::{load_biomes, push_biome};
use std::{collections::BTreeMap, fs};

use heck::ToShoutySnakeCase;
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use serde::Deserialize;
use serde_json::Value;
use syn::LitInt;

#[derive(Deserialize)]
struct GeyserBiomeMapping {
    bedrock_id: u8,
}

fn deserialize_carvers<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        String(String),
        Vec(Vec<String>),
    }

    match Option::<StringOrVec>::deserialize(deserializer)? {
        Some(StringOrVec::String(s)) => Ok(vec![s]),
        Some(StringOrVec::Vec(v)) => Ok(v),
        None => Ok(Vec::new()),
    }
}

/// Raw deserialization shape for a single biome entry from `biome.json`.
#[derive(Deserialize)]
pub struct Biome {
    /// Whether this biome has precipitation (rain or snow).
    has_precipitation: bool,
    /// Base temperature of the biome affecting weather and foliage color.
    temperature: f32,
    /// Downfall value affecting how wet the biome is (influences snow/rain).
    downfall: f32,
    /// Optional modifier that changes how temperature is applied.
    temperature_modifier: Option<TemperatureModifier>,
    #[serde(default, deserialize_with = "deserialize_carvers")]
    pub carvers: Vec<String>,
    /// Nested lists of feature resource-location strings applied during world generation.
    features: Vec<Vec<String>>,
    /// Probability per chunk tick that a creature spawns, if not overridden per-biome.
    creature_spawn_probability: Option<f32>,
    /// Spawn group data for each entity category in this biome.
    #[serde(default)]
    spawners: SpawnGroups,
    /// Per-entity spawn cost budget entries, keyed by namespaced entity ID.
    #[serde(default)]
    spawn_costs: BTreeMap<String, SpawnCosts>,
    /// Environment attributes; 26.3 stores natural mob spawns here.
    #[serde(default)]
    attributes: BTreeMap<String, Value>,
    /// Numeric registry ID assigned to this biome.
    #[serde(default)]
    pub id: u8,
}

impl Biome {
    fn apply_natural_mob_spawns(&mut self) {
        let Some(attr) = self.attributes.get("minecraft:gameplay/natural_mob_spawns") else {
            return;
        };
        let Some(argument) = attr.get("argument") else {
            return;
        };
        if let Some(spawns) = argument.get("spawns_by_category")
            && let Ok(groups) = serde_json::from_value::<SpawnGroups>(spawns.clone())
        {
            self.spawners = groups;
        }
        if let Some(costs) = argument.get("spawn_costs")
            && let Ok(spawn_costs) = serde_json::from_value(costs.clone())
        {
            self.spawn_costs = spawn_costs;
        }
    }
}

/// Spawn group data for all entity categories within a biome.
#[derive(Deserialize, Default, PartialEq, Eq, Hash)]
struct SpawnGroups {
    /// Hostile mob spawners for this biome.
    #[serde(default)]
    monster: Vec<Spawner>,
    /// Ambient creature spawners (e.g. bats) for this biome.
    #[serde(default)]
    ambient: Vec<Spawner>,
    /// Axolotl spawners for this biome.
    #[serde(default)]
    axolotls: Vec<Spawner>,
    /// Passive creature spawners (e.g. cows, sheep) for this biome.
    #[serde(default)]
    creature: Vec<Spawner>,
    /// Miscellaneous entity spawners for this biome.
    #[serde(default)]
    misc: Vec<Spawner>,
    /// Underground water creature spawners (e.g. glow squid) for this biome.
    #[serde(default)]
    underground_water_creature: Vec<Spawner>,
    /// Water ambient spawners (e.g. fish) for this biome.
    #[serde(default)]
    water_ambient: Vec<Spawner>,
    /// Water creature spawners (e.g. dolphins) for this biome.
    #[serde(default)]
    water_creature: Vec<Spawner>,
}

/// A single entity spawner entry within a spawn group, as defined in `biome.json`.
#[derive(Hash, PartialEq, Eq)]
struct Spawner {
    /// Namespaced entity type ID (e.g. `"minecraft:zombie"`).
    r#type: String,
    /// Minimum number of entities in a spawn group.
    min_count: i32,
    /// Maximum number of entities in a spawn group.
    max_count: i32,
}

impl<'de> Deserialize<'de> for Spawner {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            r#type: String,
            #[serde(default, alias = "minCount")]
            min_count: Option<i32>,
            #[serde(default, alias = "maxCount")]
            max_count: Option<i32>,
            #[serde(default)]
            count: Option<Value>,
        }

        let raw = Raw::deserialize(deserializer)?;
        let (min_count, max_count) = match raw.count {
            Some(Value::Number(n)) => {
                let value = n.as_i64().unwrap_or(1) as i32;
                (value, value)
            }
            Some(Value::Object(object)) => {
                let min = object
                    .get("min_inclusive")
                    .and_then(Value::as_i64)
                    .unwrap_or(1) as i32;
                let max = object
                    .get("max_inclusive")
                    .and_then(Value::as_i64)
                    .unwrap_or(min as i64) as i32;
                (min, max)
            }
            _ => (raw.min_count.unwrap_or(1), raw.max_count.unwrap_or(1)),
        };
        Ok(Self {
            r#type: raw.r#type,
            min_count,
            max_count,
        })
    }
}

impl Spawner {
    /// Converts this spawner entry into a `TokenStream` for use in generated code.
    pub fn to_tokens(&self) -> TokenStream {
        let r#type = &self.r#type;
        let min_count = &self.min_count;
        let max_count = &self.max_count;
        quote! {
            Spawner {
                r#type: #r#type,
                min_count: #min_count,
                max_count: #max_count,
            }
        }
    }
}

/// Mob spawn cost data controlling how this entity affects biome spawn budgets.
#[derive(Deserialize, PartialEq)]
struct SpawnCosts {
    /// Maximum energy this entity type may consume from the spawn budget.
    energy_budget: f64,
    /// Energy cost charged to the budget each time this entity spawns.
    charge: f64,
}

impl SpawnCosts {
    /// Converts these spawn costs into a `TokenStream` for use in generated code.
    pub fn to_tokens(&self) -> TokenStream {
        let energy_budget = &self.energy_budget;
        let charge = &self.charge;
        quote! {
            SpawnCosts {
                energy_budget: #energy_budget,
                charge: #charge,
            }
        }
    }
}

/// Optional modifier that adjusts how biome temperature behaves (e.g. frozen biomes).
#[derive(Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
enum TemperatureModifier {
    /// No modification; temperature is used as-is.
    None,
    /// Temperature is forced below freezing regardless of the base value.
    Frozen,
}

/// An inclusive range used as one dimension in the multi-noise biome source tree.
#[derive(Deserialize)]
struct ParameterRange {
    /// Lower bound of the noise range (inclusive).
    min: i64,
    /// Upper bound of the noise range (inclusive).
    max: i64,
}

impl ParameterRange {
    /// Converts this parameter range into a `TokenStream` for use in generated code.
    fn into_token_stream(self) -> TokenStream {
        let min = self.min;
        let max = self.max;

        quote! {
            ParameterRange {
                min: #min,
                max: #max
            }
        }
    }
}

/// A node in the multi-noise biome source k-d tree, either a leaf (resolved biome)
/// or a branch (spatial partition with child nodes).
#[derive(Deserialize)]
#[serde(tag = "_type", rename_all = "lowercase")]
enum BiomeTree {
    /// A terminal node resolving to a specific biome.
    Leaf {
        /// The seven noise-parameter ranges defining this leaf's position in parameter space.
        parameters: [ParameterRange; 7],
        /// Namespaced biome resource location (e.g. `"minecraft:plains"`).
        biome: String,
    },
    /// An interior node that partitions parameter space and delegates to child nodes.
    Branch {
        /// The seven noise-parameter ranges bounding this branch's region.
        parameters: [ParameterRange; 7],
        /// Child nodes of this branch (leaves or further branches).
        #[serde(rename = "subTree")]
        nodes: Box<[Self]>,
    },
}

impl BiomeTree {
    /// Converts this biome tree node into a `TokenStream` for use in generated code.
    fn into_token_stream(self) -> TokenStream {
        match self {
            Self::Leaf { parameters, biome } => {
                let biome = format_ident!(
                    "{}",
                    biome
                        .strip_prefix("minecraft:")
                        .unwrap()
                        .to_shouty_snake_case()
                );
                let parameters = parameters.map(ParameterRange::into_token_stream);
                quote! {
                    BiomeTree::Leaf {
                        parameters: [#(#parameters),*],
                        biome: &Biome::#biome
                    }
                }
            }
            Self::Branch { parameters, nodes } => {
                let nodes = nodes
                    .into_iter()
                    .map(Self::into_token_stream)
                    .collect::<Vec<_>>();
                let parameters = parameters.map(ParameterRange::into_token_stream);
                quote! {
                    BiomeTree::Branch {
                        parameters: [#(#parameters),*],
                        nodes: &[#(#nodes),*]
                    }
                }
            }
        }
    }
}

/// Root container holding the overworld and nether multi-noise biome source trees.
#[derive(Deserialize)]
struct MultiNoiseBiomeSuppliers {
    /// Multi-noise k-d tree used to resolve biomes in the overworld dimension.
    overworld: BiomeTree,
    /// Multi-noise k-d tree used to resolve biomes in the nether dimension.
    nether: BiomeTree,
}

/// Generates the `TokenStream` for the `Biome` struct, its constants, lookup methods,
/// the multi-noise biome source trees, and the `BiomeTree` search implementation.
pub fn build() -> TokenStream {
    let biomes = load_biomes();

    let biome_trees: MultiNoiseBiomeSuppliers = serde_json::from_str(
        &fs::read_to_string("../../assets/multi_noise_biome_tree.json").unwrap(),
    )
    .expect("Failed to parse multi_noise_biome_tree.json");
    let bedrock_biomes: BTreeMap<String, GeyserBiomeMapping> =
        serde_json::from_str(&fs::read_to_string("../../assets/bedrock/biomes.json").unwrap())
            .expect("Failed to parse biomes.json");

    let mut variants = TokenStream::new();
    let mut name_to_type = TokenStream::new();
    let mut id_to_type = TokenStream::new();
    let mut all_variants = TokenStream::new();

    for (name, biome) in biomes {
        push_biome(
            name,
            biome,
            &bedrock_biomes,
            &mut variants,
            &mut name_to_type,
            &mut id_to_type,
            &mut all_variants,
        );
    }

    let overworld_tree = biome_trees.overworld.into_token_stream();
    let nether_tree = biome_trees.nether.into_token_stream();
    let imports_and_biome_tokens = static_items::imports_and_biome_tokens();
    let biome_tokens = static_items::biome_tokens();
    let biome_tokens_2 = static_items::biome_tokens_2();
    let parameter_tokens = static_items::parameter_tokens();
    let target_point_tokens = static_items::target_point_tokens();
    let parameter_point_tokens = static_items::parameter_point_tokens();
    let parameters_tokens = static_items::parameters_tokens();
    let biome_tree_tokens = static_items::biome_tree_tokens();
    quote! {
        #imports_and_biome_tokens

        #[derive(Debug)]
        pub struct Spawner {
            pub r#type: &'static str,
            pub min_count: i32,
            pub max_count: i32,
        }

        #biome_tokens

        impl Biome {
            #variants

            pub const ALL: &'static [&'static Self] = &[#all_variants];

            pub fn from_name(name: &str) -> Option<&'static Self> {
                match name {
                    #name_to_type
                    _ => None
                }
            }

            pub const fn from_id(id: u8) -> Option<&'static Self> {
                match id {
                    #id_to_type
                    _ => None
                }
            }
        }

        #biome_tokens_2

        #parameter_tokens

        #target_point_tokens

        #parameter_point_tokens

        #parameters_tokens

        #biome_tree_tokens

        pub const OVERWORLD_BIOME_SOURCE: BiomeTree = #overworld_tree;
        pub const NETHER_BIOME_SOURCE: BiomeTree = #nether_tree;
    }
}
