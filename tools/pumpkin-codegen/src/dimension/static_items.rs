//! The fixed part of the generated dimension file.

use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn imports_and_bed_rule_option_tokens() -> TokenStream {
    quote! {
        use pumpkin_util::math::int_provider::{
            BiasedToBottomIntProvider, ClampedIntProvider, TrapezoidIntProvider, ClampedNormalIntProvider,
            ConstantIntProvider, IntProvider, NormalIntProvider, UniformIntProvider,
            WeightedEntry, WeightedListIntProvider,
        };

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum BedRuleOption {
            Always,
            WhenDark,
            Never,
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct BedRule {
            pub can_sleep: BedRuleOption,
            pub can_set_spawn: BedRuleOption,
            pub explodes: bool,
        }

        impl BedRule {
            #[inline]
            #[must_use]
            pub const fn can_sleep(&self, is_dark_outside: bool) -> bool {
                match self.can_sleep {
                    BedRuleOption::Always => true,
                    BedRuleOption::WhenDark => is_dark_outside,
                    BedRuleOption::Never => false,
                }
            }

            #[inline]
            #[must_use]
            pub const fn can_set_spawn(&self, is_dark_outside: bool) -> bool {
                match self.can_set_spawn {
                    BedRuleOption::Always => true,
                    BedRuleOption::WhenDark => is_dark_outside,
                    BedRuleOption::Never => false,
                }
            }
        }

        #[derive(Debug, Clone)]
        pub struct Dimension {
            pub id: u8,
            pub minecraft_name: &'static str,
            pub fixed_time: Option<i64>,
            pub has_fixed_time: bool,
            pub has_skylight: bool,
            pub has_ceiling: bool,
            pub coordinate_scale: f64,
            pub min_y: i32,
            pub height: i32,
            pub logical_height: i32,
            pub infiniburn: &'static str,
            pub ambient_light: f32,
            pub monster_spawn_light_level: IntProvider,
            pub monster_spawn_block_light_limit: u8,
            pub sky_color: Option<i32>,
            pub fog_color: Option<i32>,
            pub cloud_color: Option<i32>,
            pub ambient_light_color: Option<i32>,
            pub sky_light_color: Option<i32>,
            pub sky_light_factor: Option<f32>,
            pub fog_start_distance: Option<f32>,
            pub fog_end_distance: Option<f32>,
            pub cloud_height: Option<f32>,
            pub sky_light_level: Option<f32>,
            pub water_evaporates: bool,
            pub fast_lava: bool,
            pub respawn_anchor_works: bool,
            pub piglins_zombify: bool,
            pub snow_golem_melts: bool,
            pub can_start_raid: bool,
            pub nether_portal_spawns_piglin: bool,
            pub bed_rule: BedRule,
            pub timelines: Option<&'static str>,
        }
    }
}
