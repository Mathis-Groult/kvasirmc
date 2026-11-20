use pumpkin_data::Block;
use pumpkin_data::BlockState;
use pumpkin_data::BlockStateId;
use pumpkin_data::damage::DamageType;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::tag::{self, Taggable};
use pumpkin_data::world::WorldEvent;
use pumpkin_protocol::bedrock::client::CUpdateBlock;
use pumpkin_protocol::java::client::play::CBlockUpdate;
use pumpkin_util::math::position::BlockPos;
use pumpkin_world::world::BlockFlags;
use std::sync::{Arc, atomic::Ordering};

use crate::{
    block::{blocks::falling::FallingBlock, registry::can_replace_with_other_block},
    entity::{Entity, EntityBase, living::LivingEntity},
    server::Server,
    world::World,
};

pub struct FallingEntity {
    entity: Entity,
    block_state_id: BlockStateId,
    /// Vanilla `fallDistance`: blocks fallen since the last landing.
    fall_distance: crossbeam::atomic::AtomicCell<f64>,
}

/// Vanilla `setHurtsEntities`: damage per block fallen and the cap. Only anvils here
/// (`AnvilBlock.falling`: 2 per block, at most 40); pointed dripstone is not ported.
fn hurts_entities(block: &Block) -> Option<(f32, f32)> {
    block
        .has_tag(&tag::Block::MINECRAFT_ANVIL)
        .then_some((2.0, 40.0))
}

impl FallingEntity {
    pub const fn new(mut entity: Entity, block_state_id: BlockStateId) -> Self {
        entity.blocks_building = true;
        Self {
            entity,
            block_state_id,
            fall_distance: crossbeam::atomic::AtomicCell::new(0.0),
        }
    }

    /// Replaced the current Block and Spawns a new Falling one (synchronous)
    pub fn replace_spawn(world: &Arc<World>, position: BlockPos, block_state: BlockStateId) {
        // Replace the original block, TODO: use fluid state
        world.set_block_state(
            &position,
            Block::AIR.default_state.id,
            BlockFlags::NOTIFY_ALL,
        );

        let position = position.0.to_f64().add_raw(0.5, 0.0, 0.5);
        let entity = Entity::new(world.clone(), position, &EntityType::FALLING_BLOCK);
        entity
            .data
            .store(i32::from(block_state.as_u16()), Ordering::Relaxed);
        let entity = Arc::new(Self::new(entity, block_state));
        world.spawn_entity_non_save(entity);
    }
}

impl FallingEntity {
    /// Vanilla `FallingBlockEntity.causeFallDamage` on landing: an anvil hurts every living
    /// entity in its box (not creative or spectator players) by 2 per block fallen beyond the
    /// first, at most 40, and may crack (5% + 5% per block). Returns the block to place, `None`
    /// when a damaged anvil broke. Pumpkin had no fall damage at all (card #62).
    fn cause_fall_damage(&self, world: &Arc<World>) -> Option<BlockStateId> {
        let state_id = self.block_state_id;
        let fallen = self.fall_distance.swap(0.0);
        let Some((per_block, max)) = hurts_entities(Block::from_state_id(state_id)) else {
            return Some(state_id);
        };
        let blocks = (fallen - 1.0).ceil();
        if blocks < 0.0 {
            return Some(state_id);
        }
        let damage = ((blocks as f32) * per_block).floor().min(max);
        for target in world.get_all_at_box(&self.entity.bounding_box.load()) {
            let Some(living) = target.get_living_entity() else {
                continue;
            };
            if target.get_entity().entity_id == self.entity.entity_id
                || living.health.load() <= 0.0
                || target.is_spectator()
                || target
                    .get_player()
                    .is_some_and(super::player::Player::is_creative)
            {
                continue;
            }
            // `caller` is the victim itself (as in explosions), not the anvil.
            target.damage(target.as_ref(), damage, DamageType::FALLING_ANVIL);
        }
        let crack_chance = 0.05 + blocks as f32 * 0.05;
        if damage > 0.0 && rand::random::<f32>() < crack_chance {
            return crate::block::blocks::anvil::AnvilBlock::damage(state_id);
        }
        Some(state_id)
    }
}

impl EntityBase for FallingEntity {
    fn tick(&self, caller: &dyn EntityBase, _server: &Server) {
        let entity = &self.entity;
        let mut velo = entity.velocity.load();
        velo.y -= self.get_gravity();

        entity.velocity.store(velo);

        let before_y = entity.pos.load().y;
        entity.move_entity(caller, velo);
        let fallen = before_y - entity.pos.load().y;
        if fallen > 0.0 {
            self.fall_distance.store(self.fall_distance.load() + fallen);
        }
        entity.tick_block_collisions(caller);
        if entity.on_ground.load(Ordering::Relaxed) {
            entity.velocity.store(velo.multiply(0.7, -0.5, 0.7));
            let world = entity.world.load();
            let landing_pos = self.entity.block_pos.load();
            let (current_block, current_state) = world.get_block_and_state(&landing_pos);
            // Vanilla waits until a piston has finished moving a block through the landing cell.
            if current_block != &Block::MOVING_PISTON {
                let Some(mut state_id) = self.cause_fall_damage(&world) else {
                    // The anvil broke on landing: no block and no item (vanilla `cancelDrop`).
                    world.sync_world_event(WorldEvent::SoundAnvilBroken, landing_pos, 0);
                    self.entity.remove();
                    entity.velocity.store(velo.multiply(0.98, 0.98, 0.98));
                    return;
                };
                let block = Block::from_state_id(state_id);
                let (below_block, below_state) = world.get_block_and_state(&landing_pos.down());
                let may_replace = can_replace_with_other_block(current_block, current_state);
                let would_continue_falling =
                    FallingBlock::can_fall_through(below_state, below_block);
                let would_survive = world.block_registry.can_place_at(
                    None,
                    Some(&**world),
                    &**world,
                    None,
                    block,
                    state_id.to_state(),
                    &landing_pos,
                    None,
                    None,
                ) && !would_continue_falling;
                if may_replace && would_survive {
                    if block.has_tag(&tag::Block::MINECRAFT_CONCRETE_POWDERS)
                        && FallingBlock::should_solidify(&**world, &landing_pos)
                        && let Some(name) = block.name.strip_suffix("_powder")
                        && let Some(concrete) = Block::from_name(name)
                    {
                        state_id = concrete.default_state.id;
                    }
                    world.set_block_state(&landing_pos, state_id, BlockFlags::NOTIFY_ALL);
                    if block.has_tag(&tag::Block::MINECRAFT_ANVIL) {
                        // Vanilla `AnvilBlock.onLand`.
                        world.sync_world_event(WorldEvent::SoundAnvilLand, landing_pos, 0);
                    }
                    // block updates to watchers before the despawn, else a invisible block gap until the tick flush.
                    let placed = world.get_block_state_id(&landing_pos);
                    world.send_to_tracking_players_editioned(
                        entity,
                        &CBlockUpdate::new(landing_pos, i32::from(placed.as_u16()).into()),
                        &CUpdateBlock::new(landing_pos, BlockState::to_be_network_id(placed)),
                    );
                } else if world.level_info.load().game_rules.entity_drops
                    && let Some(item) = Item::from_id(block.item_id)
                {
                    entity.spawn_at_location(ItemStack::new(1, item));
                }
                self.entity.remove();
            }
        }

        entity.velocity.store(velo.multiply(0.98, 0.98, 0.98));
    }

    fn init_data_tracker(&self) {
        self.entity.set_synced_data(
            pumpkin_data::tracked_data::falling_block::START_POS,
            self.entity.block_pos.load(),
        );
    }

    fn get_entity(&self) -> &Entity {
        &self.entity
    }

    fn get_living_entity(&self) -> Option<&LivingEntity> {
        None
    }
    fn damage(&self, _caller: &dyn EntityBase, _amount: f32, _damage_type: DamageType) -> bool {
        false
    }

    fn get_gravity(&self) -> f64 {
        0.04
    }

    fn entity_bedrock_spawn_metadata(
        &self,
    ) -> Option<pumpkin_protocol::bedrock::client::set_actor_data::SyncedActorDataList> {
        let mut meta =
            pumpkin_protocol::bedrock::client::set_actor_data::SyncedActorDataList::new();
        let runtime_id = BlockState::to_be_network_id(self.block_state_id);
        meta.set(
            pumpkin_protocol::bedrock::client::set_actor_data::entity_data_key::VARIANT,
            pumpkin_protocol::bedrock::client::set_actor_data::MetadataValue::Int(
                runtime_id as i32,
            ),
        );
        Some(meta)
    }

    fn bedrock_y_offset(&self) -> f64 {
        0.49
    }

    fn cast_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests;
