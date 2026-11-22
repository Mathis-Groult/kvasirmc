//! Tests for the mob equipment system.

use super::{
    ARMOR_POPULATION_ORDER, ArmorConfig, ArmorSlotEntry, DEFAULT_EQUIPMENT_DROP_CHANCE,
    MobEquipmentDef, RegionalDifficulty, WeaponConfig, WeaponEntry, equip_mob_from_def,
    moon_brightness,
};
use pumpkin_data::data_component_impl::EquipmentSlot;
use pumpkin_data::item::Item;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_util::difficulty::Difficulty;

#[test]
fn moon_brightness_matches_the_vanilla_phase_table() {
    let expected = [1.0, 0.75, 0.5, 0.25, 0.0, 0.25, 0.5, 0.75];
    for (phase, want) in expected.into_iter().enumerate() {
        let time_of_day = phase as i64 * 24000;
        assert!(
            (moon_brightness(time_of_day) - want).abs() < f32::EPSILON,
            "phase {phase}: got {}, want {want}",
            moon_brightness(time_of_day)
        );
    }
}

#[test]
fn moon_brightness_wraps_every_eight_days() {
    for phase in 0..8i64 {
        assert!(
            (moon_brightness(phase * 24000) - moon_brightness((phase + 8) * 24000)).abs()
                < f32::EPSILON,
            "phase {phase} does not wrap"
        );
    }
}

#[test]
fn mob_spawn_equipment_enchantment_provider_applies_enchantments() {
    use pumpkin_data::data_component_impl::EquipmentSlot;
    use pumpkin_data::item::Item;
    use pumpkin_data::item_stack::ItemStack;

    let mut stack = ItemStack::new(1, &Item::DIAMOND_SWORD);
    super::apply_vanilla_enchantments(&mut stack, &EquipmentSlot::MAIN_HAND, 1.0);
    assert!(stack.has_enchantments());
}

#[test]
fn pillager_crossbow_enchantments() {
    use pumpkin_data::Enchantment;
    use pumpkin_data::item::Item;
    use pumpkin_data::item_stack::ItemStack;

    let mut stack = ItemStack::new(1, &Item::CROSSBOW);
    super::apply_pillager_crossbow_enchantments(&mut stack, None);
    assert_eq!(stack.get_enchantment_level(&Enchantment::PIERCING), 1);

    let mut stack3 = ItemStack::new(1, &Item::CROSSBOW);
    super::apply_pillager_crossbow_enchantments(&mut stack3, Some(3));
    assert_eq!(stack3.get_enchantment_level(&Enchantment::QUICK_CHARGE), 1);

    let mut stack5 = ItemStack::new(1, &Item::CROSSBOW);
    super::apply_pillager_crossbow_enchantments(&mut stack5, Some(5));
    assert_eq!(stack5.get_enchantment_level(&Enchantment::QUICK_CHARGE), 2);
}

#[test]
fn vindicator_weapon_enchantments() {
    use pumpkin_data::Enchantment;
    use pumpkin_data::item::Item;
    use pumpkin_data::item_stack::ItemStack;

    let mut stack = ItemStack::new(1, &Item::IRON_AXE);
    super::apply_vindicator_weapon_enchantments(&mut stack, Some(1));
    assert_eq!(stack.get_enchantment_level(&Enchantment::SHARPNESS), 1);

    let mut stack5 = ItemStack::new(1, &Item::IRON_AXE);
    super::apply_vindicator_weapon_enchantments(&mut stack5, Some(5));
    assert_eq!(stack5.get_enchantment_level(&Enchantment::SHARPNESS), 2);
}

// ── equip_mob_from_def: deterministic levers only ──
// `special_multiplier` 0.0 makes `should_happen` always false; 1000.0 makes
// it always true; chances 1.0 / 0.0 make `rand::random::<f32>() < chance`
// always true / false.

static SWORD_ONLY: [WeaponEntry; 1] = [WeaponEntry {
    item: &Item::DIAMOND_SWORD,
    weight: 1.0,
}];
static GOLD_ARMOR: [ArmorSlotEntry; 4] = [
    ArmorSlotEntry {
        slot: &EquipmentSlot::HEAD,
        item: &Item::GOLDEN_HELMET,
        chance: 1.0,
    },
    ArmorSlotEntry {
        slot: &EquipmentSlot::CHEST,
        item: &Item::GOLDEN_CHESTPLATE,
        chance: 0.0,
    },
    ArmorSlotEntry {
        slot: &EquipmentSlot::LEGS,
        item: &Item::GOLDEN_LEGGINGS,
        chance: 0.0,
    },
    ArmorSlotEntry {
        slot: &EquipmentSlot::FEET,
        item: &Item::GOLDEN_BOOTS,
        chance: 1.0,
    },
];

const fn difficulty(base: Difficulty, special_multiplier: f32) -> RegionalDifficulty {
    RegionalDifficulty {
        base_difficulty: base,
        effective_difficulty: 3.0,
        special_multiplier,
    }
}

const fn def(weapon: WeaponConfig, armor: ArmorConfig, enchanted: bool) -> MobEquipmentDef {
    MobEquipmentDef {
        entity_type: "test",
        weapon,
        armor,
        enchanted,
        can_pick_up_loot: false,
    }
}

fn equip(
    def: &MobEquipmentDef,
    base: Difficulty,
    multiplier: f32,
) -> Vec<(EquipmentSlot, ItemStack, f32)> {
    equip_mob_from_def(def, &difficulty(base, multiplier))
}

#[test]
fn equip_always_weapon_goes_to_main_hand_with_default_drop_chance() {
    let d = def(WeaponConfig::Always(&Item::BOW), ArmorConfig::None, true);
    let out = equip(&d, Difficulty::Normal, 0.0);
    assert_eq!(out.len(), 1);
    assert!(out[0].0 == EquipmentSlot::MAIN_HAND);
    assert_eq!(out[0].1.item.id, Item::BOW.id);
    assert!(!out[0].1.has_enchantments());
    assert!((out[0].2 - DEFAULT_EQUIPMENT_DROP_CHANCE).abs() < f32::EPSILON);
}

#[test]
fn equip_no_weapon_and_no_armor_is_empty() {
    let d = def(WeaponConfig::None, ArmorConfig::None, true);
    assert!(equip(&d, Difficulty::Hard, 1000.0).is_empty());
}

#[test]
fn equip_always_weighted_weapon_picks_from_the_table() {
    let d = def(
        WeaponConfig::AlwaysWeighted(&SWORD_ONLY),
        ArmorConfig::None,
        false,
    );
    let out = equip(&d, Difficulty::Normal, 0.0);
    assert_eq!(out.len(), 1);
    assert!(out[0].0 == EquipmentSlot::MAIN_HAND);
    assert_eq!(out[0].1.item.id, Item::DIAMOND_SWORD.id);
    assert!((out[0].2 - DEFAULT_EQUIPMENT_DROP_CHANCE).abs() < f32::EPSILON);
}

#[test]
fn equip_chance_weapon_follows_the_difficulty_dependent_chance() {
    let on_hard_only = def(
        WeaponConfig::Chance {
            on_hard: 1.0,
            otherwise: 0.0,
            items: &SWORD_ONLY,
        },
        ArmorConfig::None,
        false,
    );
    let hard = equip(&on_hard_only, Difficulty::Hard, 0.0);
    assert_eq!(hard.len(), 1);
    assert!(hard[0].0 == EquipmentSlot::MAIN_HAND);
    assert_eq!(hard[0].1.item.id, Item::DIAMOND_SWORD.id);
    assert!((hard[0].2 - DEFAULT_EQUIPMENT_DROP_CHANCE).abs() < f32::EPSILON);
    assert!(equip(&on_hard_only, Difficulty::Normal, 0.0).is_empty());

    let otherwise_only = def(
        WeaponConfig::Chance {
            on_hard: 0.0,
            otherwise: 1.0,
            items: &SWORD_ONLY,
        },
        ArmorConfig::None,
        false,
    );
    assert!(equip(&otherwise_only, Difficulty::Hard, 0.0).is_empty());
    assert_eq!(equip(&otherwise_only, Difficulty::Normal, 0.0).len(), 1);
}

#[test]
fn equip_custom_per_slot_armor_keeps_slot_order_and_chances() {
    let d = def(
        WeaponConfig::None,
        ArmorConfig::CustomPerSlot(&GOLD_ARMOR),
        false,
    );
    let out = equip(&d, Difficulty::Normal, 0.0);
    assert_eq!(out.len(), 2);
    assert!(out[0].0 == EquipmentSlot::HEAD);
    assert_eq!(out[0].1.item.id, Item::GOLDEN_HELMET.id);
    assert!(out[1].0 == EquipmentSlot::FEET);
    assert_eq!(out[1].1.item.id, Item::GOLDEN_BOOTS.id);
    for (_, _, chance) in &out {
        assert!((chance - DEFAULT_EQUIPMENT_DROP_CHANCE).abs() < f32::EPSILON);
    }
}

#[test]
fn equip_vanilla_armor_is_absent_when_the_multiplier_is_zero() {
    let d = def(WeaponConfig::None, ArmorConfig::Vanilla, true);
    assert!(equip(&d, Difficulty::Hard, 0.0).is_empty());
}

#[test]
fn equip_vanilla_armor_fills_head_first_in_population_order() {
    let d = def(WeaponConfig::None, ArmorConfig::Vanilla, false);
    for _ in 0..50 {
        let out = equip(&d, Difficulty::Normal, 1000.0);
        assert!((1..=4).contains(&out.len()));
        for (i, (slot, stack, chance)) in out.iter().enumerate() {
            assert!(*slot == ARMOR_POPULATION_ORDER[i]);
            assert!(!stack.has_enchantments());
            assert!((chance - DEFAULT_EQUIPMENT_DROP_CHANCE).abs() < f32::EPSILON);
        }
    }
}

#[test]
fn equip_enchants_every_branch_when_the_checks_always_pass() {
    let always = def(
        WeaponConfig::Always(&Item::DIAMOND_SWORD),
        ArmorConfig::None,
        true,
    );
    let weighted = def(
        WeaponConfig::AlwaysWeighted(&SWORD_ONLY),
        ArmorConfig::None,
        true,
    );
    let chance = def(
        WeaponConfig::Chance {
            on_hard: 1.0,
            otherwise: 1.0,
            items: &SWORD_ONLY,
        },
        ArmorConfig::None,
        true,
    );
    let custom = def(
        WeaponConfig::None,
        ArmorConfig::CustomPerSlot(&GOLD_ARMOR),
        true,
    );
    let vanilla = def(WeaponConfig::None, ArmorConfig::Vanilla, true);
    for d in [&always, &weighted, &chance, &custom, &vanilla] {
        let out = equip(d, Difficulty::Normal, 1000.0);
        assert!(!out.is_empty());
        for (_, stack, _) in &out {
            assert!(stack.has_enchantments());
        }
    }
}

#[test]
fn equip_never_enchants_when_the_def_is_not_enchanted() {
    let d = def(
        WeaponConfig::Always(&Item::DIAMOND_SWORD),
        ArmorConfig::CustomPerSlot(&GOLD_ARMOR),
        false,
    );
    let out = equip(&d, Difficulty::Normal, 1000.0);
    assert_eq!(out.len(), 3);
    assert!(out.iter().all(|(_, stack, _)| !stack.has_enchantments()));
}

#[test]
fn equip_skips_enchanting_when_the_check_fails() {
    let weighted = def(
        WeaponConfig::AlwaysWeighted(&SWORD_ONLY),
        ArmorConfig::None,
        true,
    );
    let chance = def(
        WeaponConfig::Chance {
            on_hard: 1.0,
            otherwise: 1.0,
            items: &SWORD_ONLY,
        },
        ArmorConfig::None,
        true,
    );
    let custom = def(
        WeaponConfig::None,
        ArmorConfig::CustomPerSlot(&GOLD_ARMOR),
        true,
    );
    for d in [&weighted, &chance, &custom] {
        let out = equip(d, Difficulty::Normal, 0.0);
        assert!(!out.is_empty());
        assert!(out.iter().all(|(_, stack, _)| !stack.has_enchantments()));
    }
}

#[test]
fn equip_vanilla_armor_enchants_only_some_pieces_at_the_check_chance() {
    // multiplier 1.0: pieces appear with chance 0.15 and each is enchanted
    // with chance 0.5, so 200 pieces without both outcomes has odds of 2^-199.
    let d = def(WeaponConfig::None, ArmorConfig::Vanilla, true);
    let (mut enchanted, mut plain) = (0, 0);
    while enchanted + plain < 200 {
        for (_, stack, _) in equip(&d, Difficulty::Normal, 1.0) {
            if stack.has_enchantments() {
                enchanted += 1;
            } else {
                plain += 1;
            }
        }
    }
    assert!(
        enchanted > 0 && plain > 0,
        "enchanted {enchanted}, plain {plain}"
    );
}
