pub mod behavior;
pub mod brain;
pub mod control;
pub mod goal;
pub mod pathfinder;
pub mod random_pos;
pub mod sensing;
pub mod target_match;
pub mod target_predicate;
pub mod util;

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Weak};

    use arc_swap::ArcSwap;
    use pumpkin_config::world::LevelConfig;
    use pumpkin_config::{AdvancedConfiguration, BasicConfiguration, TelemetryConfig};
    use pumpkin_data::dimension::Dimension;
    use pumpkin_util::GameMode;
    use pumpkin_util::world_seed::Seed;
    use pumpkin_world::{level::Level, world_info::LevelData};
    use uuid::Uuid;

    use crate::data::VanillaData;
    use crate::entity::player::Player;
    use crate::net::java::JavaClient;
    use crate::net::java::pending::PendingConnection;
    use crate::net::packet_limiter::PacketRateLimiter;
    use crate::net::{ClientPlatform, GameProfile, PlayerConfig};
    use crate::server::Server;
    use crate::{block::registry::default_registry, world::World};

    pub fn world(folder: &tempfile::TempDir) -> Arc<World> {
        world_in_dimension(folder, Dimension::OVERWORLD.clone())
    }

    pub fn world_in_dimension(folder: &tempfile::TempDir, dimension: Dimension) -> Arc<World> {
        Arc::new(World::load(
            Level::from_root_folder(
                &LevelConfig::default(),
                folder.path().to_path_buf(),
                0,
                dimension.clone(),
            ),
            Arc::new(ArcSwap::from_pointee(LevelData::default(Seed(0)))),
            dimension,
            default_registry(),
            Weak::new(),
        ))
    }

    /// A real server on an empty folder: no network, telemetry off, nothing read from or written
    /// to the working directory. About one second. Call `stop()` at the end.
    pub async fn server(
        folder: &tempfile::TempDir,
    ) -> Result<Arc<Server>, Box<dyn std::error::Error>> {
        let basic = BasicConfiguration {
            default_level_name: folder.path().join("world").to_string_lossy().into_owned(),
            ..BasicConfiguration::default()
        };
        let telemetry = TelemetryConfig {
            enabled: false,
            ..TelemetryConfig::default()
        };
        let vanilla = VanillaData {
            banned_ip_list: std::sync::RwLock::default(),
            banned_player_list: std::sync::RwLock::default(),
            operator_config: std::sync::RwLock::default(),
            user_cache: std::sync::RwLock::default(),
            whitelist_config: std::sync::RwLock::default(),
        };
        Ok(Server::new(
            basic,
            AdvancedConfiguration::default(),
            telemetry,
            vanilla,
            Vec::new(),
        )
        .await?)
    }

    /// A real player over an in-memory pipe (nothing reads what the server sends it), not added
    /// to the world. Each call gets a new id and UUID. `world` must belong to a live server
    /// (see [`server`]): `Player::new` exits the process otherwise.
    pub fn java_player(world: &Arc<World>, gamemode: GameMode) -> Arc<Player> {
        let (client, profile) = java_client();
        player_of(world, gamemode, client, profile)
    }

    /// [`java_player`] whose outgoing packets the test reads through the returned tap (`None`
    /// only if a new client no longer owned its outgoing queue).
    pub fn java_player_with_tap(
        world: &Arc<World>,
        gamemode: GameMode,
    ) -> Option<(Arc<Player>, crate::net::java::PacketTap)> {
        let (mut client, profile) = java_client();
        let tap = client.take_packet_tap()?;
        Some((player_of(world, gamemode, client, profile), tap))
    }

    fn java_client() -> (JavaClient, GameProfile) {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let (client_end, server_end) = tokio::io::duplex(1 << 16);
        drop(client_end);
        let (read, write) = tokio::io::split(server_end);
        let pending = PendingConnection::from_transport(
            Box::new(read),
            Box::new(write),
            std::net::SocketAddr::from(([127, 0, 0, 1], 25565)),
            id,
            PacketRateLimiter::new(false, 0.0, 0.0),
            Weak::new(),
        );
        let profile = GameProfile {
            id: Uuid::from_u128(u128::from(id)),
            name: format!("Test{id}"),
            properties: ArcSwap::from_pointee(Vec::new()),
            profile_actions: None,
        };
        let client = JavaClient::from_pending(pending, profile.clone(), PlayerConfig::default());
        (client, profile)
    }

    fn player_of(
        world: &Arc<World>,
        gamemode: GameMode,
        client: JavaClient,
        profile: GameProfile,
    ) -> Arc<Player> {
        Arc::new(Player::new(
            Arc::new(ClientPlatform::Java(client)),
            profile,
            PlayerConfig::default(),
            world,
            gamemode,
        ))
    }
}
