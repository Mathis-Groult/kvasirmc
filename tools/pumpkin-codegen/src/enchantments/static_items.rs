//! The fixed part of the generated enchantment file: types and methods that do not depend on the data.

use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn imports_and_level_based_value_tokens() -> TokenStream {
    quote! {
        use crate::item::Item;
        use crate::tag::Enchantment as EnchantmentTag;
        use crate::tag::Item as ItemTag;
        use crate::tag::DamageType as DamageTypeTag;
        use crate::tag::EntityType as EntityTypeTag;
        use crate::tag::{RegistryKey, Tag, Taggable};
        use crate::data_component_impl::EnchantmentsImpl;
        use pumpkin_util::text::TextComponent;
        use pumpkin_util::text::color::NamedColor;
        use std::hash::{Hash, Hasher};
        use std::slice::Iter;

        #[derive(Clone, Debug, PartialEq)]
        pub enum LevelBasedValue {
            Constant(f32),
            Linear {
                base: f32,
                per_level_above_first: f32,
            },
            Clamped {
                value: &'static Self,
                min: f32,
                max: f32,
            },
            Fraction {
                numerator: &'static Self,
                denominator: &'static Self,
            },
            LevelsSquared {
                added: f32,
            },
            Lookup {
                values: &'static [f32],
                fallback: &'static Self,
            },
        }

        impl LevelBasedValue {
            #[must_use]
            pub fn calculate(&self, level: i32) -> f32 {
                match self {
                    Self::Constant(val) => *val,
                    Self::Linear {
                        base,
                        per_level_above_first,
                    } => base + (level.max(1) - 1) as f32 * per_level_above_first,
                    Self::Clamped { value, min, max } => value.calculate(level).clamp(*min, *max),
                    Self::Fraction {
                        numerator,
                        denominator,
                    } => {
                        let denom = denominator.calculate(level);
                        if denom == 0.0 {
                            0.0
                        } else {
                            numerator.calculate(level) / denom
                        }
                    }
                    Self::LevelsSquared { added } => ((level * level) as f32) + added,
                    Self::Lookup { values, fallback } => {
                        let idx = (level - 1) as usize;
                        values
                            .get(idx)
                            .copied()
                            .unwrap_or_else(|| fallback.calculate(level))
                    }
                }
            }
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum EnchantmentTarget {
            Attacker,
            DamagingEntity,
            Victim,
        }

        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct TargetedConditionalEffect<T> {
            pub enchanted: Option<EnchantmentTarget>,
            pub affected: Option<EnchantmentTarget>,
            pub effect: T,
        }
    }
}

pub(super) fn conditional_effect_tokens() -> TokenStream {
    quote! {
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct ConditionalEffect<T> {
            pub effect: T,
        }

        #[derive(Clone, Debug, PartialEq)]
        pub enum ReplaceDiskPredicate {
            MatchingBlockTag {
                offset: pumpkin_util::math::vector3::Vector3<i32>,
                tag: &'static crate::tag::Tag,
            },
            MatchingBlocks {
                offset: pumpkin_util::math::vector3::Vector3<i32>,
                blocks: &'static [&'static str],
            },
            MatchingFluids {
                offset: pumpkin_util::math::vector3::Vector3<i32>,
                fluids: &'static [&'static str],
            },
            Unobstructed,
            AllOf(&'static [ReplaceDiskPredicate]),
        }
    }
}

pub(super) fn enchantment_entity_effect_tokens() -> TokenStream {
    quote! {
        #[derive(Clone, Debug, PartialEq)]
        pub enum EnchantmentEntityEffect {
            Ignite {
                duration: LevelBasedValue,
            },
            DamageEntity {
                min_damage: LevelBasedValue,
                max_damage: LevelBasedValue,
                damage_type: Option<&'static crate::damage::DamageType>,
            },
            ChangeItemDamage {
                amount: LevelBasedValue,
            },
            PlaySound {
                sound: &'static str,
            },
            ReplaceBlock {
                offset_x: i32,
                offset_y: i32,
                offset_z: i32,
                trigger_game_event: Option<crate::game_event::GameEvent>,
            },
            SetBlockProperties {
                properties: &'static [(&'static str, &'static str)],
                offset_x: i32,
                offset_y: i32,
                offset_z: i32,
                trigger_game_event: Option<crate::game_event::GameEvent>,
            },
            ReplaceDisk {
                radius: LevelBasedValue,
                height: LevelBasedValue,
                offset_x: i32,
                offset_y: i32,
                offset_z: i32,
                predicate: Option<ReplaceDiskPredicate>,
                block_state: &'static crate::block_state::BlockState,
                trigger_game_event: Option<crate::game_event::GameEvent>,
            },
            SummonEntity {
                entity_types: &'static [&'static crate::entity::EntityType],
                join_team: bool,
            },
            SpawnParticles {
                particle: crate::particle::Particle,
                horizontal_position: PositionSource,
                vertical_position: PositionSource,
                horizontal_velocity: VelocitySource,
                vertical_velocity: VelocitySource,
                speed: pumpkin_util::math::float_provider::FloatProvider,
            },
            RunFunction {
                function: &'static str,
            },
            ApplyExhaustion {
                amount: LevelBasedValue,
            },
            ApplyImpulse {
                direction: pumpkin_util::math::vector3::Vector3<f64>,
                coordinate_scale: pumpkin_util::math::vector3::Vector3<f64>,
                magnitude: LevelBasedValue,
            },
            ApplyMobEffect {
                to_apply: &'static [&'static crate::effect::StatusEffect],
                min_duration: LevelBasedValue,
                max_duration: LevelBasedValue,
                min_amplifier: LevelBasedValue,
                max_amplifier: LevelBasedValue,
            },
            Explode {
                attribute_to_user: bool,
                damage_type: Option<&'static crate::damage::DamageType>,
                knockback_multiplier: Option<LevelBasedValue>,
                immune_blocks: Option<&'static str>,
                offset_x: f64,
                offset_y: f64,
                offset_z: f64,
                radius: LevelBasedValue,
                create_fire: bool,
                block_interaction: &'static str,
                small_particle: Option<crate::particle::Particle>,
                large_particle: Option<crate::particle::Particle>,
                sound: Option<crate::sound::Sound>,
            },
            AllOf(&'static [EnchantmentEntityEffect]),
            Other,
        }
    }
}

pub(super) fn position_source_type_tokens() -> TokenStream {
    quote! {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
        pub enum PositionSourceType {
            #[default]
            EntityPosition,
            InBoundingBox,
        }

        impl PositionSourceType {
            #[must_use]
            pub fn get_coordinate(
                self,
                position: f64,
                center: f64,
                bounding_box_span: f32,
                random: &mut impl pumpkin_util::random::RandomImpl,
            ) -> f64 {
                match self {
                    Self::EntityPosition => position,
                    Self::InBoundingBox => {
                        let random_offset = f64::from(random.next_f32()) - 0.5;
                        center + random_offset * f64::from(bounding_box_span)
                    }
                }
            }
        }

        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct PositionSource {
            pub source_type: PositionSourceType,
            pub offset: f32,
            pub scale: f32,
        }

        impl PositionSource {
            #[must_use]
            pub const fn new(source_type: PositionSourceType, offset: f32, scale: f32) -> Self {
                Self {
                    source_type,
                    offset,
                    scale,
                }
            }

            #[must_use]
            pub const fn offset_from_entity_position(offset: f32) -> Self {
                Self {
                    source_type: PositionSourceType::EntityPosition,
                    offset,
                    scale: 1.0,
                }
            }

            #[must_use]
            pub const fn in_bounding_box() -> Self {
                Self {
                    source_type: PositionSourceType::InBoundingBox,
                    offset: 0.0,
                    scale: 1.0,
                }
            }

            #[must_use]
            pub fn get_coordinate(
                &self,
                position: f64,
                center: f64,
                bounding_box_span: f32,
                random: &mut impl pumpkin_util::random::RandomImpl,
            ) -> f64 {
                self.source_type.get_coordinate(
                    position,
                    center,
                    bounding_box_span * self.scale,
                    random,
                ) + f64::from(self.offset)
            }
        }

        #[derive(Clone, Debug, PartialEq)]
        pub struct VelocitySource {
            pub movement_scale: f32,
            pub base: pumpkin_util::math::float_provider::FloatProvider,
        }
    }
}

pub(super) fn velocity_source_tokens() -> TokenStream {
    quote! {
        impl VelocitySource {
            #[must_use]
            pub const fn new(
                movement_scale: f32,
                base: pumpkin_util::math::float_provider::FloatProvider,
            ) -> Self {
                Self {
                    movement_scale,
                    base,
                }
            }

            #[must_use]
            pub const fn movement_scaled(scale: f32) -> Self {
                Self {
                    movement_scale: scale,
                    base: pumpkin_util::math::float_provider::FloatProvider::Constant(0.0),
                }
            }

            #[must_use]
            pub const fn fixed_velocity(
                provider: pumpkin_util::math::float_provider::FloatProvider,
            ) -> Self {
                Self {
                    movement_scale: 0.0,
                    base: provider,
                }
            }

            pub fn get_velocity(
                &self,
                movement: f64,
                random: &mut impl pumpkin_util::random::RandomImpl,
            ) -> f64 {
                f64::from(self.movement_scale).mul_add(movement, f64::from(self.base.get(random)))
            }
        }

        #[derive(Clone, Debug, PartialEq)]
        pub enum EnchantmentValueEffect {
            Add(LevelBasedValue),
            Multiply(LevelBasedValue),
            Set(LevelBasedValue),
            RemoveBinomial(LevelBasedValue),
            Other,
        }

        impl EnchantmentValueEffect {
            #[must_use]
            pub fn process(&self, level: i32, current_value: f32) -> f32 {
                match self {
                    Self::Add(val) => current_value + val.calculate(level),
                    Self::Multiply(val) => current_value * val.calculate(level),
                    Self::Set(val) => val.calculate(level),
                    Self::RemoveBinomial(val) => {
                        let prob = val.calculate(level);
                        if rand::random::<f32>() < prob {
                            0.0
                        } else {
                            current_value
                        }
                    }
                    Self::Other => current_value,
                }
            }
        }
    }
}

pub(super) fn enchantment_effects_tokens() -> TokenStream {
    quote! {
        #[derive(Clone, Debug)]
        pub struct EnchantmentEffects {
            pub projectile_spawned: &'static [ConditionalEffect<EnchantmentEntityEffect>],
            pub post_attack: &'static [TargetedConditionalEffect<EnchantmentEntityEffect>],
            pub projectile_count: &'static [ConditionalEffect<EnchantmentValueEffect>],
            pub projectile_spread: &'static [ConditionalEffect<EnchantmentValueEffect>],
            pub projectile_piercing: &'static [ConditionalEffect<EnchantmentValueEffect>],
            pub ammo_use: &'static [ConditionalEffect<EnchantmentValueEffect>],
            pub damage: &'static [ConditionalEffect<EnchantmentValueEffect>],
            pub knockback: &'static [ConditionalEffect<EnchantmentValueEffect>],
            pub armor_effectiveness: &'static [ConditionalEffect<EnchantmentValueEffect>],
            pub damage_protection: &'static [ConditionalEffect<EnchantmentValueEffect>],
            pub hit_block: &'static [ConditionalEffect<EnchantmentEntityEffect>],
            pub item_damage: &'static [ConditionalEffect<EnchantmentValueEffect>],
            pub equipment_drops: &'static [TargetedConditionalEffect<EnchantmentValueEffect>],
            pub fishing_time_reduction: &'static [ConditionalEffect<EnchantmentValueEffect>],
            pub fishing_luck_bonus: &'static [ConditionalEffect<EnchantmentValueEffect>],
            pub block_experience: &'static [ConditionalEffect<EnchantmentValueEffect>],
            pub mob_experience: &'static [ConditionalEffect<EnchantmentValueEffect>],
            pub repair_with_xp: &'static [ConditionalEffect<EnchantmentValueEffect>],
            pub smash_damage_per_fallen_block: &'static [ConditionalEffect<EnchantmentValueEffect>],
            pub trident_return_acceleration: &'static [ConditionalEffect<EnchantmentValueEffect>],
            pub trident_spin_attack_strength: Option<EnchantmentValueEffect>,
            pub crossbow_charge_time: Option<EnchantmentValueEffect>,
            pub location_changed: &'static [ConditionalEffect<EnchantmentEntityEffect>],
            pub prevent_armor_change: bool,
            pub prevent_equipment_drop: bool,
        }

        pub struct Enchantment {
            pub id: u8,
            pub name: &'static str,
            pub registry_key: &'static str,
            pub description: &'static str, // TODO use TextComponent
            pub anvil_cost: u32,
            pub supported_items: &'static Tag,
            pub primary_items: Option<&'static Tag>,
            pub exclusive_set: Option<&'static Tag>,
            pub max_level: i32,
            pub slots: &'static [AttributeModifierSlot],
            pub weight: i32,
            pub min_cost: Cost,
            pub max_cost: Cost,
            pub effects: EnchantmentEffects,
        }

        #[derive(Clone, Copy, Debug)]
        pub struct Cost {
            pub base: i32,
            pub per_level_above_first: i32,
        }
    }
}

pub(super) fn cost_tokens() -> TokenStream {
    quote! {
        impl Cost {
            pub fn calculate(&self, level: i32) -> i32 {
                self.base + self.per_level_above_first * (level - 1)
            }
        }
        impl Taggable for Enchantment {
            #[inline]
            fn tag_key() -> RegistryKey {
                RegistryKey::Enchantment
            }
            #[inline]
            fn registry_key(&self) -> &str {
                self.registry_key
            }
            #[inline]
            fn registry_id(&self) -> u16 {
                self.id as u16
            }
        }
        impl PartialEq for Enchantment {
            fn eq(&self, other: &Self) -> bool {
                self.id == other.id
            }
        }
        impl Eq for Enchantment {}
        impl Hash for Enchantment {
            fn hash<H: Hasher>(&self, state: &mut H) {
                self.id.hash(state);
            }
        }
        #[derive(Debug, Clone, Hash, PartialEq)]
        pub enum AttributeModifierSlot {
            Any,
            MainHand,
            OffHand,
            Hand,
            Feet,
            Legs,
            Chest,
            Head,
            Armor,
            Body,
            Saddle,
        }
    }
}

pub(super) fn can_enchant_tokens() -> TokenStream {
    quote! {
        pub fn can_enchant(&self, item: &'static Item) -> bool {
            self.supported_items.1.contains(&item.id)
        }
        pub fn is_primary_item(&self, item: &'static Item) -> bool {
            if let Some(primary) = self.primary_items {
                primary.1.contains(&item.id)
            } else {
                self.can_enchant(item)
            }
        }
        pub fn are_compatible(&self, other: &'static Enchantment) -> bool {
            if self == other {
                return false;
            }
            if let Some(tag) = self.exclusive_set && tag.1.contains(&(other.id as u16)) {
                return false;
            }
            if let Some(tag) = other.exclusive_set && tag.1.contains(&(self.id as u16)) {
                return false;
            }
            true
        }
        pub fn is_enchantment_compatible(&self, other: &EnchantmentsImpl) -> bool {
            for (i, _) in other.enchantment.iter() {
                if !self.are_compatible(i) {
                    return false;
                }
            }
            true
        }
        #[allow(deprecated)]
        pub fn get_fullname(&self, level: i32) -> TextComponent {
            let mut ret = TextComponent::translate(self.description, []).color_named(
                if self.has_tag(&EnchantmentTag::MINECRAFT_CURSE) {
                    NamedColor::Red
                } else {
                    NamedColor::Gray
                }
            );
            if level != 1 || self.max_level != 1 {
                ret = ret.add_text(" ")
                    .add_child(TextComponent::translate("enchantment.level.".to_string() + &level.to_string(), []));
            }
            ret
        }

        pub fn modify_damage_protection(&self, level: i32, protection: &mut f32) {
            for effect in self.effects.damage_protection {
                *protection = effect.effect.process(level, *protection);
            }
        }

        pub fn modify_damage(&self, level: i32, amount: &mut f64) {
            for effect in self.effects.damage {
                *amount = f64::from(effect.effect.process(level, *amount as f32));
            }
        }

        pub fn modify_fall_based_damage(&self, level: i32, amount: &mut f64) {
            for effect in self.effects.smash_damage_per_fallen_block {
                *amount = f64::from(effect.effect.process(level, *amount as f32));
            }
        }

        pub fn modify_knockback(&self, level: i32, amount: &mut f32) {
            for effect in self.effects.knockback {
                *amount = effect.effect.process(level, *amount);
            }
        }

        pub fn modify_armor_effectiveness(&self, level: i32, amount: &mut f32) {
            for effect in self.effects.armor_effectiveness {
                *amount = effect.effect.process(level, *amount);
            }
        }

        pub fn modify_durability_change(&self, level: i32, change: &mut f32) {
            for effect in self.effects.item_damage {
                *change = effect.effect.process(level, *change);
            }
        }
    }
}

pub(super) fn modify_ammo_count_tokens() -> TokenStream {
    quote! {
        pub fn modify_ammo_count(&self, level: i32, change: &mut f32) {
            for effect in self.effects.ammo_use {
                *change = effect.effect.process(level, *change);
            }
        }

        pub fn modify_piercing_count(&self, level: i32, count: &mut f32) {
            for effect in self.effects.projectile_piercing {
                *count = effect.effect.process(level, *count);
            }
        }

        pub fn modify_block_experience(&self, level: i32, count: &mut f32) {
            for effect in self.effects.block_experience {
                *count = effect.effect.process(level, *count);
            }
        }

        pub fn modify_mob_experience(&self, level: i32, experience: &mut f32) {
            for effect in self.effects.mob_experience {
                *experience = effect.effect.process(level, *experience);
            }
        }

        pub fn modify_durability_to_repair_from_xp(&self, level: i32, change: &mut f32) {
            for effect in self.effects.repair_with_xp {
                *change = effect.effect.process(level, *change);
            }
        }

        pub fn modify_trident_return_to_owner_acceleration(&self, level: i32, count: &mut f32) {
            for effect in self.effects.trident_return_acceleration {
                *count = effect.effect.process(level, *count);
            }
        }

        pub fn modify_trident_spin_attack_strength(&self, level: i32, strength: &mut f32) {
            if let Some(effect) = &self.effects.trident_spin_attack_strength {
                *strength = effect.process(level, *strength);
            }
        }

        pub fn modify_fishing_time_reduction(&self, level: i32, time_reduction: &mut f32) {
            for effect in self.effects.fishing_time_reduction {
                *time_reduction = effect.effect.process(level, *time_reduction);
            }
        }

        pub fn modify_fishing_luck_bonus(&self, level: i32, luck: &mut f32) {
            for effect in self.effects.fishing_luck_bonus {
                *luck = effect.effect.process(level, *luck);
            }
        }

        pub fn modify_projectile_count(&self, level: i32, count: &mut f32) {
            for effect in self.effects.projectile_count {
                *count = effect.effect.process(level, *count);
            }
        }

        pub fn modify_projectile_spread(&self, level: i32, angle: &mut f32) {
            for effect in self.effects.projectile_spread {
                *angle = effect.effect.process(level, *angle);
            }
        }

        pub fn modify_crossbow_charge_time(&self, level: i32, time: &mut f32) {
            if let Some(effect) = &self.effects.crossbow_charge_time {
                *time = effect.process(level, *time);
            }
        }

        pub fn get_projectile_spawned_effects(&self) -> &'static [ConditionalEffect<EnchantmentEntityEffect>] {
            self.effects.projectile_spawned
        }

        pub fn get_location_changed_effects(&self) -> &'static [ConditionalEffect<EnchantmentEntityEffect>] {
            self.effects.location_changed
        }

        pub fn get_post_attack_effects(
            &self,
        ) -> &'static [TargetedConditionalEffect<EnchantmentEntityEffect>] {
            self.effects.post_attack
        }
    }
}

pub(super) fn modify_damage_against_tokens() -> TokenStream {
    quote! {
        pub fn modify_damage_against(
            &self,
            level: i32,
            damage: &mut f64,
            victim_type: Option<&crate::entity::EntityType>,
        ) {
            let is_applicable = if self == &Self::SHARPNESS {
                true
            } else if self == &Self::SMITE {
                victim_type.is_some_and(|t| t.has_tag(&EntityTypeTag::MINECRAFT_SENSITIVE_TO_SMITE))
            } else if self == &Self::BANE_OF_ARTHROPODS {
                victim_type.is_some_and(|t| t.has_tag(&EntityTypeTag::MINECRAFT_SENSITIVE_TO_BANE_OF_ARTHROPODS))
            } else if self == &Self::IMPALING {
                victim_type.is_some_and(|t| t.has_tag(&EntityTypeTag::MINECRAFT_SENSITIVE_TO_IMPALING))
            } else {
                false
            };

            if is_applicable {
                self.modify_damage(level, damage);
            }
        }

        pub fn modify_damage_protection_against(
            &self,
            level: i32,
            damage_type: &crate::damage::DamageType,
            protection: &mut f32,
        ) {
            let is_applicable = if self == &Self::PROTECTION {
                !damage_type.has_tag(&DamageTypeTag::MINECRAFT_BYPASSES_INVULNERABILITY)
                    && damage_type != &crate::damage::DamageType::STARVE
                    && damage_type != &crate::damage::DamageType::GENERIC_KILL
                    && damage_type != &crate::damage::DamageType::OUT_OF_WORLD
            } else if self == &Self::FIRE_PROTECTION {
                damage_type.has_tag(&DamageTypeTag::MINECRAFT_IS_FIRE)
            } else if self == &Self::BLAST_PROTECTION {
                damage_type.has_tag(&DamageTypeTag::MINECRAFT_IS_EXPLOSION)
            } else if self == &Self::PROJECTILE_PROTECTION {
                damage_type.has_tag(&DamageTypeTag::MINECRAFT_IS_PROJECTILE)
            } else if self == &Self::FEATHER_FALLING {
                damage_type.has_tag(&DamageTypeTag::MINECRAFT_IS_FALL)
            } else {
                false
            };

            if is_applicable {
                self.modify_damage_protection(level, protection);
            }
        }

        pub fn modify_durability_damage(&self, level: i32, is_armor: bool, damage: &mut f32) {
            if self == &Self::UNBREAKING {
                let effect = if is_armor {
                    self.effects.item_damage.first()
                } else {
                    self.effects.item_damage.get(1).or_else(|| self.effects.item_damage.first())
                };
                if let Some(effect) = effect {
                    *damage = effect.effect.process(level, *damage);
                }
            } else {
                self.modify_durability_change(level, damage);
            }
        }
    }
}

pub(super) fn tests_tokens() -> TokenStream {
    quote! {
        #[cfg(test)]
        mod tests {
            use super::*;

            #[test]
            fn sharpness_damage() {
                let mut dmg = 0.0;
                Enchantment::SHARPNESS.modify_damage_against(1, &mut dmg, None);
                assert_eq!(dmg, 1.0);
                let mut dmg5 = 0.0;
                Enchantment::SHARPNESS.modify_damage_against(5, &mut dmg5, None);
                assert_eq!(dmg5, 3.0);
            }

            #[test]
            fn smite_damage() {
                let mut dmg = 0.0;
                Enchantment::SMITE.modify_damage_against(1, &mut dmg, Some(&crate::entity::EntityType::ZOMBIE));
                assert_eq!(dmg, 2.5);
                let mut dmg_pig = 0.0;
                Enchantment::SMITE.modify_damage_against(1, &mut dmg_pig, Some(&crate::entity::EntityType::PIG));
                assert_eq!(dmg_pig, 0.0);
            }

            #[test]
            fn protection_absorb() {
                let mut prot = 0.0;
                Enchantment::PROTECTION.modify_damage_protection_against(4, &crate::damage::DamageType::GENERIC, &mut prot);
                assert_eq!(prot, 4.0);
                let mut fire_prot = 0.0;
                Enchantment::FIRE_PROTECTION.modify_damage_protection_against(4, &crate::damage::DamageType::IN_FIRE, &mut fire_prot);
                assert_eq!(fire_prot, 8.0);
            }

            #[test]
            fn primary_items() {
                assert!(Enchantment::SHARPNESS.primary_items.is_some());
                assert!(Enchantment::SHARPNESS.is_primary_item(&crate::item::Item::DIAMOND_SWORD));
                assert!(!Enchantment::SHARPNESS.is_primary_item(&crate::item::Item::BOOK));
            }

            #[test]
            fn knockback() {
                let mut kb = 0.0;
                Enchantment::KNOCKBACK.modify_knockback(2, &mut kb);
                assert_eq!(kb, 2.0);
            }

            #[test]
            fn breach_armor_effectiveness() {
                let mut eff = 1.0;
                Enchantment::BREACH.modify_armor_effectiveness(4, &mut eff);
                assert!((eff - 0.4).abs() < 1e-5);
            }
        }
    }
}
