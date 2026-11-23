use std::{collections::HashMap, sync::Arc};
use wasmtime::component::Resource;

use crate::{
    events::{ToFromWasmEvent, WasmPluginEventHandler},
    pumpkin::{
        self,
        plugin::{
            command::Command,
            context::{Context as WitContext, MarketplaceMetadata},
            event::{EventPriority, EventType},
            permission::{Permission, PermissionDefault, PermissionLevel},
            server::Server,
        },
    },
};
use pumpkin_core::plugin::Context;
use pumpkin_wasm_host_common::state::PluginHostState;

fn register_typed_event<E: pumpkin_core::plugin::Payload + ToFromWasmEvent + Clone + 'static>(
    ctx: &Context,
    handler: &Arc<WasmPluginEventHandler>,
    priority: pumpkin_core::plugin::EventPriority,
    blocking: bool,
) {
    ctx.register_event::<E, _>(Arc::clone(handler), priority, blocking);
}

mod player_events;
pub(crate) use player_events::register_player_event;
mod world_events;
pub(crate) use world_events::register_world_event;

#[allow(clippy::too_many_lines)]
fn register_entity_event(
    ctx: &Context,
    handler: &Arc<WasmPluginEventHandler>,
    priority: pumpkin_core::plugin::EventPriority,
    blocking: bool,
    event_type: EventType,
) {
    use pumpkin_core::plugin::entity::{
        area_effect_cloud_apply::AreaEffectCloudApplyEvent,
        arrow_body_count_change::ArrowBodyCountChangeEvent,
        bat_toggle_sleep::BatToggleSleepEvent,
        creature_spawn::CreatureSpawnEvent,
        creeper_power::CreeperPowerEvent,
        ender_dragon_change_phase::EnderDragonChangePhaseEvent,
        entity_air_change::EntityAirChangeEvent,
        entity_break_door::EntityBreakDoorEvent,
        entity_breed::EntityBreedEvent,
        entity_change_block::EntityChangeBlockEvent,
        entity_combust::EntityCombustEvent,
        entity_combust_by_block::EntityCombustByBlockEvent,
        entity_combust_by_entity::EntityCombustByEntityEvent,
        entity_damage::EntityDamageEvent,
        entity_damage_by_block::EntityDamageByBlockEvent,
        entity_damage_by_entity::EntityDamageByEntityEvent,
        entity_death::{EntityDeathEvent, PlayerDeathEvent},
        entity_dismount::EntityDismountEvent,
        entity_drop_item::EntityDropItemEvent,
        entity_dye::EntityDyeEvent,
        entity_enter_block::EntityEnterBlockEvent,
        entity_enter_love_mode::EntityEnterLoveModeEvent,
        entity_exhaustion::EntityExhaustionEvent,
        entity_explode::EntityExplodeEvent,
        entity_interact::EntityInteractEvent,
        entity_knockback::EntityKnockbackEvent,
        entity_knockback_by_entity::EntityKnockbackByEntityEvent,
        entity_mount::EntityMountEvent,
        entity_pickup_item::EntityPickupItemEvent,
        entity_place::EntityPlaceEvent,
        entity_portal::EntityPortalEvent,
        entity_portal_enter::EntityPortalEnterEvent,
        entity_portal_exit::EntityPortalExitEvent,
        entity_pose_change::EntityPoseChangeEvent,
        entity_potion_effect::EntityPotionEffectEvent,
        entity_regain_health::EntityRegainHealthEvent,
        entity_remove::EntityRemoveEvent,
        entity_resurrect::EntityResurrectEvent,
        entity_shoot_bow::EntityShootBowEvent,
        entity_spawn::EntitySpawnEvent,
        entity_spell_cast::EntitySpellCastEvent,
        entity_tame::EntityTameEvent,
        entity_target::EntityTargetEvent,
        entity_target_block::EntityTargetBlockEvent,
        entity_target_living_entity::EntityTargetLivingEntityEvent,
        entity_teleport::EntityTeleportEvent,
        entity_toggle_glide::EntityToggleGlideEvent,
        entity_toggle_swim::EntityToggleSwimEvent,
        entity_transform::EntityTransformEvent,
        entity_unleash::EntityUnleashEvent,
        exp_bottle::ExpBottleEvent,
        explosion_prime::ExplosionPrimeEvent,
        firework_explode::FireworkExplodeEvent,
        food_level_change::FoodLevelChangeEvent,
        horse_jump::HorseJumpEvent,
        item_despawn::ItemDespawnEvent,
        item_merge::ItemMergeEvent,
        item_spawn::ItemSpawnEvent,
        lingering_potion_splash::LingeringPotionSplashEvent,
        pig_zap::PigZapEvent,
        pig_zombie_anger::PigZombieAngerEvent,
        piglin_barter::PiglinBarterEvent,
        potion_splash::PotionSplashEvent,
        projectile_hit::ProjectileHitEvent,
        projectile_launch::ProjectileLaunchEvent,
        sheep_dye_wool::SheepDyeWoolEvent,
        sheep_regrow_wool::SheepRegrowWoolEvent,
        slime_split::SlimeSplitEvent,
        spawner_spawn::SpawnerSpawnEvent,
        strider_temperature_change::StriderTemperatureChangeEvent,
        trial_spawner_spawn::TrialSpawnerSpawnEvent,
        villager_acquire_trade::VillagerAcquireTradeEvent,
        villager_career_change::VillagerCareerChangeEvent,
        villager_replenish_trade::VillagerReplenishTradeEvent,
        villager_reputation_change::VillagerReputationChangeEvent,
        warden_anger_change::WardenAngerChangeEvent,
    };

    macro_rules! register {
        ($event:ty) => {
            register_typed_event::<$event>(ctx, handler, priority, blocking)
        };
    }

    match event_type {
        EventType::EntityDamageEvent => register!(EntityDamageEvent),
        EventType::EntityDeathEvent => register!(EntityDeathEvent),
        EventType::PlayerDeathEvent => register!(PlayerDeathEvent),
        EventType::EntitySpawnEvent => register!(EntitySpawnEvent),
        EventType::EntityCombustEvent => register!(EntityCombustEvent),
        EventType::EntityRegainHealthEvent => register!(EntityRegainHealthEvent),
        EventType::EntityAirChangeEvent => register!(EntityAirChangeEvent),
        EventType::EntityBreedEvent => register!(EntityBreedEvent),
        EventType::EntityDismountEvent => register!(EntityDismountEvent),
        EventType::EntityDyeEvent => register!(EntityDyeEvent),
        EventType::EntityEnterLoveModeEvent => register!(EntityEnterLoveModeEvent),
        EventType::EntityExplodeEvent => register!(EntityExplodeEvent),
        EventType::EntityMountEvent => register!(EntityMountEvent),
        EventType::EntityPickupItemEvent => register!(EntityPickupItemEvent),
        EventType::EntityPortalEvent => register!(EntityPortalEvent),
        EventType::EntityResurrectEvent => register!(EntityResurrectEvent),
        EventType::EntityShootBowEvent => register!(EntityShootBowEvent),
        EventType::EntityTameEvent => register!(EntityTameEvent),
        EventType::EntityTargetEvent => register!(EntityTargetEvent),
        EventType::EntityTeleportEvent => register!(EntityTeleportEvent),
        EventType::EntityToggleGlideEvent => register!(EntityToggleGlideEvent),
        EventType::EntityTransformEvent => register!(EntityTransformEvent),
        EventType::CreatureSpawnEvent => register!(CreatureSpawnEvent),
        EventType::EnderDragonChangePhaseEvent => register!(EnderDragonChangePhaseEvent),
        EventType::EntityBreakDoorEvent => register!(EntityBreakDoorEvent),
        EventType::EntityChangeBlockEvent => register!(EntityChangeBlockEvent),
        EventType::EntityDamageByBlockEvent => register!(EntityDamageByBlockEvent),
        EventType::EntityDamageByEntityEvent => register!(EntityDamageByEntityEvent),
        EventType::EntityDropItemEvent => register!(EntityDropItemEvent),
        EventType::EntityEnterBlockEvent => register!(EntityEnterBlockEvent),
        EventType::EntityExhaustionEvent => register!(EntityExhaustionEvent),
        EventType::EntityInteractEvent => register!(EntityInteractEvent),
        EventType::EntityKnockbackEvent => register!(EntityKnockbackEvent),
        EventType::EntityPlaceEvent => register!(EntityPlaceEvent),
        EventType::EntityPoseChangeEvent => register!(EntityPoseChangeEvent),
        EventType::EntityPotionEffectEvent => register!(EntityPotionEffectEvent),
        EventType::EntitySpellCastEvent => register!(EntitySpellCastEvent),
        EventType::EntityTargetLivingEntityEvent => register!(EntityTargetLivingEntityEvent),
        EventType::EntityToggleSwimEvent => register!(EntityToggleSwimEvent),
        EventType::ExplosionPrimeEvent => register!(ExplosionPrimeEvent),
        EventType::FireworkExplodeEvent => register!(FireworkExplodeEvent),
        EventType::FoodLevelChangeEvent => register!(FoodLevelChangeEvent),
        EventType::ItemDespawnEvent => register!(ItemDespawnEvent),
        EventType::ItemMergeEvent => register!(ItemMergeEvent),
        EventType::ItemSpawnEvent => register!(ItemSpawnEvent),
        EventType::PiglinBarterEvent => register!(PiglinBarterEvent),
        EventType::ProjectileHitEvent => register!(ProjectileHitEvent),
        EventType::ProjectileLaunchEvent => register!(ProjectileLaunchEvent),
        EventType::SheepDyeWoolEvent => register!(SheepDyeWoolEvent),
        EventType::SheepRegrowWoolEvent => register!(SheepRegrowWoolEvent),
        EventType::SlimeSplitEvent => register!(SlimeSplitEvent),
        EventType::StriderTemperatureChangeEvent => register!(StriderTemperatureChangeEvent),
        EventType::VillagerAcquireTradeEvent => register!(VillagerAcquireTradeEvent),
        EventType::VillagerCareerChangeEvent => register!(VillagerCareerChangeEvent),
        EventType::VillagerReplenishTradeEvent => register!(VillagerReplenishTradeEvent),
        EventType::WardenAngerChangeEvent => register!(WardenAngerChangeEvent),
        EventType::AreaEffectCloudApplyEvent => register!(AreaEffectCloudApplyEvent),
        EventType::ArrowBodyCountChangeEvent => register!(ArrowBodyCountChangeEvent),
        EventType::BatToggleSleepEvent => register!(BatToggleSleepEvent),
        EventType::CreeperPowerEvent => register!(CreeperPowerEvent),
        EventType::EntityCombustByBlockEvent => register!(EntityCombustByBlockEvent),
        EventType::EntityCombustByEntityEvent => register!(EntityCombustByEntityEvent),
        EventType::EntityKnockbackByEntityEvent => register!(EntityKnockbackByEntityEvent),
        EventType::EntityPortalEnterEvent => register!(EntityPortalEnterEvent),
        EventType::EntityPortalExitEvent => register!(EntityPortalExitEvent),
        EventType::EntityRemoveEvent => register!(EntityRemoveEvent),
        EventType::EntityTargetBlockEvent => register!(EntityTargetBlockEvent),
        EventType::EntityUnleashEvent => register!(EntityUnleashEvent),
        EventType::ExpBottleEvent => register!(ExpBottleEvent),
        EventType::HorseJumpEvent => register!(HorseJumpEvent),
        EventType::LingeringPotionSplashEvent => register!(LingeringPotionSplashEvent),
        EventType::PigZapEvent => register!(PigZapEvent),
        EventType::PigZombieAngerEvent => register!(PigZombieAngerEvent),
        EventType::PotionSplashEvent => register!(PotionSplashEvent),
        EventType::SpawnerSpawnEvent => register!(SpawnerSpawnEvent),
        EventType::TrialSpawnerSpawnEvent => register!(TrialSpawnerSpawnEvent),
        EventType::VillagerReputationChangeEvent => register!(VillagerReputationChangeEvent),
        _ => {
            tracing::error!("non-entity event should not be routed to register_entity_event");
        }
    }
}

fn register_inventory_event(
    resource: &Context,
    handler: &Arc<WasmPluginEventHandler>,
    priority: pumpkin_core::plugin::EventPriority,
    blocking: bool,
    event_type: EventType,
) {
    use pumpkin_core::plugin::inventory::{
        brew::BrewEvent, brewing_stand_fuel::BrewingStandFuelEvent, craft_item::CraftItemEvent,
        furnace_burn::FurnaceBurnEvent, furnace_extract::FurnaceExtractEvent,
        furnace_smelt::FurnaceSmeltEvent, furnace_start_smelt::FurnaceStartSmeltEvent,
        hopper_inventory_search::HopperInventorySearchEvent,
        inventory_creative::InventoryCreativeEvent, inventory_drag::InventoryDragEvent,
        inventory_interact::InventoryInteractEvent, inventory_move_item::InventoryMoveItemEvent,
        inventory_open::InventoryOpenEvent, inventory_pickup_item::InventoryPickupItemEvent,
        prepare_anvil::PrepareAnvilEvent, prepare_grindstone::PrepareGrindstoneEvent,
        prepare_inventory_result::PrepareInventoryResultEvent,
        prepare_item_craft::PrepareItemCraftEvent, prepare_smithing::PrepareSmithingEvent,
        smith_item::SmithItemEvent, trade_select::TradeSelectEvent,
    };

    match event_type {
        EventType::InventoryOpenEvent => {
            register_typed_event::<InventoryOpenEvent>(resource, handler, priority, blocking);
        }
        EventType::InventoryDragEvent => {
            register_typed_event::<InventoryDragEvent>(resource, handler, priority, blocking);
        }
        EventType::CraftItemEvent => {
            register_typed_event::<CraftItemEvent>(resource, handler, priority, blocking);
        }
        EventType::FurnaceSmeltEvent => {
            register_typed_event::<FurnaceSmeltEvent>(resource, handler, priority, blocking);
        }
        EventType::BrewEvent => {
            register_typed_event::<BrewEvent>(resource, handler, priority, blocking);
        }
        EventType::BrewingStandFuelEvent => {
            register_typed_event::<BrewingStandFuelEvent>(resource, handler, priority, blocking);
        }
        EventType::FurnaceBurnEvent => {
            register_typed_event::<FurnaceBurnEvent>(resource, handler, priority, blocking);
        }
        EventType::FurnaceExtractEvent => {
            register_typed_event::<FurnaceExtractEvent>(resource, handler, priority, blocking);
        }
        EventType::FurnaceStartSmeltEvent => {
            register_typed_event::<FurnaceStartSmeltEvent>(resource, handler, priority, blocking);
        }
        EventType::HopperInventorySearchEvent => {
            register_typed_event::<HopperInventorySearchEvent>(
                resource, handler, priority, blocking,
            );
        }
        EventType::InventoryCreativeEvent => {
            register_typed_event::<InventoryCreativeEvent>(resource, handler, priority, blocking);
        }
        EventType::InventoryInteractEvent => {
            register_typed_event::<InventoryInteractEvent>(resource, handler, priority, blocking);
        }
        EventType::InventoryMoveItemEvent => {
            register_typed_event::<InventoryMoveItemEvent>(resource, handler, priority, blocking);
        }
        EventType::InventoryPickupItemEvent => {
            register_typed_event::<InventoryPickupItemEvent>(resource, handler, priority, blocking);
        }
        EventType::PrepareAnvilEvent => {
            register_typed_event::<PrepareAnvilEvent>(resource, handler, priority, blocking);
        }
        EventType::PrepareGrindstoneEvent => {
            register_typed_event::<PrepareGrindstoneEvent>(resource, handler, priority, blocking);
        }
        EventType::PrepareInventoryResultEvent => {
            register_typed_event::<PrepareInventoryResultEvent>(
                resource, handler, priority, blocking,
            );
        }
        EventType::PrepareItemCraftEvent => {
            register_typed_event::<PrepareItemCraftEvent>(resource, handler, priority, blocking);
        }
        EventType::PrepareSmithingEvent => {
            register_typed_event::<PrepareSmithingEvent>(resource, handler, priority, blocking);
        }
        EventType::SmithItemEvent => {
            register_typed_event::<SmithItemEvent>(resource, handler, priority, blocking);
        }
        EventType::TradeSelectEvent => {
            register_typed_event::<TradeSelectEvent>(resource, handler, priority, blocking);
        }
        _ => {
            tracing::error!("non-inventory event should not be routed to register_inventory_event");
        }
    }
}

fn register_vehicle_event(
    resource: &Context,
    handler: &Arc<WasmPluginEventHandler>,
    priority: pumpkin_core::plugin::EventPriority,
    blocking: bool,
    event_type: EventType,
) {
    use pumpkin_core::plugin::vehicle::{
        vehicle_block_collision::VehicleBlockCollisionEvent,
        vehicle_collision::VehicleCollisionEvent, vehicle_create::VehicleCreateEvent,
        vehicle_damage::VehicleDamageEvent, vehicle_destroy::VehicleDestroyEvent,
        vehicle_enter::VehicleEnterEvent, vehicle_entity_collision::VehicleEntityCollisionEvent,
        vehicle_exit::VehicleExitEvent, vehicle_move::VehicleMoveEvent,
        vehicle_update::VehicleUpdateEvent,
    };

    match event_type {
        EventType::VehicleBlockCollisionEvent => {
            register_typed_event::<VehicleBlockCollisionEvent>(
                resource, handler, priority, blocking,
            );
        }
        EventType::VehicleCollisionEvent => {
            register_typed_event::<VehicleCollisionEvent>(resource, handler, priority, blocking);
        }
        EventType::VehicleCreateEvent => {
            register_typed_event::<VehicleCreateEvent>(resource, handler, priority, blocking);
        }
        EventType::VehicleDamageEvent => {
            register_typed_event::<VehicleDamageEvent>(resource, handler, priority, blocking);
        }
        EventType::VehicleDestroyEvent => {
            register_typed_event::<VehicleDestroyEvent>(resource, handler, priority, blocking);
        }
        EventType::VehicleEnterEvent => {
            register_typed_event::<VehicleEnterEvent>(resource, handler, priority, blocking);
        }
        EventType::VehicleEntityCollisionEvent => {
            register_typed_event::<VehicleEntityCollisionEvent>(
                resource, handler, priority, blocking,
            );
        }
        EventType::VehicleExitEvent => {
            register_typed_event::<VehicleExitEvent>(resource, handler, priority, blocking);
        }
        EventType::VehicleMoveEvent => {
            register_typed_event::<VehicleMoveEvent>(resource, handler, priority, blocking);
        }
        EventType::VehicleUpdateEvent => {
            register_typed_event::<VehicleUpdateEvent>(resource, handler, priority, blocking);
        }
        _ => {
            tracing::error!("non-vehicle event should not be routed to register_vehicle_event");
        }
    }
}

fn register_enchantment_event(
    resource: &Context,
    handler: &Arc<WasmPluginEventHandler>,
    priority: pumpkin_core::plugin::EventPriority,
    blocking: bool,
    event_type: EventType,
) {
    use pumpkin_core::plugin::enchantment::{
        enchant_item::EnchantItemEvent, prepare_item_enchant::PrepareItemEnchantEvent,
    };

    match event_type {
        EventType::PrepareItemEnchantEvent => {
            register_typed_event::<PrepareItemEnchantEvent>(resource, handler, priority, blocking);
        }
        EventType::EnchantItemEvent => {
            register_typed_event::<EnchantItemEvent>(resource, handler, priority, blocking);
        }
        _ => {
            tracing::error!(
                "non-enchantment event should not be routed to register_enchantment_event"
            );
        }
    }
}

#[allow(clippy::too_many_lines)]
fn register_block_event(
    resource: &Context,
    handler: &Arc<WasmPluginEventHandler>,
    priority: pumpkin_core::plugin::EventPriority,
    blocking: bool,
    event_type: EventType,
) {
    use pumpkin_core::plugin::block::{
        block_break::BlockBreakEvent, block_burn::BlockBurnEvent,
        block_can_build::BlockCanBuildEvent, block_grow::BlockGrowEvent,
        block_place::BlockPlaceEvent, block_redstone::BlockRedstoneEvent,
    };

    match event_type {
        EventType::BlockRedstoneEvent => {
            register_typed_event::<BlockRedstoneEvent>(resource, handler, priority, blocking);
        }
        EventType::BlockBreakEvent => {
            register_typed_event::<BlockBreakEvent>(resource, handler, priority, blocking);
        }
        EventType::BlockBurnEvent => {
            register_typed_event::<BlockBurnEvent>(resource, handler, priority, blocking);
        }
        EventType::BlockCanBuildEvent => {
            register_typed_event::<BlockCanBuildEvent>(resource, handler, priority, blocking);
        }
        EventType::BlockGrowEvent => {
            register_typed_event::<BlockGrowEvent>(resource, handler, priority, blocking);
        }
        EventType::BlockPlaceEvent => {
            register_typed_event::<BlockPlaceEvent>(resource, handler, priority, blocking);
        }
        EventType::BlockDamageEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::block_damage::BlockDamageEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BlockIgniteEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::block_ignite::BlockIgniteEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BlockFromToEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::block_from_to::BlockFromToEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BlockFormEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::block_form::BlockFormEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BlockFadeEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::block_fade::BlockFadeEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BlockDispenseEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::block_dispense::BlockDispenseEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BlockExplodeEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::block_explode::BlockExplodeEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BlockPhysicsEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::block_physics::BlockPhysicsEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BlockPistonExtendEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::block_piston::BlockPistonExtendEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BlockPistonRetractEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::block_piston::BlockPistonRetractEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::NotePlayEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::note_play::NotePlayEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::SignChangeEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::sign_change::SignChangeEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::SpongeAbsorbEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::sponge_absorb::SpongeAbsorbEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::TntPrimeEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::tnt_prime::TNTPrimeEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BellResonateEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::bell_resonate::BellResonateEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BellRingEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::bell_ring::BellRingEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BlockBrushEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::block_brush::BlockBrushEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BlockCookEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::block_cook::BlockCookEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BlockDamageAbortEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::block_damage_abort::BlockDamageAbortEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BlockDispenseArmorEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::block_dispense_armor::BlockDispenseArmorEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BlockDispenseLootEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::block_dispense_loot::BlockDispenseLootEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BlockDropItemEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::block_drop_item::BlockDropItemEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BlockExpEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::block_exp::BlockExpEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BlockFertilizeEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::block_fertilize::BlockFertilizeEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BlockMultiPlaceEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::block_multi_place::BlockMultiPlaceEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BlockReceiveGameEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::block_receive_game::BlockReceiveGameEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BlockShearEntityEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::block_shear_entity::BlockShearEntityEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BlockSpreadEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::block_spread::BlockSpreadEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::BrewingStartEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::brewing_start::BrewingStartEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::CampfireStartEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::campfire_start::CampfireStartEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::CauldronLevelChangeEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::cauldron_level_change::CauldronLevelChangeEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::CrafterCraftEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::crafter_craft::CrafterCraftEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::EntityBlockFormEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::entity_block_form::EntityBlockFormEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::FluidLevelChangeEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::fluid_level_change::FluidLevelChangeEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::InventoryBlockStartEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::inventory_block_start::InventoryBlockStartEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::LeavesDecayEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::leaves_decay::LeavesDecayEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::MoistureChangeEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::moisture_change::MoistureChangeEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::SculkBloomEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::sculk_bloom::SculkBloomEvent,
            >(resource, handler, priority, blocking);
        }
        EventType::VaultDisplayItemEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::block::vault_display_item::VaultDisplayItemEvent,
            >(resource, handler, priority, blocking);
        }

        _ => {
            tracing::error!("non-block event should not be routed to register_block_event");
        }
    }
}

fn register_raid_event(
    resource: &Context,
    handler: &Arc<WasmPluginEventHandler>,
    priority: pumpkin_core::plugin::EventPriority,
    blocking: bool,
    event_type: EventType,
) {
    use pumpkin_core::plugin::raid::{
        raid_finish::RaidFinishEvent, raid_spawn_wave::RaidSpawnWaveEvent,
        raid_stop::RaidStopEvent, raid_trigger::RaidTriggerEvent,
    };

    match event_type {
        EventType::RaidFinishEvent => {
            register_typed_event::<RaidFinishEvent>(resource, handler, priority, blocking);
        }
        EventType::RaidSpawnWaveEvent => {
            register_typed_event::<RaidSpawnWaveEvent>(resource, handler, priority, blocking);
        }
        EventType::RaidStopEvent => {
            register_typed_event::<RaidStopEvent>(resource, handler, priority, blocking);
        }
        EventType::RaidTriggerEvent => {
            register_typed_event::<RaidTriggerEvent>(resource, handler, priority, blocking);
        }
        _ => {
            tracing::error!("non-raid event should not be routed to register_raid_event");
        }
    }
}

fn register_dialog_event(
    resource: &Context,
    handler: &Arc<WasmPluginEventHandler>,
    priority: pumpkin_core::plugin::EventPriority,
    blocking: bool,
    event_type: EventType,
) {
    use pumpkin_core::plugin::api::events::dialog::{
        DialogClearEvent, DialogClickActionEvent, DialogShowEvent,
    };

    match event_type {
        EventType::DialogClickActionEvent => {
            register_typed_event::<DialogClickActionEvent>(resource, handler, priority, blocking);
        }
        EventType::DialogShowEvent => {
            register_typed_event::<DialogShowEvent>(resource, handler, priority, blocking);
        }
        EventType::DialogClearEvent => {
            register_typed_event::<DialogClearEvent>(resource, handler, priority, blocking);
        }
        _ => {
            tracing::error!("non-dialog event should not be routed to register_dialog_event");
        }
    }
}

fn register_server_event(
    resource: &Context,
    handler: &Arc<WasmPluginEventHandler>,
    priority: pumpkin_core::plugin::EventPriority,
    blocking: bool,
    event_type: EventType,
) {
    use pumpkin_core::plugin::server::{
        list_ping::ServerListPingEvent,
        packet::{PacketReceivedEvent, PacketSentEvent},
        server_broadcast::ServerBroadcastEvent,
        server_command::ServerCommandEvent,
        server_load::ServerLoadEvent,
        server_tick_end::ServerTickEndEvent,
        server_tick_start::ServerTickStartEvent,
    };

    match event_type {
        EventType::PacketReceivedEvent => {
            register_typed_event::<PacketReceivedEvent>(resource, handler, priority, blocking);
        }
        EventType::PacketSentEvent => {
            register_typed_event::<PacketSentEvent>(resource, handler, priority, blocking);
        }
        EventType::ServerCommandEvent => {
            register_typed_event::<ServerCommandEvent>(resource, handler, priority, blocking);
        }
        EventType::ServerListPingEvent => {
            register_typed_event::<ServerListPingEvent>(resource, handler, priority, blocking);
        }
        EventType::ServerBroadcastEvent => {
            register_typed_event::<ServerBroadcastEvent>(resource, handler, priority, blocking);
        }
        EventType::ServerLoadEvent => {
            register_typed_event::<ServerLoadEvent>(resource, handler, priority, blocking);
        }
        EventType::ServerTickEndEvent => {
            register_typed_event::<ServerTickEndEvent>(resource, handler, priority, blocking);
        }
        EventType::ServerTickStartEvent => {
            register_typed_event::<ServerTickStartEvent>(resource, handler, priority, blocking);
        }
        EventType::MapInitializeEvent => {
            register_typed_event::<
                pumpkin_core::plugin::api::events::server::map_initialize::MapInitializeEvent,
            >(resource, handler, priority, blocking);
        }
        _ => {
            tracing::error!("non-server event should not be routed to register_server_event");
        }
    }
}

fn register_hanging_event(
    ctx: &Context,
    handler: &Arc<WasmPluginEventHandler>,
    priority: pumpkin_core::plugin::EventPriority,
    blocking: bool,
    event_type: EventType,
) {
    use pumpkin_core::plugin::hanging::{
        hanging_break::HangingBreakEvent, hanging_break_by_entity::HangingBreakByEntityEvent,
        hanging_place::HangingPlaceEvent,
    };

    match event_type {
        EventType::HangingBreakEvent => {
            register_typed_event::<HangingBreakEvent>(ctx, handler, priority, blocking);
        }
        EventType::HangingBreakByEntityEvent => {
            register_typed_event::<HangingBreakByEntityEvent>(ctx, handler, priority, blocking);
        }
        EventType::HangingPlaceEvent => {
            register_typed_event::<HangingPlaceEvent>(ctx, handler, priority, blocking);
        }
        _ => {
            tracing::error!("non-hanging event should not be routed to register_hanging_event");
        }
    }
}

impl pumpkin::plugin::context::Host for PluginHostState {}

impl pumpkin::plugin::context::HostContext for PluginHostState {
    #[allow(clippy::too_many_lines)]
    fn register_event(
        &mut self,
        context: Resource<WitContext>,
        handler_id: u32,
        event_type: EventType,
        event_priority: EventPriority,
        blocking: bool,
    ) -> wasmtime::Result<()> {
        // Updated return type
        let priority = match event_priority {
            EventPriority::Highest => pumpkin_core::plugin::EventPriority::Highest,
            EventPriority::High => pumpkin_core::plugin::EventPriority::High,
            EventPriority::Normal => pumpkin_core::plugin::EventPriority::Normal,
            EventPriority::Low => pumpkin_core::plugin::EventPriority::Low,
            EventPriority::Lowest => pumpkin_core::plugin::EventPriority::Lowest,
        };

        // Use ? to trap if the plugin was dropped or the context handle is dead
        let plugin = self
            .plugin
            .as_ref()
            .ok_or_else(|| wasmtime::Error::msg("Plugin state uninitialized"))?
            .upgrade()
            .ok_or_else(|| wasmtime::Error::msg("Plugin has been dropped"))?;

        let ctx = self.get(&context)?.as_ref();
        let handler = Arc::new(WasmPluginEventHandler { handler_id, plugin });

        match event_type {
            event_type @ (EventType::PacketReceivedEvent
            | EventType::PacketSentEvent
            | EventType::ServerCommandEvent
            | EventType::ServerListPingEvent
            | EventType::ServerBroadcastEvent
            | EventType::ServerLoadEvent
            | EventType::ServerTickEndEvent
            | EventType::ServerTickStartEvent
            | EventType::MapInitializeEvent) => {
                register_server_event(ctx, &handler, priority, blocking, event_type);
            }
            event_type @ (EventType::HangingBreakEvent
            | EventType::HangingBreakByEntityEvent
            | EventType::HangingPlaceEvent) => {
                register_hanging_event(ctx, &handler, priority, blocking, event_type);
            }
            event_type @ (EventType::SpawnChangeEvent
            | EventType::ChunkLoadEvent
            | EventType::ChunkSaveEvent
            | EventType::ChunkSendEvent
            | EventType::WeatherChangeEvent
            | EventType::ThunderChangeEvent
            | EventType::WorldLoadEvent
            | EventType::WorldUnloadEvent
            | EventType::AsyncStructureGenerateEvent
            | EventType::AsyncStructureSpawnEvent
            | EventType::ChunkPopulateEvent
            | EventType::ChunkUnloadEvent
            | EventType::EntitiesLoadEvent
            | EventType::EntitiesUnloadEvent
            | EventType::GenericGameEvent
            | EventType::LootGenerateEvent
            | EventType::PortalCreateEvent
            | EventType::StructureGrowEvent
            | EventType::TimeSkipEvent
            | EventType::WorldInitEvent
            | EventType::WorldSaveEvent
            | EventType::LightningStrikeEvent) => {
                register_world_event(ctx, &handler, priority, blocking, event_type);
            }
            event_type @ (EventType::EntityDamageEvent
            | EventType::EntityDeathEvent
            | EventType::PlayerDeathEvent
            | EventType::EntitySpawnEvent
            | EventType::EntityCombustEvent
            | EventType::EntityRegainHealthEvent
            | EventType::EntityAirChangeEvent
            | EventType::EntityBreedEvent
            | EventType::EntityDismountEvent
            | EventType::EntityDyeEvent
            | EventType::EntityEnterLoveModeEvent
            | EventType::EntityExplodeEvent
            | EventType::EntityMountEvent
            | EventType::EntityPickupItemEvent
            | EventType::EntityPortalEvent
            | EventType::EntityResurrectEvent
            | EventType::EntityShootBowEvent
            | EventType::EntityTameEvent
            | EventType::EntityTargetEvent
            | EventType::EntityTeleportEvent
            | EventType::EntityToggleGlideEvent
            | EventType::EntityTransformEvent
            | EventType::CreatureSpawnEvent
            | EventType::EnderDragonChangePhaseEvent
            | EventType::EntityBreakDoorEvent
            | EventType::EntityChangeBlockEvent
            | EventType::EntityDamageByBlockEvent
            | EventType::EntityDamageByEntityEvent
            | EventType::EntityDropItemEvent
            | EventType::EntityEnterBlockEvent
            | EventType::EntityExhaustionEvent
            | EventType::EntityInteractEvent
            | EventType::EntityKnockbackEvent
            | EventType::EntityPlaceEvent
            | EventType::EntityPoseChangeEvent
            | EventType::EntityPotionEffectEvent
            | EventType::EntitySpellCastEvent
            | EventType::EntityTargetLivingEntityEvent
            | EventType::EntityToggleSwimEvent
            | EventType::ExplosionPrimeEvent
            | EventType::FireworkExplodeEvent
            | EventType::FoodLevelChangeEvent
            | EventType::ItemDespawnEvent
            | EventType::ItemMergeEvent
            | EventType::ItemSpawnEvent
            | EventType::PiglinBarterEvent
            | EventType::ProjectileHitEvent
            | EventType::ProjectileLaunchEvent
            | EventType::SheepDyeWoolEvent
            | EventType::SheepRegrowWoolEvent
            | EventType::SlimeSplitEvent
            | EventType::StriderTemperatureChangeEvent
            | EventType::VillagerAcquireTradeEvent
            | EventType::VillagerCareerChangeEvent
            | EventType::VillagerReplenishTradeEvent
            | EventType::WardenAngerChangeEvent
            | EventType::AreaEffectCloudApplyEvent
            | EventType::ArrowBodyCountChangeEvent
            | EventType::BatToggleSleepEvent
            | EventType::CreeperPowerEvent
            | EventType::EntityCombustByBlockEvent
            | EventType::EntityCombustByEntityEvent
            | EventType::EntityKnockbackByEntityEvent
            | EventType::EntityPortalEnterEvent
            | EventType::EntityPortalExitEvent
            | EventType::EntityRemoveEvent
            | EventType::EntityTargetBlockEvent
            | EventType::EntityUnleashEvent
            | EventType::ExpBottleEvent
            | EventType::HorseJumpEvent
            | EventType::LingeringPotionSplashEvent
            | EventType::PigZapEvent
            | EventType::PigZombieAngerEvent
            | EventType::PotionSplashEvent
            | EventType::SpawnerSpawnEvent
            | EventType::TrialSpawnerSpawnEvent
            | EventType::VillagerReputationChangeEvent) => {
                register_entity_event(ctx, &handler, priority, blocking, event_type);
            }
            event_type @ (EventType::BlockRedstoneEvent
            | EventType::BlockBreakEvent
            | EventType::BlockBurnEvent
            | EventType::BlockCanBuildEvent
            | EventType::BlockGrowEvent
            | EventType::BlockPlaceEvent
            | EventType::BlockDamageEvent
            | EventType::BlockIgniteEvent
            | EventType::BlockFromToEvent
            | EventType::BlockFormEvent
            | EventType::BlockFadeEvent
            | EventType::BlockDispenseEvent
            | EventType::BlockExplodeEvent
            | EventType::BlockPhysicsEvent
            | EventType::BlockPistonExtendEvent
            | EventType::BlockPistonRetractEvent
            | EventType::NotePlayEvent
            | EventType::SignChangeEvent
            | EventType::SpongeAbsorbEvent
            | EventType::TntPrimeEvent
            | EventType::BellResonateEvent
            | EventType::BellRingEvent
            | EventType::BlockBrushEvent
            | EventType::BlockCookEvent
            | EventType::BlockDamageAbortEvent
            | EventType::BlockDispenseArmorEvent
            | EventType::BlockDispenseLootEvent
            | EventType::BlockDropItemEvent
            | EventType::BlockExpEvent
            | EventType::BlockFertilizeEvent
            | EventType::BlockMultiPlaceEvent
            | EventType::BlockReceiveGameEvent
            | EventType::BlockShearEntityEvent
            | EventType::BlockSpreadEvent
            | EventType::BrewingStartEvent
            | EventType::CampfireStartEvent
            | EventType::CauldronLevelChangeEvent
            | EventType::CrafterCraftEvent
            | EventType::EntityBlockFormEvent
            | EventType::FluidLevelChangeEvent
            | EventType::InventoryBlockStartEvent
            | EventType::LeavesDecayEvent
            | EventType::MoistureChangeEvent
            | EventType::SculkBloomEvent
            | EventType::VaultDisplayItemEvent) => {
                register_block_event(ctx, &handler, priority, blocking, event_type);
            }
            event_type @ (EventType::InventoryOpenEvent
            | EventType::InventoryDragEvent
            | EventType::CraftItemEvent
            | EventType::FurnaceSmeltEvent
            | EventType::BrewEvent
            | EventType::BrewingStandFuelEvent
            | EventType::FurnaceBurnEvent
            | EventType::FurnaceExtractEvent
            | EventType::FurnaceStartSmeltEvent
            | EventType::HopperInventorySearchEvent
            | EventType::InventoryCreativeEvent
            | EventType::InventoryInteractEvent
            | EventType::InventoryMoveItemEvent
            | EventType::InventoryPickupItemEvent
            | EventType::PrepareAnvilEvent
            | EventType::PrepareGrindstoneEvent
            | EventType::PrepareInventoryResultEvent
            | EventType::PrepareItemCraftEvent
            | EventType::PrepareSmithingEvent
            | EventType::SmithItemEvent
            | EventType::TradeSelectEvent) => {
                register_inventory_event(ctx, &handler, priority, blocking, event_type);
            }
            event_type @ (EventType::PrepareItemEnchantEvent | EventType::EnchantItemEvent) => {
                register_enchantment_event(ctx, &handler, priority, blocking, event_type);
            }
            event_type @ (EventType::VehicleBlockCollisionEvent
            | EventType::VehicleCollisionEvent
            | EventType::VehicleCreateEvent
            | EventType::VehicleDamageEvent
            | EventType::VehicleDestroyEvent
            | EventType::VehicleEnterEvent
            | EventType::VehicleEntityCollisionEvent
            | EventType::VehicleExitEvent
            | EventType::VehicleMoveEvent
            | EventType::VehicleUpdateEvent) => {
                register_vehicle_event(ctx, &handler, priority, blocking, event_type);
            }
            event_type @ (EventType::RaidFinishEvent
            | EventType::RaidSpawnWaveEvent
            | EventType::RaidStopEvent
            | EventType::RaidTriggerEvent) => {
                register_raid_event(ctx, &handler, priority, blocking, event_type);
            }
            event_type @ (EventType::DialogClickActionEvent
            | EventType::DialogShowEvent
            | EventType::DialogClearEvent) => {
                register_dialog_event(ctx, &handler, priority, blocking, event_type);
            }
            event_type => {
                register_player_event(ctx, &handler, priority, blocking, event_type);
            }
        }

        Ok(())
    }

    fn register_command(
        &mut self,
        context: Resource<WitContext>,
        command: Resource<Command>,
        permission: String,
    ) -> wasmtime::Result<()> {
        use pumpkin_core::command::argument_builder::ArgumentBuilder;

        let command = self.take(command)?;
        let context = self.get(&context)?.clone();
        let aliases = if command.names.len() > 1 {
            command.names[1..].to_vec()
        } else {
            Vec::new()
        };
        let node = command.builder.build();
        context.register_command_with_aliases(node, &aliases, permission);

        Ok(())
    }

    fn register_permission(
        &mut self,
        context: Resource<WitContext>,
        permission: Permission,
    ) -> wasmtime::Result<Result<(), String>> {
        let mut children = HashMap::with_capacity(permission.children.len());
        for child in permission.children {
            children.insert(child.node, child.value);
        }

        let util_permission = pumpkin_util::permission::Permission {
            node: permission.node,
            description: permission.description,
            default: match permission.default {
                PermissionDefault::Deny => pumpkin_util::permission::PermissionDefault::Deny,
                PermissionDefault::Allow => pumpkin_util::permission::PermissionDefault::Allow,
                PermissionDefault::Op(lvl) => {
                    pumpkin_util::permission::PermissionDefault::Op(match lvl {
                        PermissionLevel::Zero => pumpkin_util::permission::PermissionLvl::Zero,
                        PermissionLevel::One => pumpkin_util::permission::PermissionLvl::One,
                        PermissionLevel::Two => pumpkin_util::permission::PermissionLvl::Two,
                        PermissionLevel::Three => pumpkin_util::permission::PermissionLvl::Three,
                        PermissionLevel::Four => pumpkin_util::permission::PermissionLvl::Four,
                    })
                }
            },
            children,
        };

        let context_res = self.get(&context)?;
        Ok(context_res.register_permission(util_permission))
    }

    fn get_data_folder(&mut self, _context: Resource<WitContext>) -> wasmtime::Result<String> {
        Ok("data".to_string())
    }

    fn get_server(&mut self, context: Resource<WitContext>) -> wasmtime::Result<Resource<Server>> {
        self.add(self.get(&context)?.server.clone())
    }

    fn get_marketplace_metadata(
        &mut self,
        _context: Resource<WitContext>,
    ) -> wasmtime::Result<Option<MarketplaceMetadata>> {
        Ok(self
            .marketplace_metadata
            .clone()
            .map(|metadata| MarketplaceMetadata {
                marketplace_url: metadata.marketplace_url,
                plugin_id: metadata.plugin_id,
                plugin_name: metadata.plugin_name,
                version: metadata.version,
                dev_id: metadata.dev_id,
                dev_name: metadata.dev_name,
                is_paid: metadata.is_paid,
                user_id: metadata.user_id,
                license_key: metadata.license_key,
                issued_at: metadata.issued_at,
            }))
    }

    fn drop(&mut self, rep: Resource<WitContext>) -> wasmtime::Result<()> {
        self.drop(rep)
    }
}
