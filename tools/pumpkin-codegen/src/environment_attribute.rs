mod registry;
mod static_items;

use heck::ToPascalCase;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use registry::{
    attribute_enum_tokens, attribute_matches, day_timeline_tokens, early_game_timeline_tokens,
    moon_timeline_tokens, villager_schedule_tokens,
};

struct AttributeDef {
    id: u8,
    name: &'static str,
    attr_type: &'static str,
    syncable: bool,
    positional: bool,
    spatially_interpolated: bool,
}

const STATIC_ATTRIBUTES: &[AttributeDef] = &[
    AttributeDef {
        id: 0,
        name: "visual/fog_color",
        attr_type: "RgbColor",
        syncable: true,
        positional: true,
        spatially_interpolated: true,
    },
    AttributeDef {
        id: 1,
        name: "visual/fog_start_distance",
        attr_type: "Float",
        syncable: true,
        positional: true,
        spatially_interpolated: true,
    },
    AttributeDef {
        id: 2,
        name: "visual/fog_end_distance",
        attr_type: "Float",
        syncable: true,
        positional: true,
        spatially_interpolated: true,
    },
    AttributeDef {
        id: 3,
        name: "visual/sky_fog_end_distance",
        attr_type: "Float",
        syncable: true,
        positional: true,
        spatially_interpolated: true,
    },
    AttributeDef {
        id: 4,
        name: "visual/cloud_fog_end_distance",
        attr_type: "Float",
        syncable: true,
        positional: true,
        spatially_interpolated: true,
    },
    AttributeDef {
        id: 5,
        name: "visual/water_fog_color",
        attr_type: "RgbColor",
        syncable: true,
        positional: true,
        spatially_interpolated: true,
    },
    AttributeDef {
        id: 6,
        name: "visual/water_fog_start_distance",
        attr_type: "Float",
        syncable: true,
        positional: true,
        spatially_interpolated: true,
    },
    AttributeDef {
        id: 7,
        name: "visual/water_fog_end_distance",
        attr_type: "Float",
        syncable: true,
        positional: true,
        spatially_interpolated: true,
    },
    AttributeDef {
        id: 8,
        name: "visual/sky_color",
        attr_type: "RgbColor",
        syncable: true,
        positional: true,
        spatially_interpolated: true,
    },
    AttributeDef {
        id: 9,
        name: "visual/sunrise_sunset_color",
        attr_type: "ArgbColor",
        syncable: true,
        positional: true,
        spatially_interpolated: true,
    },
    AttributeDef {
        id: 10,
        name: "visual/cloud_color",
        attr_type: "ArgbColor",
        syncable: true,
        positional: true,
        spatially_interpolated: true,
    },
    AttributeDef {
        id: 11,
        name: "visual/cloud_height",
        attr_type: "Float",
        syncable: true,
        positional: true,
        spatially_interpolated: true,
    },
    AttributeDef {
        id: 12,
        name: "visual/sun_angle",
        attr_type: "AngleDegrees",
        syncable: true,
        positional: true,
        spatially_interpolated: true,
    },
    AttributeDef {
        id: 13,
        name: "visual/moon_angle",
        attr_type: "AngleDegrees",
        syncable: true,
        positional: true,
        spatially_interpolated: true,
    },
    AttributeDef {
        id: 14,
        name: "visual/star_angle",
        attr_type: "AngleDegrees",
        syncable: true,
        positional: true,
        spatially_interpolated: true,
    },
    AttributeDef {
        id: 15,
        name: "visual/moon_phase",
        attr_type: "MoonPhase",
        syncable: true,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 16,
        name: "visual/star_brightness",
        attr_type: "Float",
        syncable: true,
        positional: true,
        spatially_interpolated: true,
    },
    AttributeDef {
        id: 17,
        name: "visual/block_light_tint",
        attr_type: "RgbColor",
        syncable: true,
        positional: true,
        spatially_interpolated: true,
    },
    AttributeDef {
        id: 18,
        name: "visual/sky_light_color",
        attr_type: "RgbColor",
        syncable: true,
        positional: true,
        spatially_interpolated: true,
    },
    AttributeDef {
        id: 19,
        name: "visual/sky_light_factor",
        attr_type: "Float",
        syncable: true,
        positional: true,
        spatially_interpolated: true,
    },
    AttributeDef {
        id: 20,
        name: "visual/night_vision_color",
        attr_type: "RgbColor",
        syncable: true,
        positional: true,
        spatially_interpolated: true,
    },
    AttributeDef {
        id: 21,
        name: "visual/ambient_light_color",
        attr_type: "RgbColor",
        syncable: true,
        positional: true,
        spatially_interpolated: true,
    },
    AttributeDef {
        id: 22,
        name: "visual/default_dripstone_particle",
        attr_type: "Particle",
        syncable: true,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 23,
        name: "visual/ambient_particles",
        attr_type: "AmbientParticles",
        syncable: true,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 24,
        name: "audio/background_music",
        attr_type: "BackgroundMusic",
        syncable: true,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 25,
        name: "audio/music_volume",
        attr_type: "Float",
        syncable: true,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 26,
        name: "audio/ambient_sounds",
        attr_type: "AmbientSounds",
        syncable: true,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 27,
        name: "audio/firefly_bush_sounds",
        attr_type: "Boolean",
        syncable: true,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 28,
        name: "gameplay/sky_light_level",
        attr_type: "Float",
        syncable: true,
        positional: false,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 29,
        name: "gameplay/can_start_raid",
        attr_type: "Boolean",
        syncable: false,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 30,
        name: "gameplay/water_evaporates",
        attr_type: "Boolean",
        syncable: true,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 31,
        name: "gameplay/bed_rule",
        attr_type: "BedRule",
        syncable: false,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 32,
        name: "gameplay/respawn_anchor_works",
        attr_type: "Boolean",
        syncable: false,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 33,
        name: "gameplay/nether_portal_spawns_piglin",
        attr_type: "Boolean",
        syncable: false,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 34,
        name: "gameplay/fast_lava",
        attr_type: "Boolean",
        syncable: true,
        positional: false,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 35,
        name: "gameplay/increased_fire_burnout",
        attr_type: "Boolean",
        syncable: false,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 36,
        name: "gameplay/eyeblossom_open",
        attr_type: "TriState",
        syncable: false,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 37,
        name: "gameplay/turtle_egg_hatch_chance",
        attr_type: "Float",
        syncable: false,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 38,
        name: "gameplay/piglins_zombify",
        attr_type: "Boolean",
        syncable: true,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 39,
        name: "gameplay/snow_golem_melts",
        attr_type: "Boolean",
        syncable: false,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 40,
        name: "gameplay/creaking_active",
        attr_type: "Boolean",
        syncable: true,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 41,
        name: "gameplay/surface_slime_spawn_chance",
        attr_type: "Float",
        syncable: false,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 42,
        name: "gameplay/cat_waking_up_gift_chance",
        attr_type: "Float",
        syncable: false,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 43,
        name: "gameplay/bees_stay_in_hive",
        attr_type: "Boolean",
        syncable: false,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 44,
        name: "gameplay/monsters_burn",
        attr_type: "Boolean",
        syncable: false,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 45,
        name: "gameplay/can_pillager_patrol_spawn",
        attr_type: "Boolean",
        syncable: false,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 46,
        name: "gameplay/villager_activity",
        attr_type: "Activity",
        syncable: false,
        positional: true,
        spatially_interpolated: false,
    },
    AttributeDef {
        id: 47,
        name: "gameplay/baby_villager_activity",
        attr_type: "Activity",
        syncable: false,
        positional: true,
        spatially_interpolated: false,
    },
];

#[derive(serde::Deserialize)]
struct TimelineJson {
    #[serde(default)]
    period_ticks: Option<i32>,
    #[serde(default)]
    tracks: std::collections::BTreeMap<String, TimelineTrackJson>,
}

#[derive(serde::Deserialize)]
struct TimelineTrackJson {
    #[serde(default)]
    keyframes: Vec<KeyframeJson>,
}

#[derive(serde::Deserialize)]
struct KeyframeJson {
    ticks: i32,
    value: serde_json::Value,
}

pub fn build() -> TokenStream {
    let matches = attribute_matches();
    let attribute_enum = attribute_enum_tokens(&matches);
    let day_timeline = day_timeline_tokens();
    let moon_timeline = moon_timeline_tokens();
    let early_game_timeline = early_game_timeline_tokens();
    let villager_schedule_timeline = villager_schedule_tokens();

    let environment_attribute_type_tokens = static_items::environment_attribute_type_tokens();
    let float_keyframe_tokens = static_items::float_keyframe_tokens();
    let moon_phase_tokens = static_items::moon_phase_tokens();
    let activity_tokens = static_items::activity_tokens();
    let activity_tokens_2 = static_items::activity_tokens_2();
    let sample_float_track_tokens = static_items::sample_float_track_tokens();
    let sample_unbounded_bool_track_tokens = static_items::sample_unbounded_bool_track_tokens();
    quote! {
        #environment_attribute_type_tokens

        #attribute_enum

        #float_keyframe_tokens

        #day_timeline

        #moon_phase_tokens

        impl MoonPhaseKeyframe {
            pub const fn new(ticks: i32, value: MoonPhase) -> Self {
                Self { ticks, value }
            }
        }

        pub struct MoonTimeline;

        #moon_timeline

        pub struct EarlyGameTimeline;

        #early_game_timeline

        #activity_tokens

        #activity_tokens_2

        #villager_schedule_timeline

        #sample_float_track_tokens

        #sample_unbounded_bool_track_tokens
    }
}
