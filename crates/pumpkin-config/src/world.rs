use serde::{Deserialize, Deserializer, Serialize};

use crate::{chunk::ChunkConfig, lighting::LightingEngineConfig};

/// Configuration for world and level-specific settings.
///
/// Currently, it includes chunk-related options; more settings may be added later.
#[derive(Deserialize, Serialize, Clone)]
pub struct LevelConfig {
    /// Configuration for chunk behaviour and management.
    pub chunk: ChunkConfig,
    /// Configuration for lighting engine propagation mode.
    #[serde(default)]
    pub lighting: LightingEngineConfig,
    /// Number of ticks between autosave checks. If 0, autosave is disabled.
    #[serde(default = "default_autosave_ticks")]
    pub autosave_ticks: u64,
    /// Margins for ticking entities in parallel islands.
    #[serde(default)]
    pub entity_islands: EntityIslandsConfig,
    /// How often mobs far from players, unseen or crowded make their full AI decisions.
    #[serde(default)]
    pub mob_lod: kvasir_moblod::Config,
    /// How redstone dust is updated.
    #[serde(default)]
    pub redstone: kvasir_wirenet::Config,
    /// Start the stronghold ring search on a background pool as soon as the overworld loads (as
    /// vanilla does), so the first eye of ender or `/locate` does not wait for it. With `false`
    /// the same search runs the first time something asks for strongholds. Test servers turn it
    /// off: each one otherwise spends about 2 CPU seconds on rings no test reads.
    #[serde(default = "default_true")]
    pub stronghold_search_at_load: bool,
    // TODO: More options
}

/// Safety margins for ticking entities in parallel islands. Lower values tick more entities in
/// parallel but can let interactions run out of tick order.
#[derive(Deserialize, Serialize, Clone, Copy)]
#[serde(default)]
pub struct EntityIslandsConfig {
    /// Blocks an entity whose tick is not audited can reach beyond its follow range.
    #[serde(deserialize_with = "EntityIslandsConfig::deserialize_unaudited_reach")]
    pub unaudited_reach: f64,
    /// Blocks a hit from such an entity can throw an audited one in a tick. Raise it for TNT
    /// cannons.
    #[serde(deserialize_with = "EntityIslandsConfig::deserialize_knockback_reach")]
    pub knockback_reach: f64,
}

impl EntityIslandsConfig {
    fn deserialize_unaudited_reach<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<f64, D::Error> {
        deserialize_margin(deserializer, Self::default().unaudited_reach)
    }

    fn deserialize_knockback_reach<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<f64, D::Error> {
        deserialize_margin(deserializer, Self::default().knockback_reach)
    }
}

fn deserialize_margin<'de, D: Deserializer<'de>>(
    deserializer: D,
    fallback: f64,
) -> Result<f64, D::Error> {
    let value = f64::deserialize(deserializer)?;
    Ok(if value.is_finite() {
        value.max(0.0)
    } else {
        fallback
    })
}

impl Default for EntityIslandsConfig {
    fn default() -> Self {
        Self {
            unaudited_reach: 16.0,
            knockback_reach: 4.0,
        }
    }
}

const fn default_autosave_ticks() -> u64 {
    6000 // Default to 5 minutes at 20 TPS
}

const fn default_true() -> bool {
    true
}

impl Default for LevelConfig {
    fn default() -> Self {
        Self {
            chunk: ChunkConfig::default(),
            lighting: LightingEngineConfig::default(),
            autosave_ticks: default_autosave_ticks(),
            entity_islands: EntityIslandsConfig::default(),
            mob_lod: kvasir_moblod::Config::default(),
            redstone: kvasir_wirenet::Config::default(),
            stronghold_search_at_load: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_level_config_enables_autosave() {
        assert_eq!(LevelConfig::default().autosave_ticks, 6000);
    }

    #[test]
    fn generated_config_round_trip_keeps_autosave_enabled() {
        let generated = toml::to_string(&LevelConfig::default()).unwrap();
        let reloaded: LevelConfig = toml::from_str(&generated).unwrap();
        assert_eq!(reloaded.autosave_ticks, 6000);
    }

    #[test]
    fn an_explicit_zero_still_disables_autosave() {
        let disabled = LevelConfig {
            autosave_ticks: 0,
            ..LevelConfig::default()
        };
        let generated = toml::to_string(&disabled).unwrap();
        let reloaded: LevelConfig = toml::from_str(&generated).unwrap();
        assert_eq!(reloaded.autosave_ticks, 0);
    }

    #[test]
    fn stronghold_search_starts_at_load_unless_turned_off() {
        assert!(LevelConfig::default().stronghold_search_at_load);
        // A config file written before the option existed keeps the vanilla behaviour.
        let mut old = toml::to_string(&LevelConfig::default()).unwrap();
        old = old.replace("stronghold_search_at_load = true\n", "");
        assert!(!old.contains("stronghold_search_at_load"));
        let reloaded: LevelConfig = toml::from_str(&old).unwrap();
        assert!(reloaded.stronghold_search_at_load);
        let off = LevelConfig {
            stronghold_search_at_load: false,
            ..LevelConfig::default()
        };
        let reloaded: LevelConfig = toml::from_str(&toml::to_string(&off).unwrap()).unwrap();
        assert!(!reloaded.stronghold_search_at_load);
    }
}
