//! The fixed part of the generated biome file: types and trees that do not depend on the biome list.

use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn imports_and_biome_tokens() -> TokenStream {
    quote! {
        use crate::biome::de::Deserialize;
        use crate::entity_type::EntityType;
        use crate::tag::Taggable;
        use crate::tag::RegistryKey;
        use pumpkin_util::biome::{TemperatureModifier, Weather};
        use serde::{Deserializer, de};
        use std::{fmt, hash::{Hasher, Hash}};

        #[derive(Debug)]
        pub struct Biome {
            pub id: u8,
            pub registry_id: &'static str,
            pub be_network_id: u8,
            pub weather: Weather,
            pub carvers: &'static [&'static crate::carver::CarverConfig],
            pub features: &'static [&'static [crate::placed_feature::PlacedFeature]],
            pub creature_spawn_probability: f32,
            pub spawners: SpawnGroups,
            pub spawn_costs: phf::Map<&'static str, SpawnCosts>,
        }

        impl PartialEq<u8> for Biome {
            fn eq(&self, other: &u8) -> bool {
                self.id == *other
            }
        }

        impl PartialEq<Biome> for u8 {
            fn eq(&self, other: &Biome) -> bool {
                *self == other.id
            }
        }

        #[derive(Debug)]
        pub struct SpawnGroups {
            pub monster: &'static [Spawner],
            pub ambient: &'static [Spawner],
            pub axolotls: &'static [Spawner],
            pub creature: &'static [Spawner],
            pub misc: &'static [Spawner],
            pub underground_water_creature: &'static [Spawner],
            pub water_ambient: &'static [Spawner],
            pub water_creature: &'static [Spawner],
        }
    }
}

pub(super) fn biome_tokens() -> TokenStream {
    quote! {
        impl PartialEq for Biome {
            fn eq(&self, other: &Biome) -> bool {
                self.id == other.id
            }
        }

        impl Eq for Biome {}

        impl Hash for Biome {
            fn hash<H: Hasher>(&self, state: &mut H) {
                self.id.hash(state);
            }
        }

        #[derive(Debug)]
        pub struct SpawnCosts {
            pub energy_budget: f64,
            pub charge: f64,
        }

        impl<'de> Deserialize<'de> for &'static Biome {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct BiomeVisitor;

                impl de::Visitor<'_> for BiomeVisitor {
                    type Value = &'static Biome;

                    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                        formatter.write_str("a biome name as a string")
                    }

                    fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
                        self.visit_str(&v)
                    }

                    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                        let biome = Biome::from_name(value.strip_prefix("minecraft:").unwrap_or(value));
                        biome.ok_or_else(|| E::unknown_variant(value, &["unknown biome"]))
                    }
                }

                deserializer.deserialize_str(BiomeVisitor)
            }
        }
    }
}

pub(super) fn biome_tokens_2() -> TokenStream {
    quote! {
        impl Taggable for Biome {
            #[inline]
            fn registry_id(&self) -> u16 {
                self.id as u16
            }
            #[inline]
            fn tag_key() -> RegistryKey {
                RegistryKey::WorldgenBiome
            }
            #[inline]
            fn registry_key(&self) -> &str {
                self.registry_id
            }
        }

        pub const QUANTIZATION_FACTOR: f32 = 10000.0;

        #[inline]
        #[must_use]
        pub const fn quantize_coord(coord: f32) -> i64 {
            (coord * QUANTIZATION_FACTOR) as i64
        }

        #[inline]
        #[must_use]
        pub const fn unquantize_coord(coord: i64) -> f32 {
            coord as f32 / QUANTIZATION_FACTOR
        }

        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        pub struct Parameter {
            pub min: i64,
            pub max: i64,
        }

        pub type ParameterRange = Parameter;
    }
}

pub(super) fn parameter_tokens() -> TokenStream {
    quote! {
        impl Parameter {
            #[must_use]
            pub const fn new(min: i64, max: i64) -> Self {
                Self { min, max }
            }

            #[must_use]
            pub const fn point(min: f32) -> Self {
                Self::span(min, min)
            }

            #[must_use]
            pub const fn span(min: f32, max: f32) -> Self {
                assert!(min <= max, "min > max");
                Self {
                    min: quantize_coord(min),
                    max: quantize_coord(max),
                }
            }

            #[must_use]
            pub const fn span_quantized(min: i64, max: i64) -> Self {
                assert!(min <= max, "min > max");
                Self { min, max }
            }

            #[inline]
            #[must_use]
            pub const fn calc_distance(&self, noise: i64) -> i64 {
                self.distance(noise)
            }

            #[inline]
            #[must_use]
            pub const fn distance(&self, target: i64) -> i64 {
                let above = target - self.max;
                let below = self.min - target;
                if above > 0 {
                    above
                } else if below > 0 {
                    below
                } else {
                    0
                }
            }

            #[inline]
            #[must_use]
            pub const fn distance_parameter(&self, target: &Self) -> i64 {
                let above = target.min - self.max;
                let below = self.min - target.max;
                if above > 0 {
                    above
                } else if below > 0 {
                    below
                } else {
                    0
                }
            }

            #[inline]
            #[must_use]
            pub const fn span_with(&self, other: Option<&Self>) -> Self {
                match other {
                    None => *self,
                    Some(other) => Self {
                        min: if self.min < other.min { self.min } else { other.min },
                        max: if self.max > other.max { self.max } else { other.max },
                    },
                }
            }
        }

        impl fmt::Display for Parameter {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                if self.min == self.max {
                    write!(f, "{}", self.min)
                } else {
                    write!(f, "[{}-{}]", self.min, self.max)
                }
            }
        }
    }
}

pub(super) fn target_point_tokens() -> TokenStream {
    quote! {
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        pub struct TargetPoint {
            pub temperature: i64,
            pub humidity: i64,
            pub continentalness: i64,
            pub erosion: i64,
            pub depth: i64,
            pub weirdness: i64,
        }

        impl TargetPoint {
            #[must_use]
            pub const fn new(
                temperature: i64,
                humidity: i64,
                continentalness: i64,
                erosion: i64,
                depth: i64,
                weirdness: i64,
            ) -> Self {
                Self {
                    temperature,
                    humidity,
                    continentalness,
                    erosion,
                    depth,
                    weirdness,
                }
            }

            #[must_use]
            pub const fn to_parameter_array(&self) -> [i64; 7] {
                [
                    self.temperature,
                    self.humidity,
                    self.continentalness,
                    self.erosion,
                    self.depth,
                    self.weirdness,
                    0,
                ]
            }

            #[must_use]
            pub const fn convert_to_list(&self) -> [i64; 7] {
                self.to_parameter_array()
            }
        }

        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        pub struct ParameterPoint {
            pub temperature: Parameter,
            pub humidity: Parameter,
            pub continentalness: Parameter,
            pub erosion: Parameter,
            pub depth: Parameter,
            pub weirdness: Parameter,
            pub offset: i64,
        }
    }
}

pub(super) fn parameter_point_tokens() -> TokenStream {
    quote! {
        impl ParameterPoint {
            #[must_use]
            pub const fn new(
                temperature: Parameter,
                humidity: Parameter,
                continentalness: Parameter,
                erosion: Parameter,
                depth: Parameter,
                weirdness: Parameter,
                offset: i64,
            ) -> Self {
                Self {
                    temperature,
                    humidity,
                    continentalness,
                    erosion,
                    depth,
                    weirdness,
                    offset,
                }
            }

            #[inline]
            #[must_use]
            pub const fn fitness(&self, target: &TargetPoint) -> i64 {
                let temp_dist = self.temperature.distance(target.temperature);
                let hum_dist = self.humidity.distance(target.humidity);
                let cont_dist = self.continentalness.distance(target.continentalness);
                let ero_dist = self.erosion.distance(target.erosion);
                let dep_dist = self.depth.distance(target.depth);
                let wei_dist = self.weirdness.distance(target.weirdness);

                temp_dist * temp_dist
                    + hum_dist * hum_dist
                    + cont_dist * cont_dist
                    + ero_dist * ero_dist
                    + dep_dist * dep_dist
                    + wei_dist * wei_dist
                    + self.offset * self.offset
            }

            #[must_use]
            pub const fn parameter_space(&self) -> [Parameter; 7] {
                [
                    self.temperature,
                    self.humidity,
                    self.continentalness,
                    self.erosion,
                    self.depth,
                    self.weirdness,
                    Parameter::new(self.offset, self.offset),
                ]
            }
        }

        #[must_use]
        pub const fn target(
            temperature: f32,
            humidity: f32,
            continentalness: f32,
            erosion: f32,
            depth: f32,
            weirdness: f32,
        ) -> TargetPoint {
            TargetPoint::new(
                quantize_coord(temperature),
                quantize_coord(humidity),
                quantize_coord(continentalness),
                quantize_coord(erosion),
                quantize_coord(depth),
                quantize_coord(weirdness),
            )
        }
    }
}

pub(super) fn parameters_tokens() -> TokenStream {
    quote! {
        #[must_use]
        pub const fn parameters(
            temperature: f32,
            humidity: f32,
            continentalness: f32,
            erosion: f32,
            depth: f32,
            weirdness: f32,
            offset: f32,
        ) -> ParameterPoint {
            ParameterPoint::new(
                Parameter::point(temperature),
                Parameter::point(humidity),
                Parameter::point(continentalness),
                Parameter::point(erosion),
                Parameter::point(depth),
                Parameter::point(weirdness),
                quantize_coord(offset),
            )
        }

        #[derive(PartialEq)]
        pub enum BiomeTree {
            Leaf {
                parameters: [ParameterRange; 7],
                biome: &'static Biome,
            },
            Branch {
                parameters: [ParameterRange; 7],
                nodes: &'static [BiomeTree],
            },
        }
    }
}

pub(super) fn biome_tree_tokens() -> TokenStream {
    quote! {
        impl BiomeTree {
           pub fn get(
                &'static self,
                point_list: &[i64; 7],
                previous_result_node: &mut Option<&'static BiomeTree>,
            ) -> &'static Biome {
                // Initialize best distance from the previous result for spatial coherence
                let mut best_dist = previous_result_node
                    .map(|node| node.get_squared_distance(point_list))
                    .unwrap_or(i64::MAX);

                let mut best_node = previous_result_node.unwrap_or(self);

                self.search(point_list, &mut best_dist, &mut best_node);

                match best_node {
                    BiomeTree::Leaf { biome, .. } => {
                        *previous_result_node = Some(best_node);
                        biome
                    }
                    // Should not happen with valid tree data
                    _ => unreachable!("Biome search failed to find a leaf"),
                }
            }

            fn search(
                &'static self,
                point: &[i64; 7],
                best_dist: &mut i64,
                best_node: &mut &'static BiomeTree,
            ) {
                let dist = self.get_squared_distance(point);

                // PRUNING: If this branch/leaf is further than our best, skip it and all children
                if dist >= *best_dist {
                    return;
                }

                match self {
                    Self::Leaf { .. } => {
                        *best_dist = dist;
                        *best_node = self;
                    }
                    Self::Branch { nodes, .. } => {
                        for node in *nodes {
                            node.search(point, best_dist, best_node);
                        }
                    }
                }
            }

            #[inline(always)]
            fn get_squared_distance(&self, p: &[i64; 7]) -> i64 {
                let params = match self {
                    Self::Leaf { parameters, .. } => parameters,
                    Self::Branch { parameters, .. } => parameters,
                };

                let d0 = params[0].calc_distance(p[0]);
                let d1 = params[1].calc_distance(p[1]);
                let d2 = params[2].calc_distance(p[2]);
                let d3 = params[3].calc_distance(p[3]);
                let d4 = params[4].calc_distance(p[4]);
                let d5 = params[5].calc_distance(p[5]);
                let d6 = params[6].calc_distance(p[6]);

                d0 * d0 + d1 * d1 + d2 * d2 + d3 * d3 + d4 * d4 + d5 * d5 + d6 * d6
            }
        }
    }
}
