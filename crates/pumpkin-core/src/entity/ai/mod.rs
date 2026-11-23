pub mod behavior;
pub mod brain;
pub mod control;
#[cfg(test)]
#[path = "pathfinder/fly_neighbors_golden.rs"]
mod fly_neighbors_golden;
#[cfg(test)]
#[path = "pathfinder/find_path_golden.rs"]
mod find_path_golden;
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
                &test_level_config(),
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

    /// The default world settings, except that the stronghold ring search waits until a test
    /// asks for strongholds: started at load it costs each test server about 2 CPU seconds on a
    /// background pool (card #79), with identical rings either way.
    fn test_level_config() -> LevelConfig {
        LevelConfig {
            stronghold_search_at_load: false,
            ..LevelConfig::default()
        }
    }

    /// A real server on an empty folder: no network, telemetry off, a fixed seed, nothing read
    /// from or written to the working directory. About one second. Call `stop()` at the end.
    pub async fn server(
        folder: &tempfile::TempDir,
    ) -> Result<Arc<Server>, Box<dyn std::error::Error>> {
        server_with(folder, |_| {}).await
    }

    /// [`server`] with the basic configuration adjusted by `adjust` before the server starts.
    pub async fn server_with(
        folder: &tempfile::TempDir,
        adjust: impl FnOnce(&mut BasicConfiguration),
    ) -> Result<Arc<Server>, Box<dyn std::error::Error>> {
        server_with_configs(folder, adjust, |_| {}).await
    }

    /// [`server_with`] that also adjusts the advanced configuration (networking, resource
    /// packs) before the server starts.
    pub async fn server_with_configs(
        folder: &tempfile::TempDir,
        adjust: impl FnOnce(&mut BasicConfiguration),
        adjust_advanced: impl FnOnce(&mut AdvancedConfiguration),
    ) -> Result<Arc<Server>, Box<dyn std::error::Error>> {
        let mut basic = BasicConfiguration {
            default_level_name: folder.path().join("world").to_string_lossy().into_owned(),
            ..BasicConfiguration::default()
        };
        // A fixed seed: packets that carry it (Bedrock `StartGame`) are then the same in every
        // run; the default configuration picks a random one.
        basic.seed.0 = 20_261_112;
        adjust(&mut basic);
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
        let mut advanced = AdvancedConfiguration {
            world: test_level_config(),
            ..AdvancedConfiguration::default()
        };
        adjust_advanced(&mut advanced);
        Ok(Server::new(
            basic,
            advanced,
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

    /// A real Bedrock player over an offline `NetherNet` session (see
    /// `nethernet::offline_session`), not added to the world, with its outgoing packets read
    /// through the returned tap. `world` must belong to a live server (see [`server`]).
    pub async fn bedrock_player_with_tap(
        world: &Arc<World>,
        gamemode: GameMode,
    ) -> Result<(Arc<Player>, crate::net::bedrock::BedrockPacketTap), String> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], 19132));
        let session = crate::net::bedrock::nethernet::offline_session(address).await?;
        let client = Arc::new(crate::net::bedrock::BedrockClient::new(
            session,
            address,
            Arc::default(),
            PacketRateLimiter::new(false, 0.0, 0.0),
        ));
        let tap = client
            .take_packet_tap()
            .ok_or("a new Bedrock client no longer owned its outgoing queue")?;
        let profile = GameProfile {
            id: Uuid::from_u128((1 << 64) | u128::from(id)),
            name: format!("Bedrock{id}"),
            properties: ArcSwap::from_pointee(Vec::new()),
            profile_actions: None,
        };
        let player = Arc::new(Player::new(
            Arc::new(ClientPlatform::Bedrock(client)),
            profile,
            PlayerConfig::default(),
            world,
            gamemode,
        ));
        Ok((player, tap))
    }

    /// Who a test player is, fixed by the test: packets that carry the name, the UUID or the
    /// entity id are then the same in every run (the entity id counter is global to the test
    /// process, so it depends on which tests ran before).
    pub struct Identity {
        pub name: &'static str,
        pub uuid: Uuid,
        pub entity_id: i32,
    }

    impl Identity {
        fn profile(&self) -> GameProfile {
            GameProfile {
                id: self.uuid,
                name: self.name.to_string(),
                properties: ArcSwap::from_pointee(Vec::new()),
                profile_actions: None,
            }
        }

        fn player(
            &self,
            client: ClientPlatform,
            world: &Arc<World>,
            gamemode: GameMode,
        ) -> Arc<Player> {
            let mut player = Player::new(
                Arc::new(client),
                self.profile(),
                PlayerConfig::default(),
                world,
                gamemode,
            );
            player.living_entity.entity.entity_id = self.entity_id;
            Arc::new(player)
        }
    }

    /// As [`bedrock_player_with_tap`], as `who`.
    pub async fn bedrock_player_as(
        world: &Arc<World>,
        gamemode: GameMode,
        who: &Identity,
    ) -> Result<(Arc<Player>, crate::net::bedrock::BedrockPacketTap), String> {
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], 19132));
        let session = crate::net::bedrock::nethernet::offline_session(address).await?;
        let client = Arc::new(crate::net::bedrock::BedrockClient::new(
            session,
            address,
            Arc::default(),
            PacketRateLimiter::new(false, 0.0, 0.0),
        ));
        let tap = client
            .take_packet_tap()
            .ok_or("a new Bedrock client no longer owned its outgoing queue")?;
        Ok((
            who.player(ClientPlatform::Bedrock(client), world, gamemode),
            tap,
        ))
    }

    /// As [`java_player_with_tap`], as `who`.
    pub fn java_player_as(
        world: &Arc<World>,
        gamemode: GameMode,
        who: &Identity,
    ) -> Option<(Arc<Player>, crate::net::java::PacketTap)> {
        let mut client = java_client_of(who.profile(), u64::from(who.entity_id.unsigned_abs()));
        let tap = client.take_packet_tap()?;
        Some((
            who.player(ClientPlatform::Java(client), world, gamemode),
            tap,
        ))
    }

    fn java_client() -> (JavaClient, GameProfile) {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let profile = GameProfile {
            id: Uuid::from_u128(u128::from(id)),
            name: format!("Test{id}"),
            properties: ArcSwap::from_pointee(Vec::new()),
            profile_actions: None,
        };
        (java_client_of(profile.clone(), id), profile)
    }

    fn java_client_of(profile: GameProfile, id: u64) -> JavaClient {
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
        JavaClient::from_pending(pending, profile, PlayerConfig::default())
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
