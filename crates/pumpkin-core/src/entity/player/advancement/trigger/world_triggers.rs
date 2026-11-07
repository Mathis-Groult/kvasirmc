//! Triggers from places, blocks, dimensions and time.

use crate::entity::player::Player;
use crate::entity::player::advancement::predicate::CriterionConditions;
use pumpkin_util::math::position::BlockPos;

impl Player {
    /// Handles the arm `AdvancementTrigger::Location`.
    pub(super) fn location(&self) {
        let pos = self.position();
        let block_pos = BlockPos::new(
            pos.x.floor() as i32,
            pos.y.floor() as i32,
            pos.z.floor() as i32,
        );
        let world = self.world();
        let biome = world.get_biome(&block_pos).registry_id;
        let dimension = world.dimension.minecraft_name;
        let step_pos = BlockPos::new(block_pos.0.x, block_pos.0.y - 1, block_pos.0.z);
        let step_block =
            Some(pumpkin_data::Block::from_state_id(world.get_block_state(&step_pos).id).name);
        let current_block =
            pumpkin_data::Block::from_state_id(world.get_block_state(&block_pos).id).name;
        let stepping_block = if current_block == "powder_snow" {
            Some("powder_snow")
        } else {
            step_block
        };
        let feet_item = self
            .living_entity
            .entity_equipment
            .try_lock()
            .ok()
            .and_then(|guard| {
                guard
                    .equipment
                    .get(&pumpkin_data::data_component_impl::EquipmentSlot::FEET)
                    .filter(|stack| !stack.is_empty())
                    .map(|stack| stack.item.registry_key.to_string())
            });

        self.trigger_criterion_event("minecraft:location", |cond| match cond {
            CriterionConditions::Location(pred) => pred.matches(
                (pos.x, pos.y, pos.z),
                Some(biome),
                Some(dimension),
                step_block,
                None,
                None,
                stepping_block,
                feet_item.as_deref(),
            ),
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::EnterDimension{dimension}`.
    pub(super) fn enter_dimension(&self, dimension: &str) {
        self.trigger_criterion_event("minecraft:changed_dimension", |cond| match cond {
            CriterionConditions::ChangedDimension(pred) => pred.matches(None, dimension),
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::SleptInBed`.
    pub(super) fn slept_in_bed(&self) {
        self.trigger_criterion_event("minecraft:slept_in_bed", |_cond| true);
    }

    /// Handles the arm `AdvancementTrigger::PlacedBlock{block_id}`.
    pub(super) fn placed_block(&self, block_id: &str) {
        self.trigger_criterion_event("minecraft:placed_block", |cond| match cond {
            CriterionConditions::PlacedBlock(pred) => pred.matches(block_id, None),
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::EnterBlock{block_id}`.
    pub(super) fn enter_block(&self, block_id: &str) {
        self.trigger_criterion_event("minecraft:enter_block", |cond| match cond {
            CriterionConditions::EnterBlock(pred) => {
                pred.matches(pumpkin_data::tag::RegistryKey::Block, block_id)
            }
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::LaunchedEyeOfEnder`.
    pub(super) fn launched_eye_of_ender(&self) {
        self.trigger_criterion_event("minecraft:used_ender_eye", |_cond| true);
    }

    /// Handles the arm `AdvancementTrigger::GlowedSign`.
    pub(super) fn glowed_sign(&self) {
        self.trigger_criterion_event("minecraft:item_used_on_block", |_cond| true);
    }

    /// Handles the arm `AdvancementTrigger::SlideDownBlock{block_id}`.
    pub(super) fn slide_down_block(&self, block_id: &str) {
        self.trigger_criterion_event("minecraft:slide_down_block", |cond| match cond {
            CriterionConditions::SlideDownBlock(pred) => {
                pred.matches(pumpkin_data::tag::RegistryKey::Block, block_id)
            }
            CriterionConditions::Custom { .. } => true,
            _ => false,
        });
    }

    /// Handles the arm `AdvancementTrigger::AvoidVibration`.
    pub(super) fn avoid_vibration(&self) {
        self.trigger_criterion_event("minecraft:avoid_vibration", |_cond| true);
    }

    /// Handles the arm `AdvancementTrigger::Tick`.
    pub(super) fn tick_trigger(&self) {
        self.trigger_criterion_event("minecraft:tick", |_cond| true);
    }
}
