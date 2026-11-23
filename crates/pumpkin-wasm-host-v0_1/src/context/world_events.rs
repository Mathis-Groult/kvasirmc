use std::sync::Arc;

use super::register_typed_event;
use crate::{events::WasmPluginEventHandler, pumpkin::plugin::event::EventType};
use pumpkin_core::plugin::Context;

pub fn register_world_event(
    resource: &Context,
    handler: &Arc<WasmPluginEventHandler>,
    priority: pumpkin_core::plugin::EventPriority,
    blocking: bool,
    event_type: EventType,
) {
    use pumpkin_core::plugin::world::{
        chunk_load::ChunkLoad, chunk_save::ChunkSave, chunk_send::ChunkSend,
        spawn_change::SpawnChangeEvent,
    };

    macro_rules! register {
        ($event:ty) => {
            register_typed_event::<$event>(resource, handler, priority, blocking)
        };
    }

    match event_type {
        EventType::SpawnChangeEvent => register!(SpawnChangeEvent),
        EventType::ChunkLoadEvent => register!(ChunkLoad),
        EventType::ChunkSaveEvent => register!(ChunkSave),
        EventType::ChunkSendEvent => register!(ChunkSend),
        EventType::WeatherChangeEvent => register!(pumpkin_core::plugin::api::events::world::weather_change::WeatherChangeEvent),
        EventType::ThunderChangeEvent => register!(pumpkin_core::plugin::api::events::world::weather_change::ThunderChangeEvent),
        EventType::WorldLoadEvent => register!(pumpkin_core::plugin::api::events::world::world_load::WorldLoadEvent),
        EventType::WorldUnloadEvent => register!(pumpkin_core::plugin::api::events::world::world_load::WorldUnloadEvent),
        EventType::AsyncStructureGenerateEvent => register!(pumpkin_core::plugin::api::events::world::async_structure_generate::AsyncStructureGenerateEvent),
        EventType::AsyncStructureSpawnEvent => register!(pumpkin_core::plugin::api::events::world::async_structure_spawn::AsyncStructureSpawnEvent),
        EventType::ChunkPopulateEvent => register!(pumpkin_core::plugin::api::events::world::chunk_populate::ChunkPopulateEvent),
        EventType::ChunkUnloadEvent => register!(pumpkin_core::plugin::api::events::world::chunk_unload::ChunkUnloadEvent),
        EventType::EntitiesLoadEvent => register!(pumpkin_core::plugin::api::events::world::entities_load::EntitiesLoadEvent),
        EventType::EntitiesUnloadEvent => register!(pumpkin_core::plugin::api::events::world::entities_unload::EntitiesUnloadEvent),
        EventType::GenericGameEvent => register!(pumpkin_core::plugin::api::events::world::generic_game::GenericGameEvent),
        EventType::LootGenerateEvent => register!(pumpkin_core::plugin::api::events::world::loot_generate::LootGenerateEvent),
        EventType::PortalCreateEvent => register!(pumpkin_core::plugin::api::events::world::portal_create::PortalCreateEvent),
        EventType::StructureGrowEvent => register!(pumpkin_core::plugin::api::events::world::structure_grow::StructureGrowEvent),
        EventType::TimeSkipEvent => register!(pumpkin_core::plugin::api::events::world::time_skip::TimeSkipEvent),
        EventType::WorldInitEvent => register!(pumpkin_core::plugin::api::events::world::world_init::WorldInitEvent),
        EventType::WorldSaveEvent => register!(pumpkin_core::plugin::api::events::world::world_save::WorldSaveEvent),
        EventType::LightningStrikeEvent => register!(pumpkin_core::plugin::api::events::world::lightning_strike::LightningStrikeEvent),
        _ => {
            tracing::error!("non-world event should not be routed to register_world_event");
        }
    }
}
