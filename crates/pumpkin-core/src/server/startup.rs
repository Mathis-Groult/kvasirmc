//! The stages of `Server::new`: level info, dimensions, thread pools, tick profile, key
//! fetches and worlds.

#[allow(clippy::wildcard_imports)]
use super::*;

#[cfg(test)]
mod level_fallback_tests;

/// The rayon pools the tick runs on.
pub(super) struct TickPools {
    pub(super) simulation: Arc<rayon::ThreadPool>,
    pub(super) sequential: Arc<rayon::ThreadPool>,
    pub(super) compute: Arc<rayon::ThreadPool>,
}

/// Vanilla 26.3 `LevelStorageAccess.getUnfixedDataTagWithFallback` and
/// `restoreLevelDataFromOld`: when `level.dat` is missing or cannot be read and `level.dat_old`
/// exists, the backup is read; if that works, an unreadable `level.dat` is moved aside as
/// `level.dat_corrupted_<time>` and the backup takes its place. Without a backup, or when the
/// data was read but is unsupported, the first answer stands.
fn read_level_info_with_fallback(
    world_path: &std::path::Path,
) -> Result<LevelData, WorldInfoError> {
    let first = AnvilLevelInfo.read_world_info(world_path);
    let backup_path = world_path.join(LEVEL_DAT_BACKUP_FILE_NAME);
    let Err(
        error @ (WorldInfoError::InfoNotFound
        | WorldInfoError::DeserializationError(_)
        | WorldInfoError::IoError(_)),
    ) = first
    else {
        return first;
    };
    if !backup_path.exists() {
        return Err(error);
    }
    warn!(
        "Failed to load world data from {}: {error}",
        world_path.join(LEVEL_DAT_FILE_NAME).display()
    );
    info!("Attempting to use fallback {}", backup_path.display());
    let level_info = AnvilLevelInfo
        .read_world_info_from(world_path, LEVEL_DAT_BACKUP_FILE_NAME)
        .inspect_err(|error| {
            error!(
                "The fallback {} cannot be read either: {error}",
                backup_path.display()
            );
        })?;
    restore_level_data_from_old(world_path)?;
    Ok(level_info)
}

/// Moves an unreadable `level.dat` aside (vanilla names it after the local time,
/// `yyyy-MM-dd_HH-mm-ss`) and puts `level.dat_old` in its place.
fn restore_level_data_from_old(world_path: &std::path::Path) -> std::io::Result<()> {
    let dat_path = world_path.join(LEVEL_DAT_FILE_NAME);
    if dat_path.exists() {
        let now =
            time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
        let aside = world_path.join(format!(
            "{LEVEL_DAT_FILE_NAME}_corrupted_{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
            now.year(),
            u8::from(now.month()),
            now.day(),
            now.hour(),
            now.minute(),
            now.second()
        ));
        warn!(
            "Moving the unreadable {} aside to {}",
            dat_path.display(),
            aside.display()
        );
        fs::rename(&dat_path, aside)?;
    }
    fs::rename(world_path.join(LEVEL_DAT_BACKUP_FILE_NAME), &dat_path)
}

/// The world's `level.dat` (or its backup, see [`read_level_info_with_fallback`]), backed up to
/// `level.dat_old` first; a world without either is created from the configured seed.
/// Unsupported or unreadable data stops the start.
pub(super) fn load_level_info(
    world_path: &std::path::Path,
    basic_config: &BasicConfiguration,
) -> Result<LevelData, WorldInfoError> {
    let level_info = match read_level_info_with_fallback(world_path) {
        Ok(level_info) => {
            let dat_path = world_path.join(LEVEL_DAT_FILE_NAME);
            if dat_path.exists() {
                let backup_path = world_path.join(LEVEL_DAT_BACKUP_FILE_NAME);
                if let Err(err) = fs::copy(&dat_path, &backup_path) {
                    warn!("Failed to create backup {LEVEL_DAT_BACKUP_FILE_NAME}: {err}");
                }
            }
            level_info
        }
        Err(WorldInfoError::InfoNotFound) => {
            warn!(
                "No {LEVEL_DAT_FILE_NAME} in {}, creating a new world with seed {}",
                world_path.display(),
                basic_config.seed.0 as i64
            );
            let overworld_gen = pumpkin_world::generation::generator::VanillaGenerator::new(
                basic_config.seed,
                Dimension::OVERWORLD,
            );
            let default_data = LevelData::from_world_generator(basic_config.seed, &overworld_gen);
            AnvilLevelInfo.write_world_info(&default_data, world_path)?;
            default_data
        }
        Err(
            error @ (WorldInfoError::UnsupportedDataVersion(_)
            | WorldInfoError::UnsupportedLevelVersion(_)),
        ) => {
            error!("Failed to load world info!");
            error!("{error}");
            error!("Unsupported world version! See the logs for more info.");
            return Err(error);
        }
        Err(error) => {
            error!("Failed to load the world data in {}!", world_path.display());
            error!("{error}");
            error!(
                "Refusing to continue: a default world would generate different terrain on top of the existing region files. Restore {LEVEL_DAT_FILE_NAME} from {LEVEL_DAT_BACKUP_FILE_NAME}, which also holds a copy of the world seed, or move the world folder aside to start a new world."
            );
            error!("Failed to load the world data! See the logs for more info.");
            return Err(error);
        }
    };
    Ok(level_info)
}

/// The overworld, then the nether and the end when the configuration allows them.
pub(super) fn enabled_dimensions(basic_config: &BasicConfiguration) -> Vec<Dimension> {
    let dimensions = {
        let mut dimensions = vec![Dimension::OVERWORLD];
        if basic_config.allow_nether {
            dimensions.push(Dimension::THE_NETHER);
        }
        if basic_config.allow_end {
            dimensions.push(Dimension::THE_END);
        }
        dimensions
    };
    info!(
        "Enabled dimensions: {:?}",
        dimensions
            .iter()
            .map(|d| d.minecraft_name)
            .collect::<Vec<_>>()
    );
    dimensions
}

/// The simulation pool (all cores but the reserved ones), its one-thread sequential twin, and
/// the compute pool (half the cores, 2 to 8).
pub(super) fn build_pools() -> Result<TickPools, WorldInfoError> {
    let cpus = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    // Two cores stay free for the rest of the machine; see `kvasir-tickmode`.
    let tick_threads = kvasir_tickmode::pool_threads(
        cpus,
        kvasir_tickmode::reserved_cores(std::env::var("KVASIR_RESERVED_CORES").ok().as_deref()),
    );
    let simulation_pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(tick_threads)
            .thread_name(|i| format!("Simulation-{i}"))
            .build()
            .map_err(|e| WorldInfoError::IoError(std::io::Error::other(e)))?,
    );
    let sequential_pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .thread_name(|_| "Simulation-seq".to_owned())
            .build()
            .map_err(|e| WorldInfoError::IoError(std::io::Error::other(e)))?,
    );
    let compute_threads = (cpus / 2).clamp(2, 8);
    let compute_pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(compute_threads)
            .thread_name(|i| format!("Compute-{i}"))
            .build()
            .map_err(|e| WorldInfoError::IoError(std::io::Error::other(e)))?,
    );
    Ok(TickPools {
        simulation: simulation_pool,
        sequential: sequential_pool,
        compute: compute_pool,
    })
}

/// The tick profile, with per-kind timing (`KVASIR_TICK_KINDS`) and the tick trace
/// (`KVASIR_TICK_TRACE`) switched on from the environment.
pub(super) fn tick_profile() -> kvasir_tickprof::TickProfile {
    let profile = kvasir_tickprof::TickProfile::new();
    // KVASIR_TICK_KINDS=1: time every entity's tick, per kind (see /tickprofile).
    if std::env::var_os("KVASIR_TICK_KINDS").is_some() {
        profile.kinds.enable();
        kvasir_tickprof::parts::enable();
        kvasir_tickprof::motion::enable();
    }
    // KVASIR_TICK_TRACE=<file>: one line per tick, for finding what makes ticks slow.
    if let Some(path) = std::env::var_os("KVASIR_TICK_TRACE")
        && let Err(error) = profile.start_trace(std::path::Path::new(&path))
    {
        tracing::warn!("cannot write the tick trace: {error}");
    }
    profile
}

impl Server {
    /// Background tasks: the server key pair, and, when enabled, Mojang's chat-report keys
    /// and the Bedrock OIDC keys.
    pub(super) fn spawn_key_fetches(self: &Arc<Self>) {
        let server = self;
        // Fetch / generate keys in background tasks to avoid blocking startup
        let server_clone = server.clone();
        server.spawn_task(async move {
            let key_store = Arc::new(KeyStore::new());
            let _ = server_clone.key_store.set(key_store);
        });

        if server.basic_config.allow_chat_reports {
            let server_clone = server.clone();
            server.spawn_task(async move {
                let auth_config = server_clone
                    .advanced_config
                    .networking
                    .java
                    .authentication
                    .clone();
                let keys = fetch_mojang_public_keys(&auth_config)
                    .await
                    .unwrap_or_else(|e| {
                        error!("Failed to fetch Mojang keys: {e}");
                        Vec::new()
                    });
                server_clone.mojang_public_keys.store(Arc::new(keys));
            });
        }

        if server.advanced_config.networking.bedrock.online_mode
            && server
                .advanced_config
                .networking
                .bedrock
                .authentication
                .enabled
        {
            let server_clone = server.clone();
            server.spawn_task(async move {
                let auth = server_clone
                    .advanced_config
                    .networking
                    .bedrock
                    .authentication
                    .clone();
                let keys = match pumpkin_auth::jwt::fetch_oidc_jwks(
                    auth.url.as_deref(),
                    auth.connect_timeout,
                    auth.read_timeout,
                )
                .await
                {
                    Ok(keys) => keys,
                    Err(error) => {
                        error!("Failed to fetch Bedrock OIDC keys: {error}");
                        (String::new(), pumpkin_auth::jwt::Jwks { keys: Vec::new() })
                    }
                };
                let _ = server_clone.bedrock_oidc_keys.set(keys);
            });
        }
    }

    /// Loads one world per enabled dimension, fires their init and load events, then makes
    /// them the server's worlds.
    pub(super) async fn load_worlds(
        self: &Arc<Self>,
        world_path: &std::path::Path,
        seed: i64,
        block_registry: &Arc<BlockRegistry>,
    ) {
        let server = self;
        let mut worlds_vec = Vec::new();
        for dim in &server.dimensions {
            info!(
                "Loading {}",
                TextComponent::text(dim.minecraft_name.to_string())
                    .color_named(NamedColor::DarkGreen)
                    .to_pretty_console()
            );
            let config = Arc::new(server.advanced_config.world.clone());
            let level = into_level(dim.clone(), &config, world_path.to_path_buf(), seed);
            let world = Arc::new(World::load(
                level.clone(),
                server.level_info.clone(),
                dim.clone(),
                block_registry.clone(),
                Arc::downgrade(server),
            ));
            let portal: Arc<dyn WorldPortalExt> = Arc::new(WorldPortal(world.clone()));
            level.world_portal.store(Arc::new(Some(portal)));
            worlds_vec.push(world);
        }

        for world in &worlds_vec {
            let mut world_init_event =
                crate::plugin::api::events::world::world_init::WorldInitEvent::new(world.clone());
            server
                .plugin_manager
                .fire(server, &mut world_init_event)
                .await;

            let mut world_load_event =
                crate::plugin::api::events::world::world_load::WorldLoadEvent::new(world.clone());
            server
                .plugin_manager
                .fire(server, &mut world_load_event)
                .await;
        }

        server.worlds.store(Arc::new(worlds_vec));

        info!("All worlds loaded successfully.");
    }
}
