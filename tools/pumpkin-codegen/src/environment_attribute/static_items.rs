//! The fixed part of the generated environment attribute file.

use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn environment_attribute_type_tokens() -> TokenStream {
    quote! {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum EnvironmentAttributeType {
            RgbColor,
            ArgbColor,
            Float,
            AngleDegrees,
            Boolean,
            TriState,
            MoonPhase,
            Activity,
            BedRule,
            Particle,
            AmbientParticles,
            BackgroundMusic,
            AmbientSounds,
            Integer,
        }
    }
}

pub(super) fn float_keyframe_tokens() -> TokenStream {
    quote! {
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct FloatKeyframe {
            pub ticks: i32,
            pub value: f32,
        }

        impl FloatKeyframe {
            pub const fn new(ticks: i32, value: f32) -> Self {
                Self { ticks, value }
            }
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct BoolKeyframe {
            pub ticks: i32,
            pub value: bool,
        }

        impl BoolKeyframe {
            pub const fn new(ticks: i32, value: bool) -> Self {
                Self { ticks, value }
            }
        }

        pub struct DayTimeline;
    }
}

pub(super) fn moon_phase_tokens() -> TokenStream {
    quote! {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum MoonPhase {
            FullMoon,
            WaningGibbous,
            ThirdQuarter,
            WaningCrescent,
            NewMoon,
            WaxingCrescent,
            FirstQuarter,
            WaxingGibbous,
        }

        impl MoonPhase {
            #[must_use]
            pub const fn index(&self) -> u8 {
                match self {
                    Self::FullMoon => 0,
                    Self::WaningGibbous => 1,
                    Self::ThirdQuarter => 2,
                    Self::WaningCrescent => 3,
                    Self::NewMoon => 4,
                    Self::WaxingCrescent => 5,
                    Self::FirstQuarter => 6,
                    Self::WaxingGibbous => 7,
                }
            }

            #[must_use]
            pub const fn from_index(index: u8) -> Option<Self> {
                match index {
                    0 => Some(Self::FullMoon),
                    1 => Some(Self::WaningGibbous),
                    2 => Some(Self::ThirdQuarter),
                    3 => Some(Self::WaningCrescent),
                    4 => Some(Self::NewMoon),
                    5 => Some(Self::WaxingCrescent),
                    6 => Some(Self::FirstQuarter),
                    7 => Some(Self::WaxingGibbous),
                    _ => None,
                }
            }

            #[must_use]
            pub const fn name(&self) -> &'static str {
                match self {
                    Self::FullMoon => "full_moon",
                    Self::WaningGibbous => "waning_gibbous",
                    Self::ThirdQuarter => "third_quarter",
                    Self::WaningCrescent => "waning_crescent",
                    Self::NewMoon => "new_moon",
                    Self::WaxingCrescent => "waxing_crescent",
                    Self::FirstQuarter => "first_quarter",
                    Self::WaxingGibbous => "waxing_gibbous",
                }
            }

            #[must_use]
            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    "full_moon" => Some(Self::FullMoon),
                    "waning_gibbous" => Some(Self::WaningGibbous),
                    "third_quarter" => Some(Self::ThirdQuarter),
                    "waning_crescent" => Some(Self::WaningCrescent),
                    "new_moon" => Some(Self::NewMoon),
                    "waxing_crescent" => Some(Self::WaxingCrescent),
                    "first_quarter" => Some(Self::FirstQuarter),
                    "waxing_gibbous" => Some(Self::WaxingGibbous),
                    _ => None,
                }
            }

            #[must_use]
            pub const fn start_tick(&self) -> i32 {
                self.index() as i32 * 24000
            }
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct MoonPhaseKeyframe {
            pub ticks: i32,
            pub value: MoonPhase,
        }
    }
}

pub(super) fn activity_tokens() -> TokenStream {
    quote! {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Activity {
            Core,
            Idle,
            Work,
            Play,
            Rest,
            Meet,
            Panic,
            Raid,
            PreRaid,
            Hide,
            Fight,
            Celebrate,
            AdmireItem,
            Avoid,
            Ride,
            PlayDead,
            LongJump,
            Ram,
            Tongue,
            Swim,
            LaySpawn,
            Sniff,
            Investigate,
            Roar,
            Emerge,
            Dig,
        }
    }
}

pub(super) fn activity_tokens_2() -> TokenStream {
    quote! {
        impl Activity {
            #[must_use]
            pub const fn name(&self) -> &'static str {
                match self {
                    Self::Core => "core",
                    Self::Idle => "idle",
                    Self::Work => "work",
                    Self::Play => "play",
                    Self::Rest => "rest",
                    Self::Meet => "meet",
                    Self::Panic => "panic",
                    Self::Raid => "raid",
                    Self::PreRaid => "pre_raid",
                    Self::Hide => "hide",
                    Self::Fight => "fight",
                    Self::Celebrate => "celebrate",
                    Self::AdmireItem => "admire_item",
                    Self::Avoid => "avoid",
                    Self::Ride => "ride",
                    Self::PlayDead => "play_dead",
                    Self::LongJump => "long_jump",
                    Self::Ram => "ram",
                    Self::Tongue => "tongue",
                    Self::Swim => "swim",
                    Self::LaySpawn => "lay_spawn",
                    Self::Sniff => "sniff",
                    Self::Investigate => "investigate",
                    Self::Roar => "roar",
                    Self::Emerge => "emerge",
                    Self::Dig => "dig",
                }
            }

            #[must_use]
            pub fn from_name(name: &str) -> Option<Self> {
                let name = name.strip_prefix("minecraft:").unwrap_or(name);
                match name {
                    "core" => Some(Self::Core),
                    "idle" => Some(Self::Idle),
                    "work" => Some(Self::Work),
                    "play" => Some(Self::Play),
                    "rest" => Some(Self::Rest),
                    "meet" => Some(Self::Meet),
                    "panic" => Some(Self::Panic),
                    "raid" => Some(Self::Raid),
                    "pre_raid" => Some(Self::PreRaid),
                    "hide" => Some(Self::Hide),
                    "fight" => Some(Self::Fight),
                    "celebrate" => Some(Self::Celebrate),
                    "admire_item" => Some(Self::AdmireItem),
                    "avoid" => Some(Self::Avoid),
                    "ride" => Some(Self::Ride),
                    "play_dead" => Some(Self::PlayDead),
                    "long_jump" => Some(Self::LongJump),
                    "ram" => Some(Self::Ram),
                    "tongue" => Some(Self::Tongue),
                    "swim" => Some(Self::Swim),
                    "lay_spawn" => Some(Self::LaySpawn),
                    "sniff" => Some(Self::Sniff),
                    "investigate" => Some(Self::Investigate),
                    "roar" => Some(Self::Roar),
                    "emerge" => Some(Self::Emerge),
                    "dig" => Some(Self::Dig),
                    _ => None,
                }
            }
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct ActivityKeyframe {
            pub ticks: i32,
            pub value: Activity,
        }

        impl ActivityKeyframe {
            pub const fn new(ticks: i32, value: Activity) -> Self {
                Self { ticks, value }
            }
        }

        pub struct VillagerScheduleTimeline;
    }
}

pub(super) fn sample_float_track_tokens() -> TokenStream {
    quote! {
        /// Samples a cyclic float keyframe track with linear interpolation across `period_ticks`.
        #[must_use]
        pub fn sample_float_track(keyframes: &[FloatKeyframe], period_ticks: i32, ticks: i64) -> f32 {
            if keyframes.is_empty() {
                return 1.0;
            }
            if keyframes.len() == 1 {
                return keyframes[0].value;
            }
            let t = ticks.rem_euclid(period_ticks as i64) as f32;
            let n = keyframes.len();
            let first = &keyframes[0];
            let last = &keyframes[n - 1];

            if t <= first.ticks as f32 {
                let seg_len = (first.ticks + period_ticks - last.ticks) as f32;
                if seg_len <= 0.0 {
                    return first.value;
                }
                let alpha = (t + period_ticks as f32 - last.ticks as f32) / seg_len;
                return last.value + alpha * (first.value - last.value);
            }
            if t >= last.ticks as f32 {
                let seg_len = (first.ticks + period_ticks - last.ticks) as f32;
                if seg_len <= 0.0 {
                    return last.value;
                }
                let alpha = (t - last.ticks as f32) / seg_len;
                return last.value + alpha * (first.value - last.value);
            }

            for i in 0..n - 1 {
                let k1 = &keyframes[i];
                let k2 = &keyframes[i + 1];
                if t >= k1.ticks as f32 && t <= k2.ticks as f32 {
                    let seg_len = (k2.ticks - k1.ticks) as f32;
                    if seg_len <= 0.0 {
                        return k1.value;
                    }
                    let alpha = (t - k1.ticks as f32) / seg_len;
                    return k1.value + alpha * (k2.value - k1.value);
                }
            }

            last.value
        }

        /// Samples a cyclic float keyframe track with step interpolation (constant easing) across `period_ticks`.
        #[must_use]
        pub fn sample_step_float_track(keyframes: &[FloatKeyframe], period_ticks: i32, ticks: i64) -> f32 {
            if keyframes.is_empty() {
                return 0.0;
            }
            let t = ticks.rem_euclid(period_ticks as i64) as i32;
            let mut current = keyframes[keyframes.len() - 1].value;
            for kf in keyframes {
                if t >= kf.ticks {
                    current = kf.value;
                } else {
                    break;
                }
            }
            current
        }

        /// Samples a cyclic boolean keyframe track (step function) across `period_ticks`.
        #[must_use]
        pub fn sample_bool_track(keyframes: &[BoolKeyframe], period_ticks: i32, ticks: i64) -> bool {
            if keyframes.is_empty() {
                return false;
            }
            let t = ticks.rem_euclid(period_ticks as i64) as i32;
            let mut current = keyframes[keyframes.len() - 1].value;
            for kf in keyframes {
                if t >= kf.ticks {
                    current = kf.value;
                } else {
                    break;
                }
            }
            current
        }
    }
}

pub(super) fn sample_unbounded_bool_track_tokens() -> TokenStream {
    quote! {
        /// Samples an unbounded (non-cyclic) boolean keyframe track (step function).
        #[must_use]
        pub fn sample_unbounded_bool_track(keyframes: &[BoolKeyframe], ticks: i64) -> bool {
            if keyframes.is_empty() {
                return false;
            }
            let mut current = keyframes[0].value;
            for kf in keyframes {
                if ticks >= kf.ticks as i64 {
                    current = kf.value;
                } else {
                    break;
                }
            }
            current
        }

        /// Samples a cyclic moon phase keyframe track across `period_ticks`.
        #[must_use]
        pub fn sample_moon_phase_track(keyframes: &[MoonPhaseKeyframe], period_ticks: i32, ticks: i64) -> MoonPhase {
            if keyframes.is_empty() {
                return MoonPhase::FullMoon;
            }
            let t = ticks.rem_euclid(period_ticks as i64) as i32;
            let mut current = keyframes[keyframes.len() - 1].value;
            for kf in keyframes {
                if t >= kf.ticks {
                    current = kf.value;
                } else {
                    break;
                }
            }
            current
        }

        /// Samples a cyclic activity keyframe track across `period_ticks`.
        #[must_use]
        pub fn sample_activity_track(keyframes: &[ActivityKeyframe], period_ticks: i32, ticks: i64) -> Activity {
            if keyframes.is_empty() {
                return Activity::Idle;
            }
            let t = ticks.rem_euclid(period_ticks as i64) as i32;
            let mut current = keyframes[keyframes.len() - 1].value;
            for kf in keyframes {
                if t >= kf.ticks {
                    current = kf.value;
                } else {
                    break;
                }
            }
            current
        }
    }
}
