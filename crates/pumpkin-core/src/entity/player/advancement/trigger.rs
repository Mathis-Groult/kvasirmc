mod combat_triggers;
mod creature_triggers;
mod entity_event_triggers;
mod inventory_triggers;
mod item_use_triggers;
mod travel_triggers;
mod world_triggers;
use crate::entity::player::Player;
use crate::entity::player::advancement::PlayerAdvancement;
use crate::entity::player::advancement::predicate::CriterionConditions;

#[derive(Debug, Clone)]
pub enum AdvancementTrigger {
    InventoryChanged,
    PlayerKilledEntity {
        entity_type_resource: String,
    },
    SniperDuel,
    TwoBirdsOneArrow,
    Arbalistic,
    EnterDimension {
        dimension: String,
    },
    SleptInBed,
    ConsumeItem {
        item_id: String,
    },
    PlacedBlock {
        block_id: String,
    },
    EnterBlock {
        block_id: String,
    },
    FishedItem {
        item_id: String,
    },
    DeflectedDamage,
    PlayerKilled,
    BredAnimal {
        parent_type: String,
    },
    LaunchedEyeOfEnder,
    GlowedSign,
    DealtOverkillDamage,
    Bullseye,
    CuredZombieVillager,
    TradedWithVillager,
    AllayDropItemOnBlock {
        block_id: String,
        item_id: String,
    },
    ThrownItemPickedUpByPlayer {
        thrower_type: String,
    },
    Location,
    ItemUsedOnBlock {
        block_id: String,
        item_id: String,
        state: Option<(String, String)>,
        smokey: Option<bool>,
        biome: Option<String>,
    },
    TameAnimal {
        entity_type: String,
    },
    FilledBucket {
        item_id: String,
    },
    UsingItem {
        item_id: String,
        looking_at: Option<String>,
    },
    ConstructBeacon {
        level: i32,
    },
    SlideDownBlock {
        block_id: String,
    },
    RecipeCrafted {
        recipe_id: String,
    },
    RecipeUnlocked {
        recipe_id: String,
    },
    PlayerGeneratesContainerLoot {
        loot_table_id: String,
    },
    EnchantedItem {
        levels: i32,
    },
    BrewedPotion,
    ItemDurabilityChanged {
        item_id: String,
        vehicle_type: Option<String>,
    },
    Levitation {
        distance_y: f64,
    },
    FallFromHeight {
        distance_y: f64,
        from_y: f64,
        to_y: f64,
    },
    FallAfterExplosion {
        distance_y: f64,
        cause: String,
    },
    NetherTravel {
        distance_horizontal: f64,
    },
    RideEntityInLava {
        distance_horizontal: f64,
        dimension: String,
        vehicle_type: Option<String>,
    },
    ShotCrossbow {
        item_id: String,
    },
    AvoidVibration,
    BeeNestDestroyed {
        block_id: String,
        num_bees_inside: i32,
    },
    ChanneledLightning {
        victim_type: String,
    },
    LightningStrike {
        bystander_type: Option<String>,
        blocks_set_on_fire: i32,
        distance: f64,
    },
    KillMobNearSculkCatalyst,
    HeroOfTheVillage,
    PlayerInteractedWithEntity {
        entity_type: String,
        item_id: String,
        is_baby: bool,
        player_wearing_safe_armor: bool,
    },
    ThrownItemPickedUpByEntity {
        entity_type: String,
        item_id: String,
        is_baby: bool,
        player_wearing_safe_armor: bool,
    },
    PlayerShearedEquipment {
        entity_type: String,
        item_id: String,
    },
    StartedRiding {
        vehicle_type: Option<String>,
        passenger_type: Option<String>,
    },
    SummonedEntity {
        entity_type: String,
    },
    CrafterRecipeCrafted {
        recipe_id: String,
    },
    EffectsChanged,
    SpearMobs {
        count: i32,
    },
    UsedTotem {
        item_id: String,
    },
    Tick,
}

impl Player {
    /// Evaluates any active criteria listeners for the given trigger type against the provided predicate.
    pub fn trigger_criterion_event(
        &self,
        trigger_type: &str,
        predicate_matcher: impl Fn(&CriterionConditions) -> bool,
    ) {
        let to_finish = {
            let Ok(mut adv) = self.advancements.try_lock() else {
                return;
            };

            let Some(listeners) = adv.active_listeners.get(trigger_type) else {
                return;
            };

            if listeners.is_empty() {
                return;
            }

            let mut matched = Vec::new();
            for listener in listeners {
                if predicate_matcher(&listener.conditions) {
                    matched.push((listener.advancement, listener.criterion));
                }
            }

            if matched.is_empty() {
                return;
            }

            let Some(player) = adv.player.upgrade() else {
                return;
            };

            let mut awards = Vec::with_capacity(matched.len());
            for (advancement, criterion) in matched {
                let award = adv.award(advancement, criterion);
                if award.completed {
                    awards.push((player.clone(), advancement, award));
                }
            }
            awards
        };

        for (player, advancement, award) in to_finish {
            PlayerAdvancement::finish_award(&player, advancement, award);
        }
    }

    /// Evaluates data-driven advancement triggers for the player.
    #[expect(
        clippy::too_many_lines,
        reason = "dispatching all advancement triggers"
    )]
    pub fn trigger_advancement(&self, trigger: AdvancementTrigger) {
        match trigger {
            AdvancementTrigger::InventoryChanged => self.inventory_changed(),

            AdvancementTrigger::Location => self.location(),

            AdvancementTrigger::PlayerKilledEntity {
                entity_type_resource,
            } => self.player_killed_entity(&entity_type_resource),

            AdvancementTrigger::SniperDuel => self.sniper_duel(),

            AdvancementTrigger::TwoBirdsOneArrow | AdvancementTrigger::Arbalistic => {
                self.two_birds_one_arrow_or_arbalistic();
            }

            AdvancementTrigger::EnterDimension { dimension } => self.enter_dimension(&dimension),

            AdvancementTrigger::SleptInBed => self.slept_in_bed(),

            AdvancementTrigger::ConsumeItem { item_id } => self.consume_item(&item_id),

            AdvancementTrigger::PlacedBlock { block_id } => self.placed_block(&block_id),

            AdvancementTrigger::EnterBlock { block_id } => self.enter_block(&block_id),

            AdvancementTrigger::FishedItem { item_id } => self.fished_item(&item_id),

            AdvancementTrigger::DeflectedDamage => self.deflected_damage(),

            AdvancementTrigger::PlayerKilled => self.player_killed(),

            AdvancementTrigger::BredAnimal { parent_type } => self.bred_animal(&parent_type),

            AdvancementTrigger::LaunchedEyeOfEnder => self.launched_eye_of_ender(),

            AdvancementTrigger::GlowedSign => self.glowed_sign(),

            AdvancementTrigger::DealtOverkillDamage => self.dealt_overkill_damage(),

            AdvancementTrigger::Bullseye => self.bullseye(),

            AdvancementTrigger::CuredZombieVillager => self.cured_zombie_villager(),

            AdvancementTrigger::TradedWithVillager => self.traded_with_villager(),

            AdvancementTrigger::AllayDropItemOnBlock { block_id, item_id } => {
                self.allay_drop_item_on_block(&block_id, &item_id);
            }

            AdvancementTrigger::ThrownItemPickedUpByPlayer { thrower_type } => {
                self.thrown_item_picked_up_by_player(&thrower_type);
            }

            AdvancementTrigger::ItemUsedOnBlock {
                block_id,
                item_id,
                state,
                smokey,
                biome,
            } => self.item_used_on_block(
                &block_id,
                &item_id,
                state.as_ref(),
                smokey,
                biome.as_deref(),
            ),

            AdvancementTrigger::TameAnimal { entity_type } => self.tame_animal(&entity_type),

            AdvancementTrigger::FilledBucket { item_id } => self.filled_bucket(&item_id),

            AdvancementTrigger::UsingItem {
                item_id,
                looking_at,
            } => self.using_item(&item_id, looking_at.as_deref()),

            AdvancementTrigger::ConstructBeacon { level } => self.construct_beacon(level),

            AdvancementTrigger::SlideDownBlock { block_id } => self.slide_down_block(&block_id),

            AdvancementTrigger::RecipeCrafted { recipe_id } => self.recipe_crafted(&recipe_id),

            AdvancementTrigger::RecipeUnlocked { recipe_id } => self.recipe_unlocked(&recipe_id),

            AdvancementTrigger::PlayerGeneratesContainerLoot { loot_table_id } => {
                self.player_generates_container_loot(&loot_table_id);
            }

            AdvancementTrigger::EnchantedItem { .. } => self.enchanted_item(),

            AdvancementTrigger::BrewedPotion => self.brewed_potion(),

            AdvancementTrigger::ItemDurabilityChanged {
                item_id,
                vehicle_type,
            } => self.item_durability_changed(&item_id, vehicle_type.as_deref()),

            AdvancementTrigger::Levitation { distance_y } => self.levitation(distance_y),

            AdvancementTrigger::FallFromHeight {
                distance_y,
                from_y,
                to_y,
            } => self.fall_from_height(distance_y, from_y, to_y),

            AdvancementTrigger::FallAfterExplosion { distance_y, cause } => {
                self.fall_after_explosion(distance_y, &cause);
            }

            AdvancementTrigger::NetherTravel {
                distance_horizontal,
            } => self.nether_travel(distance_horizontal),

            AdvancementTrigger::RideEntityInLava {
                distance_horizontal,
                dimension,
                vehicle_type,
            } => self.ride_entity_in_lava(distance_horizontal, &dimension, vehicle_type.as_deref()),

            AdvancementTrigger::ShotCrossbow { item_id } => self.shot_crossbow(&item_id),

            AdvancementTrigger::AvoidVibration => self.avoid_vibration(),

            AdvancementTrigger::BeeNestDestroyed {
                block_id,
                num_bees_inside,
            } => self.bee_nest_destroyed(&block_id, num_bees_inside),

            AdvancementTrigger::ChanneledLightning { victim_type } => {
                self.channeled_lightning(&victim_type);
            }

            AdvancementTrigger::LightningStrike {
                bystander_type,
                blocks_set_on_fire,
                distance,
            } => self.lightning_strike(bystander_type.as_deref(), blocks_set_on_fire, distance),

            AdvancementTrigger::KillMobNearSculkCatalyst => self.kill_mob_near_sculk_catalyst(),

            AdvancementTrigger::HeroOfTheVillage => self.hero_of_the_village(),

            AdvancementTrigger::PlayerInteractedWithEntity {
                entity_type,
                item_id,
                is_baby,
                player_wearing_safe_armor,
            } => self.player_interacted_with_entity(
                &entity_type,
                &item_id,
                is_baby,
                player_wearing_safe_armor,
            ),

            AdvancementTrigger::ThrownItemPickedUpByEntity {
                entity_type,
                item_id,
                is_baby,
                player_wearing_safe_armor,
            } => self.thrown_item_picked_up_by_entity(
                &entity_type,
                &item_id,
                is_baby,
                player_wearing_safe_armor,
            ),

            AdvancementTrigger::PlayerShearedEquipment {
                entity_type,
                item_id,
            } => self.player_sheared_equipment(&entity_type, &item_id),

            AdvancementTrigger::StartedRiding {
                vehicle_type,
                passenger_type,
            } => self.started_riding(vehicle_type.as_deref(), passenger_type.as_deref()),

            AdvancementTrigger::SummonedEntity { entity_type } => {
                self.summoned_entity(&entity_type);
            }

            AdvancementTrigger::CrafterRecipeCrafted { recipe_id } => {
                self.crafter_recipe_crafted(&recipe_id);
            }

            AdvancementTrigger::EffectsChanged => self.effects_changed(),

            AdvancementTrigger::SpearMobs { count } => self.spear_mobs(count),

            AdvancementTrigger::UsedTotem { item_id } => self.used_totem(&item_id),

            AdvancementTrigger::Tick => self.tick_trigger(),
        }
    }
}
