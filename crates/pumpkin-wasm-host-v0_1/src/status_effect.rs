use crate::pumpkin::plugin::status_effect;
use pumpkin_wasm_host_common::state::PluginHostState;

impl status_effect::Host for PluginHostState {}

/// Generates both directions of the plugin WIT `status-effect-type` <-> `EffectType` mapping
/// from one list; the WIT enum is maintained by hand, so each pair is written once here.
macro_rules! status_effect_pairs {
    ($($variant:ident),+ $(,)?) => {
        #[must_use]
        pub const fn from_wasm_status_effect_type(
            t: status_effect::StatusEffectType,
        ) -> pumpkin_data::status_effect::EffectType {
            match t {
                $(status_effect::StatusEffectType::$variant => {
                    pumpkin_data::status_effect::EffectType::$variant
                })+
            }
        }

        #[must_use]
        pub const fn to_wasm_status_effect_type(
            t: pumpkin_data::status_effect::EffectType,
        ) -> status_effect::StatusEffectType {
            match t {
                $(pumpkin_data::status_effect::EffectType::$variant => {
                    status_effect::StatusEffectType::$variant
                })+
            }
        }
    };
}

status_effect_pairs!(
    Speed,
    Slowness,
    Haste,
    MiningFatigue,
    Strength,
    InstantHealth,
    InstantDamage,
    JumpBoost,
    Nausea,
    Regeneration,
    Resistance,
    FireResistance,
    WaterBreathing,
    Invisibility,
    Blindness,
    NightVision,
    Hunger,
    Weakness,
    Poison,
    Wither,
    HealthBoost,
    Absorption,
    Saturation,
    Glowing,
    Levitation,
    Luck,
    Unluck,
    SlowFalling,
    ConduitPower,
    DolphinsGrace,
    BadOmen,
    HeroOfTheVillage,
    Darkness,
    TrialOmen,
    RaidOmen,
    WindCharged,
    Weaving,
    Oozing,
    Infested,
);

#[must_use]
pub fn to_wasm_status_effect_instance(
    effect: &pumpkin_data::potion::Effect,
) -> Option<status_effect::StatusEffectInstance> {
    let name = effect
        .effect_type
        .minecraft_name
        .strip_prefix("minecraft:")
        .unwrap_or(effect.effect_type.minecraft_name);
    let effect_type_enum = pumpkin_data::status_effect::EffectType::from_name(name)?;
    let wasm_type = to_wasm_status_effect_type(effect_type_enum);
    Some(status_effect::StatusEffectInstance {
        effect_type: wasm_type,
        duration: u32::try_from(effect.duration).unwrap_or(0),
        amplifier: effect.amplifier,
        ambient: effect.ambient,
        show_particles: effect.show_particles,
        show_icon: effect.show_icon,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumpkin_data::status_effect::EffectType;

    const WIT: &str = include_str!("../../pumpkin-plugin-wit/v0.1/status-effect.wit");

    /// Every variant of the WIT `status-effect-type` enum, as its `snake_case` vanilla name.
    fn wit_variant_names() -> Vec<String> {
        WIT.lines()
            .skip_while(|l| !l.contains("enum status-effect-type"))
            .skip(1)
            .take_while(|l| !l.contains('}'))
            .map(|l| l.trim().trim_end_matches(',').replace('-', "_"))
            .collect()
    }

    #[test]
    fn every_variant_round_trips_with_its_own_name() {
        let names = wit_variant_names();
        assert_eq!(names.len(), 39, "WIT variant count changed: {names:?}");
        for name in names {
            let effect = EffectType::from_name(&name)
                .unwrap_or_else(|| panic!("no EffectType for WIT variant {name}"));
            let wasm = to_wasm_status_effect_type(effect);
            // Variant names are identical on both sides, so a swapped pair shows here.
            let wasm_name = format!("{wasm:?}");
            let wasm_name = wasm_name.trim_start_matches("StatusEffectType::");
            assert_eq!(wasm_name, format!("{effect:?}"), "to_wasm {name}");
            assert_eq!(
                from_wasm_status_effect_type(wasm),
                effect,
                "from_wasm {name}"
            );
        }
    }

    #[test]
    fn pinned_pairs() {
        let pairs = [
            (status_effect::StatusEffectType::Speed, EffectType::Speed),
            (
                status_effect::StatusEffectType::MiningFatigue,
                EffectType::MiningFatigue,
            ),
            (
                status_effect::StatusEffectType::Infested,
                EffectType::Infested,
            ),
        ];
        for (wasm, effect) in pairs {
            assert_eq!(from_wasm_status_effect_type(wasm), effect);
            assert_eq!(to_wasm_status_effect_type(effect), wasm);
        }
    }
}
