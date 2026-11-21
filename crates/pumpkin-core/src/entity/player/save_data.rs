//! The player's own save data: the parts of `Player::write_custom_nbt` and
//! `Player::read_custom_nbt` (experience, status flags, game mode, breath, respawn point, vehicle).

use std::sync::atomic::Ordering;

use pumpkin_data::dimension::Dimension;
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_nbt::tag::NbtTag;
use pumpkin_util::GameMode;
use pumpkin_util::math::{experience, position::BlockPos, vector3::Vector3};

use super::{Player, RespawnPoint, write_root_vehicle};
use crate::entity::mob::warden_spawn_tracker::WardenSpawnTracker;

impl Player {
    /// Experience bar, level, total, enchantment seed and score.
    pub(super) fn write_experience_nbt(&self, nbt: &mut NbtCompound) {
        nbt.put_float("XpP", self.experience_progress.load());
        nbt.put_int("XpLevel", self.experience_level.load(Ordering::Relaxed));
        nbt.put_int("XpTotal", self.xp_state.total.load(Ordering::Relaxed));
        nbt.put_int("XpSeed", self.enchantment_seed.load(Ordering::Relaxed));
        nbt.put_int("Score", self.score.load(Ordering::Relaxed));
    }

    /// Sleep timer, game modes, credits, warden warnings and the fall-particle and played flags.
    pub(super) fn write_status_nbt(&self, nbt: &mut NbtCompound) {
        nbt.put_short("SleepTimer", self.sleeping_since.load().unwrap_or(0) as i16);

        nbt.put_int("playerGameType", self.gamemode.load() as i32);
        if let Some(previous_gamemode) = self.previous_gamemode.load() {
            nbt.put_int("previousPlayerGameType", previous_gamemode as i32);
        }

        nbt.put_bool("seenCredits", self.end_credits.seen.load(Ordering::Relaxed));
        nbt.put_compound(
            "warden_spawn_tracker",
            self.warden_spawn_tracker
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .to_nbt(),
        );
        nbt.put_bool(
            "spawn_extra_particles_on_fall",
            self.spawn_extra_particles_on_fall.load(Ordering::Relaxed),
        );
        nbt.put_bool(
            "HasPlayedBefore",
            self.has_played_before.load(Ordering::Relaxed),
        );
    }

    /// Air supply (as the old short and the new int) and the drowning tick, clamped.
    pub(super) fn write_breath_nbt(&self, nbt: &mut NbtCompound) {
        let air_supply = self
            .breath_manager
            .air_supply
            .load(Ordering::Relaxed)
            .clamp(0, crate::entity::breath::MAX_AIR);
        nbt.put_short("Air", air_supply as i16);
        nbt.put_int("AirSupply", air_supply);
        nbt.put_int(
            "DrowningTick",
            self.breath_manager
                .drowning_tick
                .load(Ordering::Relaxed)
                .clamp(0, crate::entity::breath::DROWNING_INTERVAL - 1),
        );
    }

    /// The respawn point, both as the legacy `Spawn*` fields and the vanilla `respawn` compound.
    pub(super) fn write_respawn_nbt(&self, nbt: &mut NbtCompound) {
        if let Some(respawn) = self
            .respawn_point
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            nbt.put_int("SpawnX", respawn.position.0.x);
            nbt.put_int("SpawnY", respawn.position.0.y);
            nbt.put_int("SpawnZ", respawn.position.0.z);
            nbt.put_string(
                "SpawnDimension",
                respawn.dimension.minecraft_name.to_owned(),
            );
            nbt.put_bool("SpawnForced", respawn.force);

            let mut respawn_compound = NbtCompound::new();
            respawn_compound.put_string("dimension", respawn.dimension.minecraft_name.to_string());
            respawn_compound.put(
                "pos",
                NbtTag::IntArray(vec![
                    respawn.position.0.x,
                    respawn.position.0.y,
                    respawn.position.0.z,
                ]),
            );
            respawn_compound.put_float("angle", respawn.yaw);
            respawn_compound.put_bool("forced", respawn.force);
            nbt.put_compound("respawn", respawn_compound);
        }
    }

    /// The vehicle ridden now, or the one remembered from the save.
    pub(super) fn write_vehicle_nbt(&self, nbt: &mut NbtCompound) {
        let vehicle_uuid = self
            .living_entity
            .entity
            .vehicle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(|vehicle| vehicle.get_entity().entity_uuid)
            .or_else(|| self.root_vehicle_uuid.load());
        if let Some(vehicle_uuid) = vehicle_uuid {
            write_root_vehicle(nbt, vehicle_uuid);
        }
    }

    /// Level, bar and total as stored (vanilla `Player.readAdditionalSaveData`: `getFloatOr`/`getIntOr`
    /// with default 0, nothing derived from each other); the enchantment seed (random when missing)
    /// and the score.
    pub(super) fn read_experience_nbt(&self, nbt: &NbtCompound) {
        let xp_p = nbt.get_float("XpP").unwrap_or(0.0);
        let level = nbt.get_int("XpLevel").unwrap_or(0);
        self.experience_level.store(level, Ordering::Relaxed);
        self.experience_progress.store(xp_p);
        let points = (xp_p * experience::points_in_level(level) as f32).round() as i32;
        self.experience_points.store(points, Ordering::Relaxed);
        self.xp_state
            .total
            .store(nbt.get_int("XpTotal").unwrap_or(0), Ordering::Relaxed);

        self.enchantment_seed.store(
            nbt.get_int("XpSeed").unwrap_or_else(rand::random),
            Ordering::Relaxed,
        );

        self.score
            .store(nbt.get_int("Score").unwrap_or(0), Ordering::Relaxed);
    }

    /// Sleep timer, credits, warden warnings and the fall-particle flag.
    pub(super) fn read_status_nbt(&self, nbt: &NbtCompound) {
        if let Some(sleep_timer) = nbt.get_short("SleepTimer")
            && sleep_timer > 0
        {
            self.sleeping_since.store(Some(sleep_timer as u8));
        }

        self.end_credits.seen.store(
            nbt.get_bool("seenCredits").unwrap_or(false),
            Ordering::Relaxed,
        );
        *self
            .warden_spawn_tracker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = nbt
            .get_compound("warden_spawn_tracker")
            .map(WardenSpawnTracker::from_nbt)
            .unwrap_or_default();
        self.spawn_extra_particles_on_fall.store(
            nbt.get_bool("spawn_extra_particles_on_fall")
                .unwrap_or(false),
            Ordering::Relaxed,
        );
    }

    /// The saved game mode (int or byte; the current one when missing or unknown), the previous
    /// one, and the abilities they imply over the saved ones.
    pub(super) fn read_game_mode_nbt(&self, nbt: &NbtCompound) -> GameMode {
        let gamemode = nbt
            .get_int("playerGameType")
            .or_else(|| nbt.get_byte("playerGameType").map(i32::from))
            .and_then(|val| GameMode::try_from(val).ok())
            .unwrap_or_else(|| self.gamemode.load());

        self.gamemode.store(gamemode);

        self.previous_gamemode.store(
            nbt.get_int("previousPlayerGameType")
                .or_else(|| nbt.get_byte("previousPlayerGameType").map(i32::from))
                .and_then(|val| GameMode::try_from(val).ok()),
        );

        {
            let mut abilities = self
                .abilities
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            abilities.set_for_gamemode(gamemode);
            abilities.read_nbt(nbt);
            if gamemode == GameMode::Creative {
                abilities.allow_flying = true;
                abilities.creative = true;
                abilities.invulnerable = true;
            } else if gamemode == GameMode::Spectator {
                abilities.allow_flying = true;
                abilities.creative = false;
                abilities.invulnerable = true;
            }
        }
        gamemode
    }

    /// Entity flags of a game mode: creative and spectator are invulnerable, spectators fly
    /// through blocks and are never on the ground.
    pub(super) fn apply_game_mode_flags(&self, gamemode: GameMode) {
        self.living_entity.entity.invulnerable.store(
            matches!(gamemode, GameMode::Creative | GameMode::Spectator),
            Ordering::Relaxed,
        );
        self.living_entity
            .entity
            .no_physics
            .store(gamemode == GameMode::Spectator, Ordering::Relaxed);
        if gamemode == GameMode::Spectator {
            self.living_entity
                .entity
                .on_ground
                .store(false, Ordering::Relaxed);
        }
    }

    /// Air supply (the old short wins over the new int) and drowning tick, clamped.
    pub(super) fn read_breath_nbt(&self, nbt: &NbtCompound) {
        if let Some(air) = nbt
            .get_short("Air")
            .map(i32::from)
            .or_else(|| nbt.get_int("AirSupply"))
        {
            self.breath_manager.air_supply.store(
                air.clamp(0, crate::entity::breath::MAX_AIR),
                Ordering::Relaxed,
            );
        }
        if let Some(tick) = nbt.get_int("DrowningTick") {
            self.breath_manager.drowning_tick.store(
                tick.clamp(0, crate::entity::breath::DROWNING_INTERVAL - 1),
                Ordering::Relaxed,
            );
        }
    }

    /// The respawn point: the vanilla `respawn` compound, else the legacy `Spawn*` fields.
    pub(super) fn read_respawn_nbt(&self, nbt: &NbtCompound) {
        // Load any saved spawnpoint data (both vanilla "respawn" compound and legacy SpawnX/SpawnY/SpawnZ)
        if let Some(respawn_compound) = nbt.get_compound("respawn") {
            let dim = respawn_compound
                .get_string("dimension")
                .and_then(|s| Dimension::from_name(s).cloned())
                .unwrap_or_else(|| self.world().dimension.clone());
            let pos = if let Some(pos_array) = respawn_compound.get_int_array("pos")
                && pos_array.len() >= 3
            {
                BlockPos(Vector3::new(pos_array[0], pos_array[1], pos_array[2]))
            } else {
                BlockPos(Vector3::new(0, 0, 0))
            };
            let yaw = respawn_compound.get_float("angle").unwrap_or(0.0);
            let force = respawn_compound.get_bool("forced").unwrap_or(false);
            *self
                .respawn_point
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(RespawnPoint {
                dimension: dim,
                position: pos,
                yaw,
                force,
            });
        } else if let (Some(x), Some(y), Some(z)) = (
            nbt.get_int("SpawnX"),
            nbt.get_int("SpawnY"),
            nbt.get_int("SpawnZ"),
        ) {
            let dim = nbt
                .get_string("SpawnDimension")
                .and_then(|s| Dimension::from_name(s).cloned())
                .unwrap_or_else(|| self.world().dimension.clone());
            let force = nbt.get_bool("SpawnForced").unwrap_or(false);
            *self
                .respawn_point
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(RespawnPoint {
                dimension: dim,
                position: BlockPos(Vector3::new(x, y, z)),
                yaw: 0.0,
                force,
            });
        }
    }
}
