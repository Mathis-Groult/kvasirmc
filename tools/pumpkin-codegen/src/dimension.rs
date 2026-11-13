mod registry;
mod static_items;

use registry::{load_dimensions, push_dimension};
use std::{collections::BTreeMap, fs};

use crate::placed_feature::value_to_int_provider;
use heck::ToShoutySnakeCase;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use serde::Deserialize;

/// Parses a CSS-style hex color string (e.g. `"#78a7ff"` or `"#ccffffff"`) into a signed 32-bit integer.
///
/// # Returns
/// The color as an `i32`, or `None` if the input is not a valid hex color.
fn parse_hex_color(s: &str) -> Option<i32> {
    if let Some(stripped) = s.strip_prefix('#') {
        u32::from_str_radix(stripped, 16).ok().map(|v| v as i32)
    } else {
        None
    }
}

/// Raw deserialization shape for a single dimension entry from `dimension.json`.
#[derive(Deserialize)]
pub struct Dimension {
    /// Whether this dimension has a skylight source (i.e. is not a cave or the Nether).
    pub has_skylight: bool,
    /// Whether this dimension has a bedrock ceiling (e.g. the Nether).
    pub has_ceiling: bool,
    /// Ambient light level added to all blocks, bypassing the normal sky/block-light calculation.
    pub ambient_light: f32,
    /// Coordinate scale factor mapping a position in this dimension to overworld coordinates.
    pub coordinate_scale: f64,
    /// Minimum Y level (inclusive) of the buildable/chunk range.
    pub min_y: i32,
    /// Total height (in blocks) of the buildable/chunk range.
    pub height: i32,
    /// Maximum Y level usable by mob AI and portals (can be less than `min_y + height`).
    pub logical_height: i32,
    /// Tag key for blocks that act as infinite burn sources (e.g. `"minecraft:infiniburn_overworld"`).
    pub infiniburn: String,
    pub monster_spawn_light_level: serde_json::Value,
    pub monster_spawn_block_light_limit: u8,
    /// Fixed day-time value in this dimension, or `None` if time progresses normally.
    #[serde(rename = "fixed_time")]
    pub fixed_time: Option<i64>,
    /// Whether time is fixed in this dimension (modern 26.2 field).
    #[serde(default, rename = "has_fixed_time")]
    pub has_fixed_time: Option<bool>,
    /// Environment attributes map (visual, gameplay, audio).
    #[serde(default)]
    pub attributes: Option<serde_json::Value>,
    /// Optional timeline resource key controlling day/night progression.
    #[serde(default)]
    pub timelines: Option<String>,
}

/// Generates the `TokenStream` for the `Dimension` struct, its constants, and `from_name` lookup.
pub fn build() -> TokenStream {
    let dimensions = load_dimensions();

    let mut variants = TokenStream::new();
    let mut name_to_type = TokenStream::new();

    for (id, (name, dim)) in dimensions.into_iter().enumerate() {
        push_dimension(id, name, dim, &mut variants, &mut name_to_type);
    }

    let imports_and_bed_rule_option_tokens = static_items::imports_and_bed_rule_option_tokens();
    quote!(
        #imports_and_bed_rule_option_tokens

        impl Dimension {
            #variants

            pub fn from_name(name: &str) -> Option<&'static Self> {
                match name {
                    #name_to_type
                    _ => None
                }
            }

            #[inline]
            #[must_use]
            pub const fn effective_sky_light_level(&self) -> f32 {
                if let Some(level) = self.sky_light_level {
                    level
                } else if self.has_skylight {
                    15.0
                } else {
                    0.0
                }
            }
        }
        impl PartialEq for Dimension {
            fn eq(&self, other: &Self) -> bool {
                 self.id == other.id
            }
       }
        impl Eq for Dimension {}
    )
}
