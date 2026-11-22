use crate::pumpkin::plugin::status_effect;
use pumpkin_wasm_host_common::state::PluginHostState;

impl status_effect::Host for PluginHostState {}

#[must_use]
#[allow(clippy::too_many_lines)]
pub const fn from_wasm_status_effect_type(
    t: status_effect::StatusEffectType,
) -> pumpkin_data::status_effect::EffectType {
    match t {
        status_effect::StatusEffectType::Speed => pumpkin_data::status_effect::EffectType::Speed,
        status_effect::StatusEffectType::Slowness => {
            pumpkin_data::status_effect::EffectType::Slowness
        }
        status_effect::StatusEffectType::Haste => pumpkin_data::status_effect::EffectType::Haste,
        status_effect::StatusEffectType::MiningFatigue => {
            pumpkin_data::status_effect::EffectType::MiningFatigue
        }
        status_effect::StatusEffectType::Strength => {
            pumpkin_data::status_effect::EffectType::Strength
        }
        status_effect::StatusEffectType::InstantHealth => {
            pumpkin_data::status_effect::EffectType::InstantHealth
        }
        status_effect::StatusEffectType::InstantDamage => {
            pumpkin_data::status_effect::EffectType::InstantDamage
        }
        status_effect::StatusEffectType::JumpBoost => {
            pumpkin_data::status_effect::EffectType::JumpBoost
        }
        status_effect::StatusEffectType::Nausea => pumpkin_data::status_effect::EffectType::Nausea,
        status_effect::StatusEffectType::Regeneration => {
            pumpkin_data::status_effect::EffectType::Regeneration
        }
        status_effect::StatusEffectType::Resistance => {
            pumpkin_data::status_effect::EffectType::Resistance
        }
        status_effect::StatusEffectType::FireResistance => {
            pumpkin_data::status_effect::EffectType::FireResistance
        }
        status_effect::StatusEffectType::WaterBreathing => {
            pumpkin_data::status_effect::EffectType::WaterBreathing
        }
        status_effect::StatusEffectType::Invisibility => {
            pumpkin_data::status_effect::EffectType::Invisibility
        }
        status_effect::StatusEffectType::Blindness => {
            pumpkin_data::status_effect::EffectType::Blindness
        }
        status_effect::StatusEffectType::NightVision => {
            pumpkin_data::status_effect::EffectType::NightVision
        }
        status_effect::StatusEffectType::Hunger => pumpkin_data::status_effect::EffectType::Hunger,
        status_effect::StatusEffectType::Weakness => {
            pumpkin_data::status_effect::EffectType::Weakness
        }
        status_effect::StatusEffectType::Poison => pumpkin_data::status_effect::EffectType::Poison,
        status_effect::StatusEffectType::Wither => pumpkin_data::status_effect::EffectType::Wither,
        status_effect::StatusEffectType::HealthBoost => {
            pumpkin_data::status_effect::EffectType::HealthBoost
        }
        status_effect::StatusEffectType::Absorption => {
            pumpkin_data::status_effect::EffectType::Absorption
        }
        status_effect::StatusEffectType::Saturation => {
            pumpkin_data::status_effect::EffectType::Saturation
        }
        status_effect::StatusEffectType::Glowing => {
            pumpkin_data::status_effect::EffectType::Glowing
        }
        status_effect::StatusEffectType::Levitation => {
            pumpkin_data::status_effect::EffectType::Levitation
        }
        status_effect::StatusEffectType::Luck => pumpkin_data::status_effect::EffectType::Luck,
        status_effect::StatusEffectType::Unluck => pumpkin_data::status_effect::EffectType::Unluck,
        status_effect::StatusEffectType::SlowFalling => {
            pumpkin_data::status_effect::EffectType::SlowFalling
        }
        status_effect::StatusEffectType::ConduitPower => {
            pumpkin_data::status_effect::EffectType::ConduitPower
        }
        status_effect::StatusEffectType::DolphinsGrace => {
            pumpkin_data::status_effect::EffectType::DolphinsGrace
        }
        status_effect::StatusEffectType::BadOmen => {
            pumpkin_data::status_effect::EffectType::BadOmen
        }
        status_effect::StatusEffectType::HeroOfTheVillage => {
            pumpkin_data::status_effect::EffectType::HeroOfTheVillage
        }
        status_effect::StatusEffectType::Darkness => {
            pumpkin_data::status_effect::EffectType::Darkness
        }
        status_effect::StatusEffectType::TrialOmen => {
            pumpkin_data::status_effect::EffectType::TrialOmen
        }
        status_effect::StatusEffectType::RaidOmen => {
            pumpkin_data::status_effect::EffectType::RaidOmen
        }
        status_effect::StatusEffectType::WindCharged => {
            pumpkin_data::status_effect::EffectType::WindCharged
        }
        status_effect::StatusEffectType::Weaving => {
            pumpkin_data::status_effect::EffectType::Weaving
        }
        status_effect::StatusEffectType::Oozing => pumpkin_data::status_effect::EffectType::Oozing,
        status_effect::StatusEffectType::Infested => {
            pumpkin_data::status_effect::EffectType::Infested
        }
    }
}

#[must_use]
#[allow(clippy::too_many_lines)]
pub const fn to_wasm_status_effect_type(
    t: pumpkin_data::status_effect::EffectType,
) -> status_effect::StatusEffectType {
    match t {
        pumpkin_data::status_effect::EffectType::Speed => status_effect::StatusEffectType::Speed,
        pumpkin_data::status_effect::EffectType::Slowness => {
            status_effect::StatusEffectType::Slowness
        }
        pumpkin_data::status_effect::EffectType::Haste => status_effect::StatusEffectType::Haste,
        pumpkin_data::status_effect::EffectType::MiningFatigue => {
            status_effect::StatusEffectType::MiningFatigue
        }
        pumpkin_data::status_effect::EffectType::Strength => {
            status_effect::StatusEffectType::Strength
        }
        pumpkin_data::status_effect::EffectType::InstantHealth => {
            status_effect::StatusEffectType::InstantHealth
        }
        pumpkin_data::status_effect::EffectType::InstantDamage => {
            status_effect::StatusEffectType::InstantDamage
        }
        pumpkin_data::status_effect::EffectType::JumpBoost => {
            status_effect::StatusEffectType::JumpBoost
        }
        pumpkin_data::status_effect::EffectType::Nausea => status_effect::StatusEffectType::Nausea,
        pumpkin_data::status_effect::EffectType::Regeneration => {
            status_effect::StatusEffectType::Regeneration
        }
        pumpkin_data::status_effect::EffectType::Resistance => {
            status_effect::StatusEffectType::Resistance
        }
        pumpkin_data::status_effect::EffectType::FireResistance => {
            status_effect::StatusEffectType::FireResistance
        }
        pumpkin_data::status_effect::EffectType::WaterBreathing => {
            status_effect::StatusEffectType::WaterBreathing
        }
        pumpkin_data::status_effect::EffectType::Invisibility => {
            status_effect::StatusEffectType::Invisibility
        }
        pumpkin_data::status_effect::EffectType::Blindness => {
            status_effect::StatusEffectType::Blindness
        }
        pumpkin_data::status_effect::EffectType::NightVision => {
            status_effect::StatusEffectType::NightVision
        }
        pumpkin_data::status_effect::EffectType::Hunger => status_effect::StatusEffectType::Hunger,
        pumpkin_data::status_effect::EffectType::Weakness => {
            status_effect::StatusEffectType::Weakness
        }
        pumpkin_data::status_effect::EffectType::Poison => status_effect::StatusEffectType::Poison,
        pumpkin_data::status_effect::EffectType::Wither => status_effect::StatusEffectType::Wither,
        pumpkin_data::status_effect::EffectType::HealthBoost => {
            status_effect::StatusEffectType::HealthBoost
        }
        pumpkin_data::status_effect::EffectType::Absorption => {
            status_effect::StatusEffectType::Absorption
        }
        pumpkin_data::status_effect::EffectType::Saturation => {
            status_effect::StatusEffectType::Saturation
        }
        pumpkin_data::status_effect::EffectType::Glowing => {
            status_effect::StatusEffectType::Glowing
        }
        pumpkin_data::status_effect::EffectType::Levitation => {
            status_effect::StatusEffectType::Levitation
        }
        pumpkin_data::status_effect::EffectType::Luck => status_effect::StatusEffectType::Luck,
        pumpkin_data::status_effect::EffectType::Unluck => status_effect::StatusEffectType::Unluck,
        pumpkin_data::status_effect::EffectType::SlowFalling => {
            status_effect::StatusEffectType::SlowFalling
        }
        pumpkin_data::status_effect::EffectType::ConduitPower => {
            status_effect::StatusEffectType::ConduitPower
        }
        pumpkin_data::status_effect::EffectType::DolphinsGrace => {
            status_effect::StatusEffectType::DolphinsGrace
        }
        pumpkin_data::status_effect::EffectType::BadOmen => {
            status_effect::StatusEffectType::BadOmen
        }
        pumpkin_data::status_effect::EffectType::HeroOfTheVillage => {
            status_effect::StatusEffectType::HeroOfTheVillage
        }
        pumpkin_data::status_effect::EffectType::Darkness => {
            status_effect::StatusEffectType::Darkness
        }
        pumpkin_data::status_effect::EffectType::TrialOmen => {
            status_effect::StatusEffectType::TrialOmen
        }
        pumpkin_data::status_effect::EffectType::RaidOmen => {
            status_effect::StatusEffectType::RaidOmen
        }
        pumpkin_data::status_effect::EffectType::WindCharged => {
            status_effect::StatusEffectType::WindCharged
        }
        pumpkin_data::status_effect::EffectType::Weaving => {
            status_effect::StatusEffectType::Weaving
        }
        pumpkin_data::status_effect::EffectType::Oozing => status_effect::StatusEffectType::Oozing,
        pumpkin_data::status_effect::EffectType::Infested => {
            status_effect::StatusEffectType::Infested
        }
    }
}

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
