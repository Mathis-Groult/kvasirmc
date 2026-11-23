//! Golden for `register_world_event`: every WIT event type is registered on a fresh plugin
//! context with a priority and blocking flag that depend on its position, and the one handler
//! the registration added (event name, priority, blocking) is printed per event type. A type the
//! function does not route prints `none`. Bless with `BLESS_REGISTER_WORLD_EVENT=1`.

use std::{
    collections::HashMap,
    fmt::Write as _,
    sync::{Arc, OnceLock},
};

use pumpkin_config::{AdvancedConfiguration, BasicConfiguration, TelemetryConfig};
use pumpkin_core::{
    data::VanillaData,
    plugin::{
        EventPriority, PluginManager,
        api::{Context, PluginMetadata},
    },
    server::Server,
};
use pumpkin_wasm_host_common::{
    concurrent_store::{LegacySyncReentry, StoreMode, TokioSpawner, start_legacy_store},
    plugin::{PluginApiVersion, WasmPlugin},
    scheduler::TaskScheduler,
    state::PluginHostState,
};
use wasmtime::{Config, Engine, Store};

use crate::context::register_world_event;
use crate::{events::WasmPluginEventHandler, pumpkin::plugin::event::EventType};

const EVENT_TYPES: [EventType; 273] = [
    EventType::PlayerJoinEvent,
    EventType::PlayerLeaveEvent,
    EventType::PlayerLoginEvent,
    EventType::PlayerChatEvent,
    EventType::PlayerCommandSendEvent,
    EventType::PlayerPermissionCheckEvent,
    EventType::PlayerMoveEvent,
    EventType::PlayerTeleportEvent,
    EventType::PlayerChangeWorldEvent,
    EventType::PlayerRespawnEvent,
    EventType::PlayerExpChangeEvent,
    EventType::PlayerItemHeldEvent,
    EventType::PlayerChangedMainHandEvent,
    EventType::PlayerGamemodeChangeEvent,
    EventType::PlayerCustomPayloadEvent,
    EventType::PlayerFishEvent,
    EventType::PlayerEggThrowEvent,
    EventType::PlayerInteractUnknownEntityEvent,
    EventType::PlayerInteractEntityEvent,
    EventType::PlayerInteractEvent,
    EventType::PlayerToggleSneakEvent,
    EventType::PlayerToggleFlightEvent,
    EventType::PlayerToggleSprintEvent,
    EventType::InventoryClickEvent,
    EventType::InventoryCloseEvent,
    EventType::BlockRedstoneEvent,
    EventType::BlockBreakEvent,
    EventType::BlockBurnEvent,
    EventType::BlockCanBuildEvent,
    EventType::BlockGrowEvent,
    EventType::BlockPlaceEvent,
    EventType::BedrockFormResponseEvent,
    EventType::DialogClickActionEvent,
    EventType::DialogShowEvent,
    EventType::DialogClearEvent,
    EventType::ServerCommandEvent,
    EventType::ServerListPingEvent,
    EventType::ServerLoadEvent,
    EventType::SpawnChangeEvent,
    EventType::ServerBroadcastEvent,
    EventType::ServerTickStartEvent,
    EventType::ServerTickEndEvent,
    EventType::PacketReceivedEvent,
    EventType::PacketSentEvent,
    EventType::ChunkLoadEvent,
    EventType::ChunkSaveEvent,
    EventType::ChunkSendEvent,
    EventType::EntityDamageEvent,
    EventType::EntityDeathEvent,
    EventType::PlayerDeathEvent,
    EventType::EntitySpawnEvent,
    EventType::EntityCombustEvent,
    EventType::EntityRegainHealthEvent,
    EventType::EntityAirChangeEvent,
    EventType::EntityBreedEvent,
    EventType::EntityDismountEvent,
    EventType::EntityDyeEvent,
    EventType::EntityEnterLoveModeEvent,
    EventType::EntityExplodeEvent,
    EventType::EntityMountEvent,
    EventType::EntityPickupItemEvent,
    EventType::EntityPortalEvent,
    EventType::EntityResurrectEvent,
    EventType::EntityShootBowEvent,
    EventType::EntityTameEvent,
    EventType::EntityTargetEvent,
    EventType::EntityTeleportEvent,
    EventType::EntityToggleGlideEvent,
    EventType::EntityTransformEvent,
    EventType::PlayerItemConsumeEvent,
    EventType::PlayerItemDamageEvent,
    EventType::PlayerDropItemEvent,
    EventType::PlayerBedEnterEvent,
    EventType::PlayerBedLeaveEvent,
    EventType::PlayerBucketEmptyEvent,
    EventType::PlayerBucketFillEvent,
    EventType::BlockDamageEvent,
    EventType::BlockIgniteEvent,
    EventType::BlockFromToEvent,
    EventType::BlockFormEvent,
    EventType::BlockFadeEvent,
    EventType::BlockDispenseEvent,
    EventType::BlockExplodeEvent,
    EventType::BlockPhysicsEvent,
    EventType::BlockPistonExtendEvent,
    EventType::BlockPistonRetractEvent,
    EventType::NotePlayEvent,
    EventType::SignChangeEvent,
    EventType::SpongeAbsorbEvent,
    EventType::TntPrimeEvent,
    EventType::WeatherChangeEvent,
    EventType::ThunderChangeEvent,
    EventType::WorldLoadEvent,
    EventType::WorldUnloadEvent,
    EventType::AsyncStructureGenerateEvent,
    EventType::AsyncStructureSpawnEvent,
    EventType::ChunkPopulateEvent,
    EventType::ChunkUnloadEvent,
    EventType::EntitiesLoadEvent,
    EventType::EntitiesUnloadEvent,
    EventType::GenericGameEvent,
    EventType::LootGenerateEvent,
    EventType::PortalCreateEvent,
    EventType::StructureGrowEvent,
    EventType::TimeSkipEvent,
    EventType::WorldInitEvent,
    EventType::WorldSaveEvent,
    EventType::InventoryOpenEvent,
    EventType::InventoryDragEvent,
    EventType::CraftItemEvent,
    EventType::FurnaceSmeltEvent,
    EventType::BrewEvent,
    EventType::BrewingStandFuelEvent,
    EventType::FurnaceBurnEvent,
    EventType::FurnaceExtractEvent,
    EventType::FurnaceStartSmeltEvent,
    EventType::HopperInventorySearchEvent,
    EventType::InventoryCreativeEvent,
    EventType::InventoryInteractEvent,
    EventType::InventoryMoveItemEvent,
    EventType::InventoryPickupItemEvent,
    EventType::PrepareAnvilEvent,
    EventType::PrepareGrindstoneEvent,
    EventType::PrepareInventoryResultEvent,
    EventType::PrepareItemCraftEvent,
    EventType::PrepareSmithingEvent,
    EventType::SmithItemEvent,
    EventType::TradeSelectEvent,
    EventType::VehicleBlockCollisionEvent,
    EventType::VehicleCollisionEvent,
    EventType::VehicleCreateEvent,
    EventType::VehicleDamageEvent,
    EventType::VehicleDestroyEvent,
    EventType::VehicleEnterEvent,
    EventType::VehicleEntityCollisionEvent,
    EventType::VehicleExitEvent,
    EventType::VehicleMoveEvent,
    EventType::VehicleUpdateEvent,
    EventType::PrepareItemEnchantEvent,
    EventType::EnchantItemEvent,
    EventType::MapInitializeEvent,
    EventType::HangingBreakEvent,
    EventType::HangingBreakByEntityEvent,
    EventType::HangingPlaceEvent,
    EventType::BellResonateEvent,
    EventType::BellRingEvent,
    EventType::BlockBrushEvent,
    EventType::BlockCookEvent,
    EventType::BlockDamageAbortEvent,
    EventType::BlockDispenseArmorEvent,
    EventType::BlockDispenseLootEvent,
    EventType::BlockDropItemEvent,
    EventType::BlockExpEvent,
    EventType::BlockFertilizeEvent,
    EventType::BlockMultiPlaceEvent,
    EventType::BlockReceiveGameEvent,
    EventType::BlockShearEntityEvent,
    EventType::BlockSpreadEvent,
    EventType::BrewingStartEvent,
    EventType::CampfireStartEvent,
    EventType::CauldronLevelChangeEvent,
    EventType::CrafterCraftEvent,
    EventType::EntityBlockFormEvent,
    EventType::FluidLevelChangeEvent,
    EventType::InventoryBlockStartEvent,
    EventType::LeavesDecayEvent,
    EventType::MoistureChangeEvent,
    EventType::SculkBloomEvent,
    EventType::VaultDisplayItemEvent,
    EventType::CreatureSpawnEvent,
    EventType::EnderDragonChangePhaseEvent,
    EventType::EntityBreakDoorEvent,
    EventType::EntityChangeBlockEvent,
    EventType::EntityDamageByBlockEvent,
    EventType::EntityDamageByEntityEvent,
    EventType::EntityDropItemEvent,
    EventType::EntityEnterBlockEvent,
    EventType::EntityExhaustionEvent,
    EventType::EntityInteractEvent,
    EventType::EntityKnockbackEvent,
    EventType::EntityPlaceEvent,
    EventType::EntityPoseChangeEvent,
    EventType::EntityPotionEffectEvent,
    EventType::EntitySpellCastEvent,
    EventType::EntityTargetLivingEntityEvent,
    EventType::EntityToggleSwimEvent,
    EventType::ExplosionPrimeEvent,
    EventType::FireworkExplodeEvent,
    EventType::FoodLevelChangeEvent,
    EventType::ItemDespawnEvent,
    EventType::ItemMergeEvent,
    EventType::ItemSpawnEvent,
    EventType::PiglinBarterEvent,
    EventType::ProjectileHitEvent,
    EventType::ProjectileLaunchEvent,
    EventType::SheepDyeWoolEvent,
    EventType::SheepRegrowWoolEvent,
    EventType::SlimeSplitEvent,
    EventType::StriderTemperatureChangeEvent,
    EventType::VillagerAcquireTradeEvent,
    EventType::VillagerCareerChangeEvent,
    EventType::VillagerReplenishTradeEvent,
    EventType::WardenAngerChangeEvent,
    EventType::AreaEffectCloudApplyEvent,
    EventType::ArrowBodyCountChangeEvent,
    EventType::BatToggleSleepEvent,
    EventType::CreeperPowerEvent,
    EventType::EntityCombustByBlockEvent,
    EventType::EntityCombustByEntityEvent,
    EventType::EntityKnockbackByEntityEvent,
    EventType::EntityPortalEnterEvent,
    EventType::EntityPortalExitEvent,
    EventType::EntityRemoveEvent,
    EventType::EntityTargetBlockEvent,
    EventType::EntityUnleashEvent,
    EventType::ExpBottleEvent,
    EventType::HorseJumpEvent,
    EventType::LingeringPotionSplashEvent,
    EventType::PigZapEvent,
    EventType::PigZombieAngerEvent,
    EventType::PotionSplashEvent,
    EventType::SpawnerSpawnEvent,
    EventType::TrialSpawnerSpawnEvent,
    EventType::VillagerReputationChangeEvent,
    EventType::AsyncPlayerChatEvent,
    EventType::AsyncPlayerPreLoginEvent,
    EventType::PlayerAdvancementDoneEvent,
    EventType::PlayerAnimationEvent,
    EventType::PlayerArmorStandManipulateEvent,
    EventType::PlayerBucketEntityEvent,
    EventType::PlayerChangedWorldEvent,
    EventType::PlayerChannelEvent,
    EventType::PlayerCommandPreprocessEvent,
    EventType::PlayerEditBookEvent,
    EventType::PlayerElytraBoostEvent,
    EventType::PlayerExpCooldownChangeEvent,
    EventType::PlayerHarvestBlockEvent,
    EventType::PlayerHideEntityEvent,
    EventType::PlayerItemBreakEvent,
    EventType::PlayerItemMendEvent,
    EventType::PlayerKickEvent,
    EventType::PlayerLeashEntityEvent,
    EventType::PlayerLevelChangeEvent,
    EventType::PlayerLocaleChangeEvent,
    EventType::PlayerNameEntityEvent,
    EventType::PlayerOpenSignEvent,
    EventType::PlayerPortalEvent,
    EventType::PlayerPreLoginEvent,
    EventType::PlayerRiptideEvent,
    EventType::PlayerShearEntityEvent,
    EventType::PlayerShowEntityEvent,
    EventType::PlayerSpawnChangeEvent,
    EventType::PlayerStatisticIncrementEvent,
    EventType::PlayerSwapHandsEvent,
    EventType::PlayerTakeLecternBookEvent,
    EventType::PlayerUnleashEntityEvent,
    EventType::PlayerVelocityEvent,
    EventType::PlayerInputEvent,
    EventType::PlayerInteractAtEntityEvent,
    EventType::PlayerLinksSendEvent,
    EventType::PlayerPickupArrowEvent,
    EventType::PlayerRecipeBookClickEvent,
    EventType::PlayerRecipeBookSettingsChangeEvent,
    EventType::PlayerRecipeDiscoverEvent,
    EventType::PlayerRegisterChannelEvent,
    EventType::PlayerResourcePackStatusEvent,
    EventType::PlayerSpawnLocationEvent,
    EventType::PlayerUnregisterChannelEvent,
    EventType::RaidFinishEvent,
    EventType::RaidSpawnWaveEvent,
    EventType::RaidStopEvent,
    EventType::RaidTriggerEvent,
    EventType::LightningStrikeEvent,
];

const GOLDEN: &str = include_str!("register_world_event.txt");

fn priority(index: usize) -> EventPriority {
    match index % 5 {
        0 => EventPriority::Highest,
        1 => EventPriority::High,
        2 => EventPriority::Normal,
        3 => EventPriority::Low,
        _ => EventPriority::Lowest,
    }
}

async fn server(folder: &tempfile::TempDir) -> Arc<Server> {
    let basic = BasicConfiguration {
        default_level_name: folder.path().join("world").to_string_lossy().into_owned(),
        ..BasicConfiguration::default()
    };
    let telemetry = TelemetryConfig {
        enabled: false,
        ..TelemetryConfig::default()
    };
    Server::new(
        basic,
        AdvancedConfiguration::default(),
        telemetry,
        VanillaData::default(),
        Vec::new(),
    )
    .await
    .expect("a server starts on an empty folder")
}

async fn plugin() -> Arc<WasmPlugin> {
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.concurrency_support(false);
    let engine = Engine::new(&config).expect("engine");
    let store = start_legacy_store(
        Store::new(&engine, PluginHostState::new()),
        LegacySyncReentry::new(),
        Arc::new(TokioSpawner::new(tokio::runtime::Handle::current())),
        StoreMode::Sequential,
    )
    .await
    .expect("store");
    Arc::new(WasmPlugin {
        plugin_instance: Arc::new(()),
        api_version: PluginApiVersion::V0_1,
        store,
        task_scheduler: Arc::new(TaskScheduler::new()),
    })
}

fn metadata() -> PluginMetadata {
    PluginMetadata {
        name: "golden".to_string(),
        version: "1".to_string(),
        authors: Vec::new(),
        description: String::new(),
        dependencies: Vec::new(),
        permissions: Vec::new(),
    }
}

/// Registers every event type in order and describes what each registration added.
async fn record() -> String {
    let folder = tempfile::tempdir().expect("temp dir");
    let server = server(&folder).await;
    let manager = Arc::new(PluginManager::new(Vec::new()));
    let ctx = Context::new(
        metadata(),
        Arc::clone(&server),
        Arc::clone(&manager.handlers),
        Arc::clone(&manager),
        Arc::new(OnceLock::new()),
    );
    let handler = Arc::new(WasmPluginEventHandler {
        handler_id: 7,
        plugin: plugin().await,
    });
    let mut counts: HashMap<&'static str, usize> = HashMap::new();
    let mut out = String::new();
    for (index, event_type) in EVENT_TYPES.iter().enumerate() {
        let blocking = index % 2 == 0;
        register_world_event(&ctx, &handler, priority(index), blocking, *event_type);
        let map = manager.handlers.load();
        let mut added = Vec::new();
        for (name, handlers) in map.iter() {
            let before = counts.insert(name, handlers.len()).unwrap_or(0);
            for h in &handlers[before..] {
                added.push(format!(
                    "{name} {:?} blocking={} source={:?}",
                    h.get_priority(),
                    h.is_blocking(),
                    h.source()
                ));
            }
        }
        added.sort();
        let line = if added.is_empty() {
            "none".to_string()
        } else {
            added.join(" | ")
        };
        writeln!(out, "{event_type:?} => {line}").expect("write to a String");
    }
    server.stop();
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn register_world_event_golden() {
    let got = record().await;
    if std::env::var_os("BLESS_REGISTER_WORLD_EVENT").is_some() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/register_world_event.txt");
        std::fs::write(path, &got).expect("write golden");
        return;
    }
    assert_eq!(
        got, GOLDEN,
        "register_world_event changed which handler it registers"
    );
}
