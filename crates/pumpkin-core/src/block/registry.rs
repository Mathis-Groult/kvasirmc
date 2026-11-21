use crate::block::{
    BlockBehaviour, BlockHitResult, BlockMetadata, BonemealArgs, FluidMetadata,
    GetInsideCollisionShapeArgs, GetScreenHandlerFactoryArgs, OnEntityCollisionArgs,
    OnEntityStepArgs, OnLandedUponArgs, OnProjectileHitArgs, PathComputationType,
    UpdateEntityMovementAfterFallOnArgs, stop_vertical_movement_after_fall,
};
use crate::entity::EntityBase;
use crate::entity::player::Player;
use crate::server::Server;
use crate::world::World;
use pumpkin_data::BlockStateId;
use pumpkin_data::block_properties::SnowLikeProperties;
use pumpkin_data::block_rotation::{Mirror, Rotation};
use pumpkin_data::data_component_impl::EquipmentSlot;
use pumpkin_data::fluid::Fluid;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::tag::{self, Taggable};
use pumpkin_data::{Block, BlockDirection, BlockId, BlockState};
use pumpkin_inventory::screen_handler::ScreenHandlerFactory;
use pumpkin_protocol::java::server::play::SUseItemOn;
use pumpkin_util::math::boundingbox::BoundingBox;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::world::{BlockAccessor, BlockFlags};
use rustc_hash::FxHashMap;
use std::sync::Arc;

use super::BlockIsReplacing;
use super::fluid::FluidBehaviour;
use super::{
    AttackArgs, BrokenArgs, CanPlaceAtArgs, CanUpdateAtArgs, EmitsRedstonePowerArgs, ExplodeArgs,
    GetRedstonePowerArgs, GetStateForNeighborUpdateArgs, NormalUseArgs, OnNeighborUpdateArgs,
    OnPlaceArgs, OnStateReplacedArgs, OnSyncedBlockEventArgs, PlacedArgs, PlayerPlacedArgs,
    PrepareArgs, UseWithItemArgs,
};

#[must_use]
pub fn default_registry() -> Arc<BlockRegistry> {
    let mut manager = BlockRegistry::default();
    defaults::register_all(&mut manager);
    Arc::new(manager)
}

// ActionResult.java
#[derive(PartialEq, Eq)]
pub enum BlockActionResult {
    /// Action was successful | Same as SUCCESS in vanilla
    Success,
    /// Action was successful and we should swing the hand for the server | Same as `SUCCESS_SERVER` in vanilla
    SuccessServer,
    /// Block other actions from being executed | Same as CONSUME in vanilla
    Consume,
    /// Allow other actions from being executed, but indicate it failed | Same as FAIL in vanilla
    Fail,
    /// Allow other actions from being executed | Same as PASS in vanilla
    Pass,
    /// Use default action for the block: `normal_use` | Same as `PASS_TO_DEFAULT_BLOCK_ACTION` in vanilla
    PassToDefaultBlockAction,
}

impl BlockActionResult {
    #[must_use]
    pub const fn consumes_action(&self) -> bool {
        matches!(self, Self::Consume | Self::Success | Self::SuccessServer)
    }
}

mod defaults;

/// Marks a block with no registered behaviour. Never handed out as a real index.
const NO_BEHAVIOUR: u16 = u16::MAX;

pub struct BlockRegistry {
    block_indices: [u16; pumpkin_data::BlockId::COUNT as usize],
    behaviours: Vec<Arc<dyn BlockBehaviour>>,
    fluids: FxHashMap<u16, Arc<dyn FluidBehaviour>>,
}

impl Default for BlockRegistry {
    fn default() -> Self {
        Self {
            block_indices: [NO_BEHAVIOUR; pumpkin_data::BlockId::COUNT as usize],
            behaviours: Vec::new(),
            fluids: FxHashMap::default(),
        }
    }
}

#[derive(Debug)]
pub enum BlockPlacingError {
    InvalidGamemode,
    BlockOutOfWorld,
}

pub(crate) fn can_replace_with_other_block(block: &Block, state: &BlockState) -> bool {
    // Vanilla SnowLayerBlock.canBeReplaced: only a single layer gives way to another item.
    if block == &Block::SNOW {
        return SnowLikeProperties::from_state_id(state.id).layers == 1;
    }
    // Sculk veins allow replacement by another block despite their state flag.
    block == &Block::SCULK_VEIN || state.replaceable()
}

impl BlockRegistry {
    pub fn bone_meal(
        &self,
        block: &Block,
        world: &Arc<World>,
        position: &BlockPos,
        state_id: BlockStateId,
    ) -> bool {
        let Some(behaviour) = self.get_pumpkin_block(block.id) else {
            return false;
        };
        let args = BonemealArgs {
            world,
            block,
            position,
            state_id,
        };
        if !behaviour.is_valid_bonemeal_target(args) {
            return false;
        }
        if behaviour.is_bonemeal_success(args) {
            if let Some(server) = world.server.upgrade() {
                let mut event =
                    crate::plugin::api::events::block::block_fertilize::BlockFertilizeEvent::new(
                        *position,
                        world.clone(),
                        None,
                        vec![],
                    );
                server.plugin_manager.fire_blocking(&server, &mut event);
                if event.cancelled {
                    return false;
                }
            }
            behaviour.perform_bonemeal(args);
        }
        true
    }

    fn entity_blocks_block_placement(entity: &dyn EntityBase) -> bool {
        let base_entity = entity.get_entity();
        if base_entity.is_removed()
            || base_entity
                .no_physics
                .load(std::sync::atomic::Ordering::Relaxed)
            || entity.is_spectator()
        {
            return false;
        }

        if entity.get_living_entity().is_some() {
            return true;
        }

        let entity_type = base_entity.entity_type;
        let resource_name = entity_type.resource_name;
        entity_type == &pumpkin_data::entity::EntityType::END_CRYSTAL
            || entity_type == &pumpkin_data::entity::EntityType::FALLING_BLOCK
            || entity_type == &pumpkin_data::entity::EntityType::TNT
            || resource_name.ends_with("_minecart")
            || resource_name.ends_with("_boat")
            || resource_name.ends_with("_raft")
    }

    fn has_blocking_entity_in_box(world: &World, placed_box: &BoundingBox) -> bool {
        let players = world.players.load();
        if players.iter().any(|player| {
            Self::entity_blocks_block_placement(player.as_ref())
                && player
                    .get_entity()
                    .bounding_box
                    .load()
                    .intersects(placed_box)
        }) {
            return true;
        }

        world.entities.load().iter().any(|entity| {
            Self::entity_blocks_block_placement(entity.as_ref())
                && entity
                    .get_entity()
                    .bounding_box
                    .load()
                    .intersects(placed_box)
        })
    }

    /// Mirrors vanilla `BlockPlaceContext`: where the placed block would go, or `None` when `canPlace()` is false.
    pub fn placement_target(
        &self,
        world: &World,
        player: &Player,
        placed_block: &Block,
        use_item_on: &SUseItemOn,
        location: BlockPos,
        face: BlockDirection,
    ) -> Option<(BlockPos, BlockDirection, BlockIsReplacing)> {
        let clicked_block_pos = BlockPos(location.0);
        let (clicked_block, clicked_block_state) = world.get_block_and_state(&clicked_block_pos);

        let replace_clicked_block = if clicked_block == placed_block {
            self.can_update_at(
                world,
                clicked_block,
                clicked_block_state.id,
                &clicked_block_pos,
                face,
                use_item_on,
                player,
            )
            .then_some(BlockIsReplacing::Itself(clicked_block_state.id))
        } else if can_replace_with_other_block(clicked_block, clicked_block_state) {
            if clicked_block == &Block::WATER {
                use pumpkin_data::block_properties::WaterLikeProperties;
                let water_props = WaterLikeProperties::from_state_id(clicked_block_state.id);
                Some(BlockIsReplacing::Water(water_props.level))
            } else {
                Some(BlockIsReplacing::Other)
            }
        } else {
            None
        };

        if let Some(replacing) = replace_clicked_block {
            return Some((clicked_block_pos, face.opposite(), replacing));
        }

        let block_pos = BlockPos(location.0 + face.to_offset());
        let (previous_block, previous_block_state) = world.get_block_and_state(&block_pos);

        let replace_previous_block = if previous_block == placed_block {
            self.can_update_at(
                world,
                previous_block,
                previous_block_state.id,
                &block_pos,
                face.opposite(),
                use_item_on,
                player,
            )
            .then_some(BlockIsReplacing::Itself(previous_block_state.id))
        } else {
            can_replace_with_other_block(previous_block, previous_block_state).then(|| {
                if previous_block == &Block::WATER {
                    use pumpkin_data::block_properties::WaterLikeProperties;
                    let water_props = WaterLikeProperties::from_state_id(previous_block_state.id);
                    BlockIsReplacing::Water(water_props.level)
                } else {
                    BlockIsReplacing::None
                }
            })
        };

        replace_previous_block.map(|replacing| (block_pos, face.opposite(), replacing))
    }

    #[expect(clippy::too_many_lines)]
    pub fn place_block(
        &self,
        player: &Arc<Player>,
        placed_block: &'static Block,
        server: &Arc<Server>,
        use_item_on: &SUseItemOn,
        location: BlockPos,
        face: BlockDirection,
    ) -> Result<Option<(BlockPos, BlockStateId)>, BlockPlacingError> {
        let entity = &player.get_entity();
        let held_stack = pumpkin_util::Hand::from_packet_id(use_item_on.hand.0)
            .ok()
            .map(|hand| player.inventory().get_stack_in_hand(hand));
        let clicked_block_pos = BlockPos(location.0);
        let world = entity.world.load_full();
        let clicked_block = world.get_block(&clicked_block_pos);

        match player.gamemode.load() {
            pumpkin_util::GameMode::Spectator => {
                return Err(BlockPlacingError::InvalidGamemode);
            }
            pumpkin_util::GameMode::Adventure => {
                let can_place = held_stack
                    .as_ref()
                    .and_then(|s| {
                        s.get_data_component::<pumpkin_data::data_component_impl::CanPlaceOnImpl>()
                    })
                    .is_some_and(|comp| comp.matches_block(clicked_block));
                if !can_place {
                    return Err(BlockPlacingError::InvalidGamemode);
                }
            }
            _ => {}
        }

        if location.0.y + face.to_offset().y < world.get_bottom_y() {
            return Err(BlockPlacingError::BlockOutOfWorld);
        }

        if location.0.y + face.to_offset().y > world.get_top_y() {
            player.send_system_message_raw(
                &pumpkin_util::text::TextComponent::translate_cross(
                    pumpkin_data::translation::java::BUILD_TOOHIGH,
                    pumpkin_data::translation::bedrock::BUILD_TOOHIGH,
                    vec![pumpkin_util::text::TextComponent::text(
                        (world.get_top_y()).to_string(),
                    )],
                )
                .color_named(pumpkin_util::text::color::NamedColor::Red),
                true,
            );
            return Err(BlockPlacingError::BlockOutOfWorld);
        }

        let Some((final_block_pos, final_face, replacing)) =
            self.placement_target(&world, player, placed_block, use_item_on, location, face)
        else {
            return Ok(None);
        };

        if world.is_in_spawn_protection(player, &final_block_pos) {
            player.send_system_message(&pumpkin_util::text::TextComponent::translate_cross(
                pumpkin_data::translation::java::BUILD_SPAWN_PROTECTION,
                pumpkin_data::translation::java::BUILD_SPAWN_PROTECTION,
                [pumpkin_util::text::TextComponent::text(
                    player.gameprofile.name.clone(),
                )],
            ));
            return Ok(None);
        }

        if !self.can_place_at(
            Some(server),
            Some(&*world),
            &*world,
            Some(player),
            placed_block,
            placed_block.default_state,
            &final_block_pos,
            Some(final_face),
            Some(use_item_on),
        ) {
            return Ok(None);
        }

        let mut new_state = self.on_place(
            server,
            &world,
            player,
            placed_block,
            &final_block_pos,
            final_face,
            replacing,
            use_item_on,
        );

        if let Some(ref stack) = held_stack
            && let Some(block_state_comp) =
                stack.get_data_component::<pumpkin_data::data_component_impl::BlockStateImpl>()
            && let Some(props) = placed_block.properties(new_state)
        {
            let mut prop_list = props.to_props();
            for (k, v) in block_state_comp.properties.iter() {
                for p in &mut prop_list {
                    if p.key().to_str() == k.as_ref() {
                        for state_entry in placed_block.states {
                            if let Some(sp) = placed_block.properties(state_entry.id)
                                && let Some(candidate) = sp
                                    .to_props()
                                    .into_iter()
                                    .find(|cp| cp.key() == p.key() && cp.value_str() == v.as_ref())
                            {
                                *p = candidate;
                                break;
                            }
                        }
                    }
                }
            }
            new_state = placed_block
                .from_properties(&prop_list)
                .to_state_id(placed_block);
        }

        // Mirror vanilla obstruction checks: only entities that block building should prevent
        // placement. (e.g. arrows/xp orbs/displays/markers should not)
        let state = BlockState::from_id(new_state);
        let mut buildable = true;
        for shape in state.get_block_collision_shapes_at(&final_block_pos) {
            let placed_box = shape.at_pos(final_block_pos);

            if Self::has_blocking_entity_in_box(world.as_ref(), &placed_box) {
                buildable = false;
                break;
            }
        }

        let mut can_build_event = crate::plugin::block::block_can_build::BlockCanBuildEvent {
            block_to_build: placed_block,
            buildable,
            player: player.clone(),
            block: clicked_block,
            cancelled: false,
        };
        server
            .plugin_manager
            .fire_blocking(server, &mut can_build_event);
        if can_build_event.cancelled || !can_build_event.buildable {
            return Ok(None);
        }

        let mut event = crate::plugin::block::block_place::BlockPlaceEvent::new(
            player.clone(),
            placed_block,
            clicked_block,
            final_block_pos,
            true,
        );
        server.plugin_manager.fire_blocking(server, &mut event);
        if event.cancelled {
            return Ok(None);
        }

        world.place_block_for(player, &final_block_pos, new_state);

        world.play_bedrock_level_sound(
            "place",
            &final_block_pos.to_centered_f64(),
            BlockState::to_be_network_id(new_state) as i32,
        );

        if let Some(ref stack) = held_stack
            && placed_block.default_state.block_entity_type != u16::MAX
        {
            if let Some(block_entity_data) =
                stack.get_data_component::<pumpkin_data::data_component_impl::BlockEntityDataImpl>()
                && let Some(existing_be) = world.get_block_entity(&final_block_pos)
            {
                let mut nbt = pumpkin_nbt::compound::NbtCompound::new();
                existing_be.write_internal(&mut nbt);
                for (k, v) in &block_entity_data.nbt.child_tags {
                    if !matches!(k.as_ref(), "x" | "y" | "z" | "id") {
                        nbt.child_tags.insert(k.clone(), v.clone());
                    }
                }
                if let Some(reloaded) = crate::block::entities::block_entity_from_nbt(&nbt) {
                    world.add_block_entity(reloaded);
                }
            }
            if let Some(block_entity) = world.get_block_entity(&final_block_pos) {
                block_entity.apply_item_components(stack);
            }
        }

        self.player_placed(
            &world,
            placed_block,
            new_state,
            &final_block_pos,
            face,
            player,
        );

        player.trigger_advancement(
            crate::entity::player::advancement::trigger::AdvancementTrigger::PlacedBlock {
                block_id: format!("minecraft:{}", placed_block.name),
            },
        );

        Ok(Some((final_block_pos, new_state)))
    }
    #[allow(clippy::expect_used)]
    pub fn register<T: BlockBehaviour + BlockMetadata + 'static>(&mut self, block: T) {
        let ids = T::ids();
        let idx = u16::try_from(self.behaviours.len())
            .ok()
            .filter(|idx| *idx != NO_BEHAVIOUR)
            .expect("Too many block behaviours for the index table");
        self.behaviours.push(Arc::new(block));
        for i in ids {
            self.block_indices[i.as_u16() as usize] = idx;
        }
    }

    pub fn register_fluid<T: FluidBehaviour + FluidMetadata + 'static>(&mut self, fluid: T) {
        let ids = T::ids();
        let val = Arc::new(fluid);
        self.fluids.reserve(ids.len());
        for i in ids {
            self.fluids.insert(i, val.clone());
        }
    }

    pub fn on_synced_block_event(
        &self,
        block: &Block,
        world: &Arc<World>,
        position: &BlockPos,
        r#type: u8,
        data: u8,
    ) -> bool {
        let pumpkin_block = self.get_pumpkin_block(block.id);
        if let Some(pumpkin_block) = pumpkin_block {
            return pumpkin_block.on_synced_block_event(OnSyncedBlockEventArgs {
                world,
                block,
                position,
                r#type,
                data,
            });
        }
        false
    }

    pub fn on_entity_collision(
        &self,
        block: &Block,
        world: &Arc<World>,
        entity: &dyn EntityBase,
        position: &BlockPos,
        state: &BlockState,
        server: &Server,
    ) {
        let pumpkin_block = self.get_pumpkin_block(block.id);
        if let Some(pumpkin_block) = pumpkin_block {
            pumpkin_block.on_entity_collision(OnEntityCollisionArgs {
                server,
                world,
                block,
                state,
                position,
                entity,
            });
        }
    }

    pub fn on_entity_step(
        &self,
        block: &Block,
        world: &Arc<World>,
        entity: &dyn EntityBase,
        position: &BlockPos,
        state: &BlockState,
        below_supporting_block: bool,
    ) {
        if let Some(pumpkin_block) = self.get_pumpkin_block(block.id) {
            pumpkin_block.on_entity_step(OnEntityStepArgs {
                world,
                block,
                state,
                position,
                entity,
                below_supporting_block,
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn on_projectile_hit(
        &self,
        block: &Block,
        world: &Arc<World>,
        projectile: &dyn EntityBase,
        position: &BlockPos,
        state: &BlockState,
        hit_pos: &Vector3<f64>,
        server: &Server,
    ) {
        if let Some(pumpkin_block) = self.get_pumpkin_block(block.id) {
            pumpkin_block.on_projectile_hit(OnProjectileHitArgs {
                server,
                world,
                block,
                state,
                position,
                projectile,
                hit_pos,
            });
        }
    }

    pub fn on_entity_collision_fluid(&self, fluid: &Fluid, entity: &dyn EntityBase) {
        let pumpkin_fluid = self.get_pumpkin_fluid(fluid.id);
        if let Some(pumpkin_fluid) = pumpkin_fluid {
            pumpkin_fluid.on_entity_collision(entity);
        }
    }

    pub fn on_use(
        &self,
        block: &Block,
        player: &Arc<Player>,
        position: &BlockPos,
        hit: &BlockHitResult<'_>,
        server: &Server,
        world: &Arc<World>,
    ) -> BlockActionResult {
        let pumpkin_block = self.get_pumpkin_block(block.id);
        if let Some(pumpkin_block) = pumpkin_block {
            return pumpkin_block.normal_use(NormalUseArgs {
                server,
                world,
                block,
                position,
                player,
                hit,
            });
        }
        BlockActionResult::Pass
    }

    pub fn get_screen_handler_factory(
        &self,
        block: &Block,
        player: &Arc<Player>,
        position: &BlockPos,
        server: &Server,
        world: &Arc<World>,
    ) -> Option<Box<dyn ScreenHandlerFactory>> {
        let pumpkin_block = self.get_pumpkin_block(block.id);
        if let Some(pumpkin_block) = pumpkin_block {
            return pumpkin_block.get_screen_handler_factory(GetScreenHandlerFactoryArgs {
                server,
                world,
                block,
                position,
                player,
            });
        }
        None
    }

    pub fn explode(&self, block: &Block, world: &Arc<World>, position: &BlockPos) {
        let pumpkin_block = self.get_pumpkin_block(block.id);
        if let Some(pumpkin_block) = pumpkin_block {
            pumpkin_block.explode(ExplodeArgs {
                world,
                block,
                position,
            });
        }
    }

    #[expect(clippy::too_many_arguments)]
    pub fn use_with_item(
        &self,
        block: &Block,
        player: &Arc<Player>,
        position: &BlockPos,
        hit: &BlockHitResult<'_>,
        item_stack: &mut ItemStack,
        equipment_slot: &EquipmentSlot,
        server: &Server,
        world: &Arc<World>,
    ) -> BlockActionResult {
        let pumpkin_block = self.get_pumpkin_block(block.id);
        if let Some(pumpkin_block) = pumpkin_block {
            return pumpkin_block.use_with_item(UseWithItemArgs {
                server,
                world,
                block,
                position,
                player,
                hit,
                item_stack,
                equipment_slot,
            });
        }
        BlockActionResult::Pass
    }

    pub fn use_with_item_fluid(
        &self,
        fluid: &Fluid,
        player: &Arc<Player>,
        position: BlockPos,
        item: &Item,
        server: &Server,
        world: &Arc<World>,
    ) -> BlockActionResult {
        let pumpkin_fluid = self.get_pumpkin_fluid(fluid.id);
        if let Some(pumpkin_fluid) = pumpkin_fluid {
            return pumpkin_fluid.use_with_item(fluid, player, position, item, server, world);
        }
        BlockActionResult::Pass
    }

    #[expect(clippy::too_many_arguments)]
    pub fn can_place_at(
        &self,
        server: Option<&Server>,
        world: Option<&World>,
        block_accessor: &dyn BlockAccessor,
        player: Option<&Player>,
        block: &Block,
        state: &BlockState,
        position: &BlockPos,
        direction: Option<BlockDirection>,
        use_item_on: Option<&SUseItemOn>,
    ) -> bool {
        let pumpkin_block = self.get_pumpkin_block(block.id);
        if let Some(pumpkin_block) = pumpkin_block {
            return pumpkin_block.can_place_at(CanPlaceAtArgs {
                server,
                world,
                block_accessor,
                block,
                state,
                position,
                direction,
                player,
                use_item_on,
            });
        }
        true
    }

    #[expect(clippy::too_many_arguments)]
    pub fn can_update_at(
        &self,
        world: &World,
        block: &Block,
        state_id: BlockStateId,
        position: &BlockPos,
        direction: BlockDirection,
        use_item_on: &SUseItemOn,
        player: &Player,
    ) -> bool {
        let pumpkin_block = self.get_pumpkin_block(block.id);
        if let Some(pumpkin_block) = pumpkin_block {
            return pumpkin_block.can_update_at(CanUpdateAtArgs {
                world,
                block,
                state_id,
                position,
                direction,
                player,
                use_item_on,
            });
        }
        false
    }

    #[expect(clippy::too_many_arguments)]
    pub fn on_place(
        &self,
        server: &Server,
        world: &World,
        player: &Player,
        block: &Block,
        position: &BlockPos,
        direction: BlockDirection,
        replacing: BlockIsReplacing,
        use_item_on: &SUseItemOn,
    ) -> BlockStateId {
        let pumpkin_block = self.get_pumpkin_block(block.id);
        if let Some(pumpkin_block) = pumpkin_block {
            return pumpkin_block.on_place(OnPlaceArgs {
                server,
                world,
                block,
                position,
                direction,
                player,
                replacing,
                use_item_on,
            });
        }
        block.default_state.id
    }

    pub fn player_placed(
        &self,
        world: &Arc<World>,
        block: &Block,
        state_id: BlockStateId,
        position: &BlockPos,
        direction: BlockDirection,
        player: &Player,
    ) {
        let pumpkin_block = self.get_pumpkin_block(block.id);
        if let Some(pumpkin_block) = pumpkin_block {
            pumpkin_block.player_placed(PlayerPlacedArgs {
                world,
                block,
                state_id,
                position,
                direction,
                player,
            });
        }
    }

    pub fn on_placed(
        &self,
        world: &Arc<World>,
        block: &Block,
        state_id: BlockStateId,
        position: &BlockPos,
        old_state_id: BlockStateId,
        notify: bool,
    ) {
        let state = world.get_block_state(position);
        if state.block_entity_type != u16::MAX
            && world.get_block_entity(position).is_none()
            && let Some(entity) =
                crate::block::entities::create_block_entity(state.block_entity_type, *position)
        {
            world.add_block_entity(entity);
        }

        let pumpkin_block = self.get_pumpkin_block(block.id);
        if let Some(pumpkin_block) = pumpkin_block {
            pumpkin_block.placed(PlacedArgs {
                world,
                block,
                state_id,
                old_state_id,
                position,
                notify,
            });
        }
    }

    pub fn on_placed_fluid(
        &self,
        world: &Arc<World>,
        fluid: &Fluid,
        state_id: BlockStateId,
        position: &BlockPos,
        old_state_id: BlockStateId,
        notify: bool,
    ) {
        let pumpkin_fluid = self.get_pumpkin_fluid(fluid.id);
        if let Some(pumpkin_fluid) = pumpkin_fluid {
            pumpkin_fluid.placed(world, fluid, state_id, position, old_state_id, notify);
        }
    }

    pub fn on_landed_upon(
        &self,
        block: &Block,
        world: &Arc<World>,
        fall_distance: f32,
        entity: &dyn EntityBase,
    ) {
        let pumpkin_block = self.get_pumpkin_block(block.id);
        if let Some(pumpkin_block) = pumpkin_block {
            pumpkin_block.on_landed_upon(OnLandedUponArgs {
                world,
                fall_distance,
                entity,
            });
        }
    }

    pub fn update_entity_movement_after_fall_on(&self, block: &Block, entity: &dyn EntityBase) {
        if let Some(pumpkin_block) = self.get_pumpkin_block(block.id) {
            pumpkin_block.update_entity_movement_after_fall_on(
                UpdateEntityMovementAfterFallOnArgs { entity },
            );
        } else {
            stop_vertical_movement_after_fall(entity);
        }
    }

    pub fn attack(
        &self,
        world: &Arc<World>,
        block: &Block,
        state: &BlockState,
        position: &BlockPos,
        player: &Arc<Player>,
    ) {
        let pumpkin_block = self.get_pumpkin_block(block.id);
        if let Some(pumpkin_block) = pumpkin_block {
            pumpkin_block.attack(AttackArgs {
                world,
                block,
                state,
                position,
                player,
            });
        }
    }

    pub fn broken(
        &self,
        world: &Arc<World>,
        block: &Block,
        player: &Arc<Player>,
        position: &BlockPos,
        server: &Server,
        state: &BlockState,
    ) {
        let pumpkin_block = self.get_pumpkin_block(block.id);
        if let Some(pumpkin_block) = pumpkin_block {
            pumpkin_block.broken(BrokenArgs {
                block,
                player,
                position,
                server,
                world,
                state,
            });
        }
    }

    pub fn on_state_replaced(
        &self,
        world: &Arc<World>,
        block: &Block,
        position: &BlockPos,
        old_state_id: BlockStateId,
        moved: bool,
    ) {
        let pumpkin_block = self.get_pumpkin_block(block.id);
        if let Some(pumpkin_block) = pumpkin_block {
            pumpkin_block.on_state_replaced(OnStateReplacedArgs {
                world,
                block,
                old_state_id,
                position,
                moved,
            });
        }
    }

    /// Updates state of all neighbors of the block
    pub fn post_process_state(
        &self,
        world: &Arc<World>,
        position: &BlockPos,
        block: &Block,
        flags: BlockFlags,
    ) {
        let state_id = world.get_block_state_id(position);
        for direction in BlockDirection::all() {
            let neighbor_pos = position.offset(direction.to_offset());
            let neighbor_state_id = world.get_block_state_id(&neighbor_pos);
            let pumpkin_block = self.get_pumpkin_block(block.id);
            if let Some(pumpkin_block) = pumpkin_block {
                let new_state =
                    pumpkin_block.get_state_for_neighbor_update(GetStateForNeighborUpdateArgs {
                        world,
                        block,
                        state_id,
                        position,
                        direction: direction.opposite(),
                        neighbor_position: &neighbor_pos,
                        neighbor_state_id,
                    });
                world.set_block_state(&neighbor_pos, new_state, flags);
            }
        }
    }

    pub fn prepare(
        &self,
        world: &Arc<World>,
        position: &BlockPos,
        block: &Block,
        state_id: BlockStateId,
        flags: BlockFlags,
    ) {
        let pumpkin_block = self.get_pumpkin_block(block.id);
        if let Some(pumpkin_block) = pumpkin_block {
            pumpkin_block.prepare(PrepareArgs {
                world,
                block,
                state_id,
                position,
                flags,
            });
        }
    }

    #[expect(clippy::too_many_arguments)]
    pub fn get_state_for_neighbor_update(
        &self,
        world: &World,
        block: &Block,
        state_id: BlockStateId,
        position: &BlockPos,
        direction: BlockDirection,
        neighbor_location: &BlockPos,
        neighbor_state_id: BlockStateId,
    ) -> BlockStateId {
        let pumpkin_block = self.get_pumpkin_block(block.id);
        if let Some(pumpkin_block) = pumpkin_block {
            return pumpkin_block.get_state_for_neighbor_update(GetStateForNeighborUpdateArgs {
                world,
                block,
                state_id,
                position,
                direction,
                neighbor_position: neighbor_location,
                neighbor_state_id,
            });
        }
        state_id
    }

    pub fn update_neighbors(&self, world: &Arc<World>, position: &BlockPos, flags: BlockFlags) {
        for direction in BlockDirection::abstract_block_update_order() {
            let pos = position.offset(direction.to_offset());

            world.replace_with_state_for_neighbor_update(&pos, direction.opposite(), flags);
        }
    }

    pub fn on_neighbor_update(
        &self,
        world: &Arc<World>,
        block: &Block,
        position: &BlockPos,
        source_block: &Block,
        notify: bool,
    ) {
        let pumpkin_block = self.get_pumpkin_block(block.id);
        if let Some(pumpkin_block) = pumpkin_block {
            pumpkin_block.on_neighbor_update(OnNeighborUpdateArgs {
                world,
                block,
                position,
                source_block,
                notify,
            });
        }
    }

    #[inline]
    #[must_use]
    pub fn get_pumpkin_block(&self, block: BlockId) -> Option<&Arc<dyn BlockBehaviour>> {
        let idx = self.block_indices[block.as_u16() as usize];
        if idx == NO_BEHAVIOUR {
            None
        } else {
            self.behaviours.get(idx as usize)
        }
    }

    #[must_use]
    pub fn get_pumpkin_fluid(&self, fluid_id: u16) -> Option<&Arc<dyn FluidBehaviour>> {
        self.fluids.get(&fluid_id).or_else(|| {
            // Still fluids share behavior with their flowing counterpart
            match fluid_id {
                2 => self.fluids.get(&1),
                4 => self.fluids.get(&3),
                _ => None,
            }
        })
    }

    #[must_use]
    pub fn emits_redstone_power(
        &self,
        block: &Block,
        state: &BlockState,
        direction: BlockDirection,
    ) -> bool {
        let pumpkin_block = self.get_pumpkin_block(block.id);
        if let Some(pumpkin_block) = pumpkin_block {
            return pumpkin_block.emits_redstone_power(EmitsRedstonePowerArgs {
                block,
                state,
                direction,
            });
        }
        false
    }

    pub fn get_weak_redstone_power(
        &self,
        block: &Block,
        world: &World,
        position: &BlockPos,
        state: &BlockState,
        direction: BlockDirection,
    ) -> u8 {
        let pumpkin_block = self.get_pumpkin_block(block.id);
        if let Some(pumpkin_block) = pumpkin_block {
            return pumpkin_block.get_weak_redstone_power(GetRedstonePowerArgs {
                world,
                block,
                state,
                position,
                direction,
            });
        }
        0
    }

    pub fn get_strong_redstone_power(
        &self,
        block: &Block,
        world: &World,
        position: &BlockPos,
        state: &BlockState,
        direction: BlockDirection,
    ) -> u8 {
        let pumpkin_block = self.get_pumpkin_block(block.id);
        if let Some(pumpkin_block) = pumpkin_block {
            return pumpkin_block.get_strong_redstone_power(GetRedstonePowerArgs {
                world,
                block,
                state,
                position,
                direction,
            });
        }
        0
    }

    pub fn get_inside_collision_shape(
        &self,
        block: &Block,
        world: &World,
        state: &BlockState,
        position: &BlockPos,
    ) -> BoundingBox {
        let pumpkin_block = self.get_pumpkin_block(block.id);
        if let Some(pumpkin_block) = pumpkin_block {
            return pumpkin_block.get_inside_collision_shape(GetInsideCollisionShapeArgs {
                world,
                block,
                state,
                position,
            });
        }
        BoundingBox::full_block()
    }

    #[must_use]
    pub fn mirror(
        &self,
        block: &Block,
        state_id: BlockStateId,
        mirror: Mirror,
    ) -> &'static BlockState {
        self.get_pumpkin_block(block.id).map_or_else(
            || block.mirror(state_id, mirror),
            |pumpkin_block| pumpkin_block.mirror(block, state_id, mirror),
        )
    }

    #[must_use]
    pub fn rotate(
        &self,
        block: &Block,
        state_id: BlockStateId,
        rotation: Rotation,
    ) -> &'static BlockState {
        self.get_pumpkin_block(block.id).map_or_else(
            || block.rotate(state_id, rotation),
            |pumpkin_block| pumpkin_block.rotate(block, state_id, rotation),
        )
    }

    #[must_use]
    pub fn is_pathfindable(
        &self,
        block: &Block,
        state: &BlockState,
        computation_type: PathComputationType,
    ) -> bool {
        self.get_pumpkin_block(block.id).map_or_else(
            || match computation_type {
                PathComputationType::Water => {
                    World::get_fluid_from_state_id(state.id).has_tag(&tag::Fluid::MINECRAFT_WATER)
                }
                PathComputationType::Land | PathComputationType::Air => !state.is_full_cube(),
            },
            |pumpkin_block| pumpkin_block.is_pathfindable(state, computation_type),
        )
    }
}

#[cfg(test)]
mod replacement_tests {
    use super::can_replace_with_other_block;
    use pumpkin_data::{
        Block, BlockState,
        block_properties::{GlowLichenLikeProperties, SnowLikeProperties},
    };

    #[test]
    fn sculk_vein_can_be_replaced_when_dry_or_waterlogged() {
        let block = &Block::SCULK_VEIN;
        for waterlogged in [false, true] {
            let mut properties = GlowLichenLikeProperties::default(block);
            properties.down = true;
            properties.waterlogged = waterlogged;
            let state = BlockState::from_id(properties.to_state_id(block));
            assert!(!state.replaceable());
            assert!(can_replace_with_other_block(block, state));
        }
    }

    #[test]
    fn only_single_layer_snow_can_be_replaced() {
        let block = &Block::SNOW;
        for (layers, expected) in [(1, true), (2, false), (8, false)] {
            let mut properties = SnowLikeProperties::default(block);
            properties.layers = layers;
            let state = BlockState::from_id(properties.to_state_id(block));
            assert!(state.replaceable());
            assert_eq!(can_replace_with_other_block(block, state), expected);
        }
    }

    #[test]
    fn other_blocks_keep_their_replacement_flags() {
        for (block, expected) in [
            (&Block::AIR, true),
            (&Block::WATER, true),
            (&Block::SHORT_GRASS, true),
            (&Block::GLOW_LICHEN, true),
            (&Block::RESIN_CLUMP, true),
            (&Block::STONE, false),
            (&Block::OAK_SLAB, false),
        ] {
            assert_eq!(
                can_replace_with_other_block(block, block.default_state),
                expected
            );
        }
    }
}
