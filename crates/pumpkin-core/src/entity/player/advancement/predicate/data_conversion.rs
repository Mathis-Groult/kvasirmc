//! The advancement conditions of the game data, turned into the typed conditions that the
//! advancement code matches players against.
//!
//! `CriterionConditions::from` has one short arm per kind of condition; what a kind needs beyond a
//! line or two is a function below, named after the predicate it builds.

use pumpkin_data::advancement_data::{
    AdvancementCondition as AC, BeaconCondition, BredAnimalsCondition, DamageCondition,
    EntityCondition, InventoryChangedCondition, ItemCondition, ItemUsedOnBlockCondition,
    LocationCondition, TargetHitCondition,
};

use super::{
    BeaconPredicate, BredAnimalsPredicate, CriterionConditions, DamagePredicate,
    DimensionPredicate, EnchantmentPredicate, EntityPredicate, FloatBounds, IntBounds,
    InventoryChangedPredicate, InventorySlotsPredicate, ItemPredicate, ItemUsedOnBlockPredicate,
    LocationPredicate, PlacedBlockPredicate, RecipePredicate, TagOrIdMatcher, TargetHitPredicate,
};

#[expect(clippy::too_many_lines, reason = "converting from data enum")]
impl From<AC> for CriterionConditions {
    fn from(cond: AC) -> Self {
        match cond {
            AC::InventoryChanged(ic) => Self::InventoryChanged(inventory_changed(ic)),
            AC::RecipeUnlocked(r) => Self::RecipeUnlocked(recipe(r)),
            AC::RecipeCrafted(r) => Self::RecipeCrafted(recipe(r)),
            AC::PlayerGeneratesContainerLoot(lt) => {
                Self::PlayerGeneratesContainerLoot(lt.to_string())
            }
            AC::ChangedDimension { from, to } => Self::ChangedDimension(DimensionPredicate {
                from: from.map(ToString::to_string),
                to: to.map(ToString::to_string),
            }),
            AC::ConsumeItem(item) => Self::ConsumeItem(item_matching(ids(item.items))),
            AC::PlacedBlock { block, item } => Self::PlacedBlock(PlacedBlockPredicate {
                block: block.map(single),
                item: item.map(|i| item_matching(single(i))),
            }),
            AC::EnterBlock(b) => Self::EnterBlock(single(b)),
            AC::SlideDownBlock(b) => Self::SlideDownBlock(single(b)),
            AC::PlayerKilledEntity(e) => Self::PlayerKilledEntity(from_entity_condition(&e)),
            AC::BredAnimals(b) => Self::BredAnimals(bred_animals(b)),
            AC::TameAnimal(e) => Self::TameAnimal(from_entity_condition(&e)),
            AC::FilledBucket(item) => Self::FilledBucket(item_matching(single(item))),
            AC::FishingRodHooked(item) => Self::FishingRodHooked(item_matching(ids(item.items))),
            AC::UsingItem { item, looking_at } => Self::UsingItem {
                item: item_with_optional_tag(&item),
                looking_at: looking_at.map(single),
            },
            AC::UsedTotem(item) => Self::UsedTotem(item_matching(ids(item.items))),
            AC::EntityHurtPlayer(d) => Self::EntityHurtPlayer(damage(d)),
            AC::PlayerHurtEntity(d) => Self::PlayerHurtEntity(damage(d)),
            AC::TargetHit(th) => Self::TargetHit(target_hit(th)),
            AC::ConstructBeacon(b) => Self::ConstructBeacon(beacon(b)),
            AC::AllayDropItemOnBlock(c) => {
                Self::AllayDropItemOnBlock(from_item_used_on_block_condition(c))
            }
            AC::ThrownItemPickedUpByPlayer(e) => {
                Self::ThrownItemPickedUpByPlayer(from_entity_condition(&e))
            }
            AC::ItemUsedOnBlock(c) => Self::ItemUsedOnBlock(from_item_used_on_block_condition(c)),
            AC::Location(l) => Self::Location(location(l)),
            AC::KilledByArrow => Self::KilledByArrow,
            AC::SleptInBed => Self::SleptInBed,
            AC::EntityKilledPlayer => Self::EntityKilledPlayer,
            AC::UsedEnderEye => Self::UsedEnderEye,
            AC::CuredZombieVillager => Self::CuredZombieVillager,
            AC::VillagerTrade => Self::VillagerTrade,
            AC::EnchantedItem => Self::EnchantedItem,
            AC::BrewedPotion => Self::BrewedPotion,
            AC::ItemDurabilityChanged { item, vehicle_type } => Self::ItemDurabilityChanged {
                item: item_with_optional_tag(&item),
                vehicle_type: vehicle_type.map(single),
            },
            AC::Levitation { distance_y_min } => Self::Levitation {
                distance_y: distance_y_min.map(at_least),
            },
            AC::FallFromHeight { distance_y_min } => Self::FallFromHeight {
                distance_y: distance_y_min.map(at_least),
            },
            AC::FallAfterExplosion {
                distance_y_min,
                cause_entity,
            } => Self::FallAfterExplosion {
                distance_y: distance_y_min.map(at_least),
                cause_entity: cause_entity.map(ToString::to_string),
            },
            AC::NetherTravel {
                distance_horizontal_min,
            } => Self::NetherTravel {
                distance_horizontal: distance_horizontal_min.map(at_least),
            },
            AC::RideEntityInLava {
                distance_horizontal_min,
                dimension,
                vehicle_type,
            } => Self::RideEntityInLava {
                distance_horizontal: distance_horizontal_min.map(at_least),
                dimension: dimension.map(ToString::to_string),
                vehicle_type: vehicle_type.map(single),
            },
            AC::ShotCrossbow(item) => Self::ShotCrossbow(item_matching(ids(item.items))),
            AC::AvoidVibration => Self::AvoidVibration,
            AC::BeeNestDestroyed {
                block,
                num_bees_inside,
            } => Self::BeeNestDestroyed {
                block: block.map(ToString::to_string),
                num_bees_inside,
            },
            AC::ChanneledLightning { victim_type } => Self::ChanneledLightning {
                victim_type: victim_type.map(ToString::to_string),
            },
            AC::LightningStrike {
                bystander_type,
                blocks_set_on_fire,
                distance_max,
            } => Self::LightningStrike {
                bystander_type: bystander_type.map(ToString::to_string),
                blocks_set_on_fire,
                distance_max,
            },
            AC::KillMobNearSculkCatalyst => Self::KillMobNearSculkCatalyst,
            AC::HeroOfTheVillage => Self::HeroOfTheVillage,
            AC::PlayerInteractedWithEntity {
                entity,
                item,
                player,
            } => Self::PlayerInteractedWithEntity {
                entity: entity.as_ref().map(from_entity_condition),
                item: item.as_ref().map(from_item_condition),
                player: player.as_ref().map(from_entity_condition),
            },
            AC::ThrownItemPickedUpByEntity {
                entity,
                item,
                player,
            } => Self::ThrownItemPickedUpByEntity {
                entity: entity.as_ref().map(from_entity_condition),
                item: item.as_ref().map(from_item_condition),
                player: player.as_ref().map(from_entity_condition),
            },
            AC::PlayerShearedEquipment { entity, item } => Self::PlayerShearedEquipment {
                entity: entity.as_ref().map(from_entity_condition),
                item: item.as_ref().map(from_item_condition),
            },
            AC::StartedRiding(p) => Self::StartedRiding(p.as_ref().map(from_entity_condition)),
            AC::SummonedEntity(e) => Self::SummonedEntity(from_entity_condition(&e)),
            AC::CrafterRecipeCrafted(r) => Self::CrafterRecipeCrafted(r.to_string()),
            AC::EffectsChanged => Self::EffectsChanged,
            AC::SpearMobs { count } => Self::SpearMobs { count },
            AC::Impossible => Self::Impossible,
            AC::Empty | AC::Tick => Self::Tick,
            AC::Custom(raw) => Self::parse("", raw),
        }
    }
}

/// A matcher for exactly one identifier.
fn single(pattern: &str) -> TagOrIdMatcher {
    TagOrIdMatcher {
        patterns: vec![pattern.to_string()],
    }
}

/// A matcher for any of the identifiers.
fn ids(items: &[&str]) -> TagOrIdMatcher {
    TagOrIdMatcher {
        patterns: items.iter().map(ToString::to_string).collect(),
    }
}

/// An item predicate that only looks at which item it is.
const fn item_matching(items: TagOrIdMatcher) -> ItemPredicate {
    ItemPredicate {
        items: Some(items),
        count: None,
        durability: None,
        enchantments: Vec::new(),
    }
}

/// A tag written with the `#` that marks tags in predicates, whether or not it had one.
fn hash_tag(tag: &str) -> String {
    if tag.starts_with('#') {
        tag.to_string()
    } else {
        format!("#{tag}")
    }
}

const fn at_least(min: f64) -> FloatBounds {
    FloatBounds {
        min: Some(min),
        max: None,
    }
}

const fn at_least_int(min: i64) -> IntBounds {
    IntBounds {
        min: Some(min),
        max: None,
    }
}

/// The bounds when either end is given, none when neither is.
fn int_bounds(min: Option<i64>, max: Option<i64>) -> Option<IntBounds> {
    (min.is_some() || max.is_some()).then_some(IntBounds { min, max })
}

fn recipe(recipe: &str) -> RecipePredicate {
    RecipePredicate {
        recipes: vec![recipe.to_string()],
    }
}

fn inventory_changed(condition: &InventoryChangedCondition) -> InventoryChangedPredicate {
    InventoryChangedPredicate {
        items: condition.items.iter().map(inventory_item).collect(),
        slots: condition.slots.map(|s| InventorySlotsPredicate {
            empty: int_bounds(s.empty_min, s.empty_max),
            full: int_bounds(s.full_min, s.full_max),
            occupied: int_bounds(s.occupied_min, s.occupied_max),
        }),
    }
}

/// An item of an inventory condition: its ids and tag (the tag always gets a `#` in front), count
/// and enchantments.
fn inventory_item(item: &ItemCondition) -> ItemPredicate {
    ItemPredicate {
        items: if item.items.is_empty() && item.tag.is_none() {
            None
        } else {
            let mut patterns = Vec::new();
            if let Some(t) = item.tag {
                patterns.push(format!("#{t}"));
            }
            for i in item.items {
                patterns.push((*i).to_string());
            }
            Some(TagOrIdMatcher { patterns })
        },
        count: int_bounds(item.count_min, item.count_max),
        durability: None,
        enchantments: item
            .enchantments
            .iter()
            .map(|e| EnchantmentPredicate {
                enchantment: e.enchantment.to_string(),
                min_level: e.min_level,
            })
            .collect(),
    }
}

/// An item predicate from ids and an optional tag, nothing else.
fn item_with_optional_tag(item: &ItemCondition) -> ItemPredicate {
    ItemPredicate {
        items: (!item.items.is_empty() || item.tag.is_some()).then_some(TagOrIdMatcher {
            patterns: item
                .items
                .iter()
                .map(ToString::to_string)
                .chain(item.tag.map(hash_tag))
                .collect(),
        }),
        count: None,
        durability: None,
        enchantments: Vec::new(),
    }
}

fn bred_animals(condition: &BredAnimalsCondition) -> BredAnimalsPredicate {
    BredAnimalsPredicate {
        parent: condition.parent.as_ref().map(from_entity_condition),
        partner: condition.partner.as_ref().map(from_entity_condition),
        child: condition.child.as_ref().map(from_entity_condition),
    }
}

fn damage(condition: DamageCondition) -> DamagePredicate {
    DamagePredicate {
        blocked: condition.blocked,
        deflector: condition.is_projectile,
        dealt: condition.dealt_min.map(at_least),
        taken: condition.taken_min.map(at_least),
    }
}

fn target_hit(condition: TargetHitCondition) -> TargetHitPredicate {
    TargetHitPredicate {
        signal_strength: condition
            .signal_strength
            .map(|sig| at_least_int(i64::from(sig))),
        distance: condition.distance_min.map(at_least),
    }
}

fn beacon(condition: BeaconCondition) -> BeaconPredicate {
    BeaconPredicate {
        level: condition.level_min.map(|lvl| at_least_int(i64::from(lvl))),
    }
}

fn location(condition: &LocationCondition) -> LocationPredicate {
    LocationPredicate {
        x: None,
        y: None,
        z: None,
        biome: condition.biome.map(single),
        dimension: condition.dimension.map(ToString::to_string),
        block: condition.block.map(single),
        can_see_sky: condition.can_see_sky,
        smokey: condition.smokey,
        structure: condition.structure.map(ToString::to_string),
        stepping_on: condition.stepping_on.map(single),
        feet_item: condition.feet_item.map(single),
    }
}

fn from_entity_condition(e: &EntityCondition) -> EntityPredicate {
    let mut patterns = Vec::new();
    if let Some(t) = e.entity_type {
        patterns.push(t.to_string());
    }
    if let Some(t) = e.tag {
        patterns.push(hash_tag(t));
    }
    let mut vpatterns = Vec::new();
    if let Some(vt) = e.vehicle_type {
        vpatterns.push(vt.to_string());
    }
    if let Some(vt) = e.vehicle_tag {
        vpatterns.push(hash_tag(vt));
    }
    let passenger_type = e.passenger_type.map(single);
    let looking_at = e.looking_at.map(single);
    let feet_item = e.feet_item.map(single);

    EntityPredicate {
        entity_type: (!patterns.is_empty()).then_some(TagOrIdMatcher { patterns }),
        distance_horizontal: e.distance_horizontal_min.map(at_least),
        distance_absolute: e.distance_absolute_min.map(at_least),
        is_baby: e.is_baby,
        vehicle_type: (!vpatterns.is_empty()).then_some(TagOrIdMatcher {
            patterns: vpatterns,
        }),
        passenger_type,
        stepping_on: e.stepping_on.map(ToString::to_string),
        looking_at,
        feet_item,
        safe_armor_disallowed: e.safe_armor_disallowed,
    }
}

fn from_item_used_on_block_condition(c: &ItemUsedOnBlockCondition) -> ItemUsedOnBlockPredicate {
    let mut block_patterns: Vec<String> = c.blocks.iter().map(ToString::to_string).collect();
    if let Some(tag) = c.block_tag {
        block_patterns.push(hash_tag(tag));
    }
    let mut item_patterns: Vec<String> = c.items.iter().map(ToString::to_string).collect();
    if let Some(tag) = c.item_tag {
        item_patterns.push(hash_tag(tag));
    }
    ItemUsedOnBlockPredicate {
        blocks: (!block_patterns.is_empty()).then_some(TagOrIdMatcher {
            patterns: block_patterns,
        }),
        items: (!item_patterns.is_empty()).then_some(TagOrIdMatcher {
            patterns: item_patterns,
        }),
        state_key: c.state_key.map(ToString::to_string),
        state_value: c.state_value.map(ToString::to_string),
        smokey: c.smokey,
        biome: c.biome.map(ToString::to_string),
    }
}

fn from_item_condition(i: &ItemCondition) -> ItemPredicate {
    let mut patterns: Vec<String> = i.items.iter().map(ToString::to_string).collect();
    if let Some(t) = i.tag {
        patterns.push(hash_tag(t));
    }
    ItemPredicate {
        items: if patterns.is_empty() {
            None
        } else {
            Some(TagOrIdMatcher { patterns })
        },
        count: int_bounds(i.count_min, i.count_max),
        durability: int_bounds(i.durability_min, i.durability_max),
        enchantments: Vec::new(),
    }
}
