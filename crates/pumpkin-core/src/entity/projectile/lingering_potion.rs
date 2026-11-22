use crate::entity::projectile::owner::ProjectileOwner;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};

use crate::entity::projectile::splash_potion::extinguish_fire_if_water_potion;
use crate::world::World;
use crate::{
    entity::{Entity, EntityBase, projectile::ThrownItemEntity},
    server::Server,
};
use pumpkin_data::effect::StatusEffect;
use pumpkin_data::entity::EntityStatus;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_protocol::bedrock::server::actor_event::ActorEventID;
use pumpkin_protocol::java::client::play::CWorldEvent;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::{Vector2, to_chunk_pos};
use pumpkin_util::math::vector3::Vector3;
use uuid::Uuid;

const GRAVITY: f64 = 0.05;

/// An effect as `PotionContents::read_potion_effects` returns it: type, duration, amplifier,
/// ambient, show particles, show icon.
type PotionEffect = (&'static StatusEffect, i32, u8, bool, bool, bool);

pub struct LingeringPotionEntity {
    pub thrown: ThrownItemEntity,
    pub item_stack: RwLock<ItemStack>,
}

impl LingeringPotionEntity {
    pub fn new(entity: Entity) -> Self {
        entity.set_velocity(Vector3::new(0.0, 0.1, 0.0));
        let thrown = ThrownItemEntity {
            entity,
            owner: ProjectileOwner::none(),
            collides_with_projectiles: false,
            has_hit: AtomicBool::new(false),
            gravity: GRAVITY,
        };

        Self {
            thrown,
            item_stack: RwLock::new(ItemStack::new(
                1,
                &pumpkin_data::item::Item::LINGERING_POTION,
            )),
        }
    }

    pub fn new_shot(entity: Entity, shooter: &Entity) -> Self {
        let thrown = ThrownItemEntity::new(entity, shooter, GRAVITY);
        thrown.entity.set_velocity(Vector3::new(0.0, 0.1, 0.0));
        Self {
            thrown,
            item_stack: RwLock::new(ItemStack::new(
                1,
                &pumpkin_data::item::Item::LINGERING_POTION,
            )),
        }
    }

    pub fn set_item_stack(&self, item_stack: ItemStack) {
        let mut write = self
            .item_stack
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *write = item_stack;
    }
}

impl EntityBase for LingeringPotionEntity {
    fn get_projectile_owner(&self) -> Option<&ProjectileOwner> {
        Some(&self.thrown.owner)
    }

    fn init_data_tracker(&self) {
        let entity = self.get_entity();
        let stack = self
            .item_stack
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        // Sync the item stack so the client renders the correct potion type
        entity.set_synced_data(
            pumpkin_data::tracked_data::lingering_potion::ITEM_STACK,
            pumpkin_protocol::codec::item_stack_seralizer::ItemStackSerializer::from(stack.clone()),
        );
    }

    fn tick(&self, caller: &dyn EntityBase, _server: &Server) {
        self.thrown.process_tick(caller);
    }

    fn get_entity(&self) -> &Entity {
        self.thrown.get_entity()
    }

    fn get_living_entity(&self) -> Option<&crate::entity::living::LivingEntity> {
        None
    }
    fn cast_any(&self) -> &dyn std::any::Any {
        self
    }

    fn on_hit(&self, hit: crate::entity::projectile::ProjectileHit) {
        let world = self.get_entity().world.load();
        let hit_pos = hit.hit_pos();
        let stack = self.stack();

        // Play impact particles
        world.send_entity_status(self.get_entity(), EntityStatus::Death, ActorEventID::Death);

        let effects = crate::item::potion::PotionContents::read_potion_effects(&stack);

        // If no effects, just splash (like water bottles)
        if effects.is_empty() {
            extinguish_fire_if_water_potion(&world, hit_pos, &stack);
            return;
        }

        let block_pos = block_pos_of(hit_pos);
        broadcast_splash(&world, block_pos, &stack, &effects);
        extinguish_fire_if_water_potion(&world, hit_pos, &stack);

        if self.cancelled_by_plugin(&world, block_pos, &stack) {
            return;
        }
        spawn_cloud(&world, hit_pos, stack, effects);
    }
}

impl LingeringPotionEntity {
    /// A copy of the stored item stack.
    fn stack(&self) -> ItemStack {
        self.item_stack
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Fires the splash event and tells whether a plugin cancelled it.
    fn cancelled_by_plugin(
        &self,
        world: &Arc<World>,
        block_pos: BlockPos,
        stack: &ItemStack,
    ) -> bool {
        let Some(server) = world.server.upgrade() else {
            return false;
        };
        let mut event = crate::plugin::api::events::entity::lingering_potion_splash::LingeringPotionSplashEvent::new(
            self.get_entity().entity_id,
            block_pos,
            stack.item.registry_key.to_string(),
        );
        server.plugin_manager.fire_blocking(&server, &mut event);
        event.cancelled
    }
}

/// The block containing the point.
const fn block_pos_of(hit_pos: Vector3<f64>) -> BlockPos {
    BlockPos(Vector3::new(
        hit_pos.x.floor() as i32,
        hit_pos.y.floor() as i32,
        hit_pos.z.floor() as i32,
    ))
}

/// The colour of the splash particles: the stack's custom colour, else the average of the
/// effects' colours.
fn splash_color(stack: &ItemStack, effects: &[PotionEffect]) -> i32 {
    if let Some(color) = stack
        .get_data_component::<pumpkin_data::data_component_impl::PotionContentsImpl>()
        .and_then(|contents| contents.custom_color)
    {
        return color;
    }
    average_color(effects)
}

/// The per-channel mean of the effects' colours, each channel cut to a whole number.
fn average_color(effects: &[PotionEffect]) -> i32 {
    let count = effects.len() as f32;
    let (mut r_sum, mut g_sum, mut b_sum) = (0.0, 0.0, 0.0);
    for (effect, _, _, _, _, _) in effects {
        let c = effect.color;
        r_sum += ((c >> 16) & 0xFF) as f32;
        g_sum += ((c >> 8) & 0xFF) as f32;
        b_sum += (c & 0xFF) as f32;
    }
    let r = (r_sum / count) as i32;
    let g = (g_sum / count) as i32;
    let b = (b_sum / count) as i32;
    (r << 16) | (g << 8) | b
}

/// Sends the splash world event (2007 for instant effects, else 2002) to the hit chunk.
fn broadcast_splash(
    world: &World,
    block_pos: BlockPos,
    stack: &ItemStack,
    effects: &[PotionEffect],
) {
    let has_instant = effects.iter().any(|(effect, _, _, _, _, _)| {
        effect.id == pumpkin_data::effect::StatusEffect::INSTANT_DAMAGE.id
            || effect.id == pumpkin_data::effect::StatusEffect::INSTANT_HEALTH.id
    });
    let event_id = if has_instant { 2007 } else { 2002 };
    let color = splash_color(stack, effects);
    let chunk_pos = to_chunk_pos(&Vector2::new(block_pos.0.x, block_pos.0.z));
    world.broadcast_to_chunk(
        chunk_pos,
        &CWorldEvent::new(event_id, block_pos, color, false),
    );
}

/// Spawns the lingering cloud the flask leaves behind.
fn spawn_cloud(
    world: &Arc<World>,
    hit_pos: Vector3<f64>,
    stack: ItemStack,
    effects: Vec<PotionEffect>,
) {
    let cloud_entity = crate::entity::Entity::from_uuid(
        Uuid::new_v4(),
        world.clone(),
        hit_pos,
        &pumpkin_data::entity::EntityType::AREA_EFFECT_CLOUD,
    );
    let cloud = crate::entity::area_effect_cloud::AreaEffectCloudEntity::create(
        cloud_entity,
        stack,
        effects,
        600,
        3.0,
        20,
        20,
        -0.5,
        -100,
    );
    world.spawn_entity(cloud);
}
