//! Reads the timelines and attribute table and emits the attribute enum and timeline types.

use super::{STATIC_ATTRIBUTES, TimelineJson};
use heck::ToPascalCase;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

/// The per-attribute match arms of `EnvironmentAttribute`.
pub(super) struct AttributeMatches {
    variants: TokenStream,
    to_name_match: TokenStream,
    to_resource_match: TokenStream,
    to_id_match: TokenStream,
    from_id_match: TokenStream,
    from_name_match: TokenStream,
    to_type_match: TokenStream,
    syncable_match: TokenStream,
    positional_match: TokenStream,
    spatially_interpolated_match: TokenStream,
}

/// Builds the variant list and every per-attribute match arm.
pub(super) fn attribute_matches() -> AttributeMatches {
    let mut variants = TokenStream::new();
    let mut to_name_match = TokenStream::new();
    let mut to_resource_match = TokenStream::new();
    let mut to_id_match = TokenStream::new();
    let mut from_id_match = TokenStream::new();
    let mut from_name_match = TokenStream::new();
    let mut to_type_match = TokenStream::new();
    let mut syncable_match = TokenStream::new();
    let mut positional_match = TokenStream::new();
    let mut spatially_interpolated_match = TokenStream::new();

    for def in STATIC_ATTRIBUTES {
        let ident = format_ident!("{}", def.name.to_pascal_case());
        let id_lit = def.id;
        let name_str = def.name;
        let resource_str = format!("minecraft:{name_str}");
        let type_ident = format_ident!("{}", def.attr_type);
        let syncable = def.syncable;
        let positional = def.positional;
        let spatially_interpolated = def.spatially_interpolated;

        variants.extend(quote! {
            #ident,
        });

        to_id_match.extend(quote! {
            Self::#ident => #id_lit,
        });

        from_id_match.extend(quote! {
            #id_lit => Some(Self::#ident),
        });

        to_name_match.extend(quote! {
            Self::#ident => #name_str,
        });

        to_resource_match.extend(quote! {
            Self::#ident => #resource_str,
        });

        from_name_match.extend(quote! {
            #name_str | #resource_str => Some(Self::#ident),
        });

        to_type_match.extend(quote! {
            Self::#ident => EnvironmentAttributeType::#type_ident,
        });

        syncable_match.extend(quote! {
            Self::#ident => #syncable,
        });

        positional_match.extend(quote! {
            Self::#ident => #positional,
        });

        spatially_interpolated_match.extend(quote! {
            Self::#ident => #spatially_interpolated,
        });
    }
    AttributeMatches {
        variants,
        to_name_match,
        to_resource_match,
        to_id_match,
        from_id_match,
        from_name_match,
        to_type_match,
        syncable_match,
        positional_match,
        spatially_interpolated_match,
    }
}

/// The `EnvironmentAttribute` enum and its lookups.
pub(super) fn attribute_enum_tokens(matches: &AttributeMatches) -> TokenStream {
    let AttributeMatches {
        variants,
        to_name_match,
        to_resource_match,
        to_id_match,
        from_id_match,
        from_name_match,
        to_type_match,
        syncable_match,
        positional_match,
        spatially_interpolated_match,
    } = matches;
    quote! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum EnvironmentAttribute {
        #variants
    }

    impl EnvironmentAttribute {
        #[must_use]
        pub const fn id(&self) -> u8 {
            match self {
                #to_id_match
            }
        }

        #[must_use]
        pub const fn name(&self) -> &'static str {
            match self {
                #to_name_match
            }
        }

        #[must_use]
        pub const fn resource_location(&self) -> &'static str {
            match self {
                #to_resource_match
            }
        }

        #[must_use]
        pub const fn attribute_type(&self) -> EnvironmentAttributeType {
            match self {
                #to_type_match
            }
        }

        #[must_use]
        pub const fn is_syncable(&self) -> bool {
            match self {
                #syncable_match
            }
        }

        #[must_use]
        pub const fn is_positional(&self) -> bool {
            match self {
                #positional_match
            }
        }

        #[must_use]
        pub const fn is_spatially_interpolated(&self) -> bool {
            match self {
                #spatially_interpolated_match
            }
        }

        #[must_use]
        pub fn from_name(name: &str) -> Option<Self> {
            match name {
                #from_name_match
                _ => None,
            }
        }

        #[must_use]
        pub const fn from_id(id: u8) -> Option<Self> {
            match id {
                #from_id_match
                _ => None,
            }
        }
    }
    }
}

/// The day timeline period and its five gameplay keyframe tracks.
type DayTracks = (
    i32,
    Vec<TokenStream>,
    Vec<TokenStream>,
    Vec<TokenStream>,
    Vec<TokenStream>,
    Vec<TokenStream>,
);

/// Reads the day timeline: period and the five gameplay keyframe tracks.
fn load_day_timeline() -> DayTracks {
    let timeline_path =
        std::path::Path::new("../../assets/datapack/data/minecraft/timeline/day.json");
    if let Ok(content) = std::fs::read_to_string(timeline_path) {
        if let Ok(timeline) = serde_json::from_str::<TimelineJson>(&content) {
            let period = timeline.period_ticks.unwrap_or(24000);
            let mut sky_kfs = Vec::new();
            let mut monsters_kfs = Vec::new();
            let mut bees_kfs = Vec::new();
            let mut creaking_kfs = Vec::new();
            let mut eyeblossom_kfs = Vec::new();

            if let Some(track) = timeline.tracks.get("minecraft:gameplay/sky_light_level") {
                for kf in &track.keyframes {
                    let ticks = kf.ticks;
                    let val = kf.value.as_f64().unwrap_or(1.0) as f32;
                    sky_kfs.push(quote! { FloatKeyframe::new(#ticks, #val) });
                }
            }
            if let Some(track) = timeline.tracks.get("minecraft:gameplay/monsters_burn") {
                for kf in &track.keyframes {
                    let ticks = kf.ticks;
                    let val = kf.value.as_bool().unwrap_or(false);
                    monsters_kfs.push(quote! { BoolKeyframe::new(#ticks, #val) });
                }
            }
            if let Some(track) = timeline.tracks.get("minecraft:gameplay/bees_stay_in_hive") {
                for kf in &track.keyframes {
                    let ticks = kf.ticks;
                    let val = kf.value.as_bool().unwrap_or(false);
                    bees_kfs.push(quote! { BoolKeyframe::new(#ticks, #val) });
                }
            }
            if let Some(track) = timeline.tracks.get("minecraft:gameplay/creaking_active") {
                for kf in &track.keyframes {
                    let ticks = kf.ticks;
                    let val = kf.value.as_bool().unwrap_or(false);
                    creaking_kfs.push(quote! { BoolKeyframe::new(#ticks, #val) });
                }
            }
            if let Some(track) = timeline.tracks.get("minecraft:gameplay/eyeblossom_open") {
                for kf in &track.keyframes {
                    let ticks = kf.ticks;
                    let val = kf.value.as_bool().unwrap_or(false);
                    eyeblossom_kfs.push(quote! { BoolKeyframe::new(#ticks, #val) });
                }
            }
            (
                period,
                sky_kfs,
                monsters_kfs,
                bees_kfs,
                creaking_kfs,
                eyeblossom_kfs,
            )
        } else {
            (
                24000,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
        }
    } else {
        (
            24000,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
    }
}

/// `DayTimeline`: the day period and its gameplay tracks.
pub(super) fn day_timeline_tokens() -> TokenStream {
    let (
        period_ticks,
        sky_light_level_keyframes,
        monsters_burn_keyframes,
        bees_stay_in_hive_keyframes,
        creaking_active_keyframes,
        eyeblossom_open_keyframes,
    ) = load_day_timeline();
    quote! {
    impl DayTimeline {
        pub const PERIOD_TICKS: i32 = #period_ticks;
        pub const SKY_LIGHT_LEVEL_KEYFRAMES: &'static [FloatKeyframe] = &[
            #(#sky_light_level_keyframes),*
        ];
        pub const MONSTERS_BURN_KEYFRAMES: &'static [BoolKeyframe] = &[
            #(#monsters_burn_keyframes),*
        ];
        pub const BEES_STAY_IN_HIVE_KEYFRAMES: &'static [BoolKeyframe] = &[
            #(#bees_stay_in_hive_keyframes),*
        ];
        pub const CREAKING_ACTIVE_KEYFRAMES: &'static [BoolKeyframe] = &[
            #(#creaking_active_keyframes),*
        ];
        pub const EYEBLOSSOM_OPEN_KEYFRAMES: &'static [BoolKeyframe] = &[
            #(#eyeblossom_open_keyframes),*
        ];
    }
    }
}

/// Reads the moon timeline: period, slime spawn chance and moon phase tracks.
fn load_moon_timeline() -> (i32, Vec<TokenStream>, Vec<TokenStream>) {
    let moon_path = std::path::Path::new("../../assets/datapack/data/minecraft/timeline/moon.json");
    if let Ok(content) = std::fs::read_to_string(moon_path) {
        if let Ok(timeline) = serde_json::from_str::<TimelineJson>(&content) {
            let period = timeline.period_ticks.unwrap_or(192000);
            let mut slime_kfs = Vec::new();
            let mut moon_kfs = Vec::new();

            if let Some(track) = timeline
                .tracks
                .get("minecraft:gameplay/surface_slime_spawn_chance")
            {
                for kf in &track.keyframes {
                    let ticks = kf.ticks;
                    let val = kf.value.as_f64().unwrap_or(0.0) as f32;
                    slime_kfs.push(quote! { FloatKeyframe::new(#ticks, #val) });
                }
            }
            if let Some(track) = timeline.tracks.get("minecraft:visual/moon_phase") {
                for kf in &track.keyframes {
                    let ticks = kf.ticks;
                    let phase_str = kf.value.as_str().unwrap_or("full_moon");
                    let phase_ident = format_ident!("{}", phase_str.to_pascal_case());
                    moon_kfs
                        .push(quote! { MoonPhaseKeyframe::new(#ticks, MoonPhase::#phase_ident) });
                }
            }
            (period, slime_kfs, moon_kfs)
        } else {
            (192000, Vec::new(), Vec::new())
        }
    } else {
        (192000, Vec::new(), Vec::new())
    }
}

/// `MoonTimeline`: the moon period, slime chance and phase tracks.
pub(super) fn moon_timeline_tokens() -> TokenStream {
    let (moon_period_ticks, surface_slime_spawn_chance_keyframes, moon_phase_keyframes) =
        load_moon_timeline();
    quote! {
    impl MoonTimeline {
        pub const PERIOD_TICKS: i32 = #moon_period_ticks;
        pub const SURFACE_SLIME_SPAWN_CHANCE_KEYFRAMES: &'static [FloatKeyframe] = &[
            #(#surface_slime_spawn_chance_keyframes),*
        ];
        pub const MOON_PHASE_KEYFRAMES: &'static [MoonPhaseKeyframe] = &[
            #(#moon_phase_keyframes),*
        ];
    }
    }
}

/// Reads the early-game timeline: the pillager patrol track.
fn load_early_game_timeline() -> Vec<TokenStream> {
    let early_game_path =
        std::path::Path::new("../../assets/datapack/data/minecraft/timeline/early_game.json");
    if let Ok(content) = std::fs::read_to_string(early_game_path) {
        if let Ok(timeline) = serde_json::from_str::<TimelineJson>(&content) {
            let mut patrol_kfs = Vec::new();
            if let Some(track) = timeline
                .tracks
                .get("minecraft:gameplay/can_pillager_patrol_spawn")
            {
                for kf in &track.keyframes {
                    let ticks = kf.ticks;
                    let val = kf.value.as_bool().unwrap_or(false);
                    patrol_kfs.push(quote! { BoolKeyframe::new(#ticks, #val) });
                }
            }
            patrol_kfs
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    }
}

/// `EarlyGameTimeline`: the pillager patrol track.
pub(super) fn early_game_timeline_tokens() -> TokenStream {
    let can_pillager_patrol_spawn_keyframes = load_early_game_timeline();
    quote! {
    impl EarlyGameTimeline {
        pub const CAN_PILLAGER_PATROL_SPAWN_KEYFRAMES: &'static [BoolKeyframe] = &[
            #(#can_pillager_patrol_spawn_keyframes),*
        ];
    }
    }
}

/// Reads the villager schedule: period and the adult and baby activity tracks.
fn load_villager_schedule() -> (i32, Vec<TokenStream>, Vec<TokenStream>) {
    let villager_schedule_path = std::path::Path::new(
        "../../assets/datapack/data/minecraft/timeline/villager_schedule.json",
    );
    if let Ok(content) = std::fs::read_to_string(villager_schedule_path) {
        if let Ok(timeline) = serde_json::from_str::<TimelineJson>(&content) {
            let period = timeline.period_ticks.unwrap_or(24000);
            let mut villager_kfs = Vec::new();
            let mut baby_kfs = Vec::new();

            if let Some(track) = timeline.tracks.get("minecraft:gameplay/villager_activity") {
                for kf in &track.keyframes {
                    let ticks = kf.ticks;
                    let val_str = kf.value.as_str().unwrap_or("idle");
                    let clean_str = val_str.strip_prefix("minecraft:").unwrap_or(val_str);
                    let ident = format_ident!("{}", clean_str.to_pascal_case());
                    villager_kfs.push(quote! { ActivityKeyframe::new(#ticks, Activity::#ident) });
                }
            }
            if let Some(track) = timeline
                .tracks
                .get("minecraft:gameplay/baby_villager_activity")
            {
                for kf in &track.keyframes {
                    let ticks = kf.ticks;
                    let val_str = kf.value.as_str().unwrap_or("idle");
                    let clean_str = val_str.strip_prefix("minecraft:").unwrap_or(val_str);
                    let ident = format_ident!("{}", clean_str.to_pascal_case());
                    baby_kfs.push(quote! { ActivityKeyframe::new(#ticks, Activity::#ident) });
                }
            }
            (period, villager_kfs, baby_kfs)
        } else {
            (24000, Vec::new(), Vec::new())
        }
    } else {
        (24000, Vec::new(), Vec::new())
    }
}

/// `VillagerScheduleTimeline`: the schedule period and activity tracks.
pub(super) fn villager_schedule_tokens() -> TokenStream {
    let (schedule_period_ticks, villager_activity_keyframes, baby_villager_activity_keyframes) =
        load_villager_schedule();
    quote! {
    impl VillagerScheduleTimeline {
        pub const PERIOD_TICKS: i32 = #schedule_period_ticks;
        pub const VILLAGER_ACTIVITY_KEYFRAMES: &'static [ActivityKeyframe] = &[
            #(#villager_activity_keyframes),*
        ];
        pub const BABY_VILLAGER_ACTIVITY_KEYFRAMES: &'static [ActivityKeyframe] = &[
            #(#baby_villager_activity_keyframes),*
        ];
    }
    }
}
