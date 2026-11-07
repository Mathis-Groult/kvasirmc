mod bubble_columns;
mod buttons_levers_plates_variants;
mod commands_and_entities_extended;
mod comparators_extended;
mod composter_and_cauldron;
mod containers_and_commands;
mod containers_and_hoppers_extended;
mod dispenser_utilities;
mod dispensers_and_crafters;
mod dispensers_and_droppers_extended;
mod doors_and_gates;
mod doors_trapdoors_gates_variants;
mod entities_and_ai;
mod fluids_and_sponge;
mod furnace_smelting;
mod golems;
mod lamps_bulbs_and_noteblocks_extended;
mod note_and_comparators;
mod pistons;
mod pistons_extended;
mod plants_and_support_blocks;
mod rails;
mod rails_and_fluids_extended;
mod redstone_gates;
mod redstone_inputs;
mod redstone_lamps_and_bulbs;
mod redstone_logic_circuits;
mod redstone_wire_and_power;
mod scaffolding;
mod target_and_projectiles;
mod tripwire;
mod walls_stairs_and_shapes;

use std::sync::Once;

use pumpkin_data::Block;
use pumpkin_gametest::{GameTestHelper, GameTestResult};
use pumpkin_util::math::position::BlockPos;

static REGISTER_BUILTIN_TESTS: Once = Once::new();

const BUILTIN_TEST_IDS: &[&str] = &[
    "pumpkin:redstone_torch_inverter",
    "pumpkin:redstone_repeater_delay",
    "pumpkin:redstone_comparator_modes",
    "pumpkin:composter_and_cauldron_comparators",
    "pumpkin:end_portal_frame_and_respawn_anchor_comparators",
    "pumpkin:note_block_instrument_and_power",
    "pumpkin:redstone_observer_pulse",
    "pumpkin:stone_button_pulse",
    "pumpkin:lever_toggles_power",
    "pumpkin:pressure_plate_entity_activation",
    "pumpkin:target_block_projectile_hit",
    "pumpkin:copper_bulb_toggles_and_comparator",
    "pumpkin:redstone_lamp_delayed_off",
    "pumpkin:piston_and_sticky_piston",
    "pumpkin:dispenser_and_dropper",
    "pumpkin:dispenser_honeycomb_beehive_and_shulker",
    "pumpkin:crafter_crafts_into_chest",
    "pumpkin:doors_trapdoors_fence_gates_redstone",
    "pumpkin:water_lava_interactions",
    "pumpkin:sponge_absorbs_water",
    "pumpkin:bubble_column_upward_and_downward",
    "pumpkin:falling_sand_lands",
    "pumpkin:scaffolding_distance_and_collapse",
    "pumpkin:command_block_chain_execution",
    "pumpkin:hopper_transfers_to_chest",
    "pumpkin:tnt_redstone_ignites",
    "pumpkin:furnace_ignites_and_smelts",
    "pumpkin:powered_rail_propagation",
    "pumpkin:activator_rail_propagation",
    "pumpkin:detector_rail_minecart",
    "pumpkin:tripwire_entity_collision",
    "pumpkin:snow_golem_pattern_spawns",
    "pumpkin:iron_golem_pattern_spawns",
    "pumpkin:wither_pattern_spawns",
    "pumpkin:item_entities_merge",
    "pumpkin:zombie_pathfinds_to_villager",
    // redstone_wire_and_power (14)
    "pumpkin:redstone_wire_attenuation_1_to_4",
    "pumpkin:redstone_wire_attenuation_5_to_8",
    "pumpkin:redstone_wire_attenuation_9_to_12",
    "pumpkin:redstone_wire_attenuation_13_to_15",
    "pumpkin:redstone_wire_corner_ne_and_sw",
    "pumpkin:redstone_wire_t_junction",
    "pumpkin:redstone_wire_cross_4_way",
    "pumpkin:redstone_wire_step_up_climb",
    "pumpkin:redstone_wire_step_down_drop",
    "pumpkin:redstone_wire_dual_sources_max",
    "pumpkin:redstone_wire_source_removal_deenergizes",
    "pumpkin:redstone_wire_repeater_reboosts_to_15",
    "pumpkin:redstone_wire_strong_power_through_stone",
    "pumpkin:redstone_block_powers_all_horizontal_wires",
    // redstone_logic_circuits (12)
    "pumpkin:redstone_double_inverter_buffer",
    "pumpkin:redstone_wall_torch_inverter",
    "pumpkin:redstone_or_gate",
    "pumpkin:redstone_nor_gate",
    "pumpkin:redstone_and_gate_three_torches",
    "pumpkin:redstone_nand_gate_two_torches",
    "pumpkin:redstone_rs_nor_latch",
    "pumpkin:redstone_repeater_delay_2_ticks",
    "pumpkin:redstone_repeater_delay_3_ticks",
    "pumpkin:redstone_repeater_delay_4_ticks",
    "pumpkin:redstone_repeater_locking_holds_off",
    "pumpkin:redstone_repeater_locking_holds_on",
    // comparators_extended (14)
    "pumpkin:comparator_composter_levels_1_to_4",
    "pumpkin:comparator_composter_levels_5_to_8",
    "pumpkin:comparator_water_cauldron_levels_1_to_3",
    "pumpkin:comparator_powder_snow_cauldron_levels_1_to_3",
    "pumpkin:comparator_respawn_anchor_charges_0_to_4",
    "pumpkin:comparator_cake_bites_0_2_4_6",
    "pumpkin:comparator_candle_cake_and_lit_candle_cake",
    "pumpkin:comparator_chest_single_item_signal_1",
    "pumpkin:comparator_chest_nonstackable_items_signal",
    "pumpkin:comparator_hopper_partial_and_full",
    "pumpkin:comparator_dispenser_and_dropper_signal",
    "pumpkin:comparator_furnace_slots_signal",
    "pumpkin:comparator_copper_bulb_oxidized_variants",
    "pumpkin:comparator_subtract_clamps_to_zero",
    // buttons_levers_plates_variants (12)
    "pumpkin:oak_button_30_tick_pulse",
    "pumpkin:polished_blackstone_button_20_tick_pulse",
    "pumpkin:button_floor_and_ceiling_power",
    "pumpkin:button_wall_strong_powers_behind_block",
    "pumpkin:button_breaks_when_support_removed",
    "pumpkin:lever_wall_and_ceiling_orientations",
    "pumpkin:lever_strong_powers_supporting_block",
    "pumpkin:lever_breaks_when_support_removed",
    "pumpkin:stone_pressure_plate_ignores_items_activates_on_mob",
    "pumpkin:wooden_pressure_plate_activates_on_item_and_clears",
    "pumpkin:light_weighted_pressure_plate_counts_entities",
    "pumpkin:heavy_weighted_pressure_plate_signal_1",
    // doors_trapdoors_gates_variants (12)
    "pumpkin:oak_and_spruce_doors_redstone",
    "pumpkin:birch_and_jungle_doors_redstone",
    "pumpkin:acacia_dark_oak_mangrove_cherry_doors_redstone",
    "pumpkin:bamboo_crimson_warped_doors_redstone",
    "pumpkin:copper_doors_oxidation_variants_redstone",
    "pumpkin:door_breaking_lower_removes_upper",
    "pumpkin:trapdoor_wooden_variants_redstone",
    "pumpkin:copper_trapdoors_oxidation_variants_redstone",
    "pumpkin:fence_gate_wooden_variants_redstone",
    "pumpkin:fence_gate_in_wall_neighbor_update",
    "pumpkin:fence_horizontal_connections",
    "pumpkin:iron_bars_and_glass_pane_connections",
    // pistons_extended (12)
    "pumpkin:piston_pushes_upward_and_retracts",
    "pumpkin:piston_pushes_downward_and_retracts",
    "pumpkin:piston_pushes_north_and_south",
    "pumpkin:piston_pushes_west_and_east",
    "pumpkin:sticky_piston_pulls_upward_and_downward",
    "pumpkin:piston_pushes_multi_block_row",
    "pumpkin:piston_blocked_by_obsidian",
    "pumpkin:piston_blocked_by_bedrock",
    "pumpkin:piston_destroys_torch_and_moss_carpet",
    "pumpkin:sticky_piston_leaves_obsidian_behind",
    "pumpkin:sticky_piston_redstone_block_powers_lamp",
    "pumpkin:piston_one_stone_leaves_pushed_block",
    // dispensers_and_droppers_extended (12)
    "pumpkin:dispenser_lava_bucket_places_and_picks_up",
    "pumpkin:dispenser_powder_snow_bucket_places_and_picks_up",
    "pumpkin:dispenser_shears_harvest_full_beehive",
    "pumpkin:dispenser_glass_bottle_fills_from_water",
    "pumpkin:dispenser_flint_and_steel_primes_tnt",
    "pumpkin:dispenser_flint_and_steel_lights_campfire",
    "pumpkin:dispenser_armor_stand_spawns_entity",
    "pumpkin:dispenser_tnt_spawns_primed_tnt",
    "pumpkin:dispenser_minecart_places_on_rail",
    "pumpkin:dispenser_boat_places_on_water",
    "pumpkin:dropper_drops_bucket_as_item_entity",
    "pumpkin:dropper_transfers_into_hopper_and_barrel",
    // containers_and_hoppers_extended (12)
    "pumpkin:hopper_pulls_item_entity_from_above",
    "pumpkin:hopper_pulls_from_chest_above",
    "pumpkin:hopper_pushes_sideways_into_barrel",
    "pumpkin:hopper_chain_three_hoppers_to_chest",
    "pumpkin:hopper_locked_by_redstone_stops_transfer",
    "pumpkin:hopper_unlocks_when_redstone_removed",
    "pumpkin:hopper_feeds_furnace_top_and_side",
    "pumpkin:blast_furnace_smelts_raw_iron",
    "pumpkin:smoker_cooks_raw_beef",
    "pumpkin:crafter_crafts_iron_ingot_from_nuggets",
    "pumpkin:crafter_crafts_sticks_from_two_planks",
    "pumpkin:barrel_and_trapped_chest_store_items",
    // walls_stairs_and_shapes (12)
    "pumpkin:wall_straight_line_has_no_center_post",
    "pumpkin:wall_corner_raises_center_post",
    "pumpkin:wall_under_full_block_turns_tall",
    "pumpkin:wall_connects_to_iron_bars_and_glass_pane",
    "pumpkin:wall_connects_to_fence_gate",
    "pumpkin:stairs_inner_left_and_inner_right_shapes",
    "pumpkin:stairs_outer_left_and_outer_right_shapes",
    "pumpkin:stairs_revert_to_straight_when_neighbor_removed",
    "pumpkin:dirt_path_turns_to_dirt_under_solid_block",
    "pumpkin:farmland_turns_to_dirt_under_solid_block",
    "pumpkin:campfire_signal_fire_over_hay_block",
    "pumpkin:campfire_waterlogged_extinguishes_lit",
    // plants_and_support_blocks (12)
    "pumpkin:cactus_breaks_when_solid_block_placed_adjacent",
    "pumpkin:cactus_breaks_when_sand_below_removed",
    "pumpkin:sugar_cane_breaks_when_water_removed",
    "pumpkin:sugar_cane_tall_column_cascades_break",
    "pumpkin:torch_and_soul_torch_break_without_support",
    "pumpkin:wall_torch_breaks_when_wall_removed",
    "pumpkin:standing_lantern_breaks_when_floor_removed",
    "pumpkin:hanging_lantern_breaks_when_ceiling_removed",
    "pumpkin:lantern_on_fence_gate_breaks_when_gate_opens",
    "pumpkin:cake_and_candle_cake_break_without_support",
    "pumpkin:carpet_and_moss_carpet_break_without_support",
    "pumpkin:pressure_plate_and_redstone_wire_break_without_support",
    // lamps_bulbs_and_noteblocks_extended (12)
    "pumpkin:copper_bulb_variants_toggle_and_remember_lit",
    "pumpkin:waxed_copper_bulb_variants_toggle_and_remember_lit",
    "pumpkin:redstone_lamp_stays_lit_while_powered",
    "pumpkin:note_block_instruments_rock_sand_glass_wood",
    "pumpkin:note_block_instruments_clay_gold_wool_ice",
    "pumpkin:note_block_instruments_bone_iron_soul_pumpkin",
    "pumpkin:note_block_instruments_emerald_hay_glowstone_default",
    "pumpkin:note_block_mob_head_above_sets_instrument",
    "pumpkin:daylight_detector_normal_and_inverted_signal",
    "pumpkin:observer_detects_redstone_lamp_state_change",
    "pumpkin:observer_chain_propagates_pulse",
    "pumpkin:target_block_egg_hit_emits_pulse_and_clears",
    // rails_and_fluids_extended (12)
    "pumpkin:powered_rail_deenergizes_beyond_8_blocks",
    "pumpkin:activator_rail_deenergizes_beyond_8_blocks",
    "pumpkin:powered_rail_bidirectional_boost_from_center",
    "pumpkin:detector_rail_powers_adjacent_powered_rail",
    "pumpkin:detector_rail_activates_on_chest_minecart",
    "pumpkin:rail_breaks_when_supporting_block_removed",
    "pumpkin:water_flows_horizontally_to_neighbors",
    "pumpkin:water_flows_downward_into_hole",
    "pumpkin:lava_source_turned_to_obsidian_by_water",
    "pumpkin:water_flow_breaks_redstone_torch",
    "pumpkin:sponge_absorbs_multi_block_water_pool",
    "pumpkin:sponge_does_not_absorb_lava",
    // commands_and_entities_extended (11)
    "pumpkin:impulse_command_block_setblock_on_power",
    "pumpkin:chain_command_block_three_stage_pipeline",
    "pumpkin:conditional_chain_command_block_skips_on_failure",
    "pumpkin:auto_command_block_executes_without_redstone",
    "pumpkin:falling_gravel_and_red_sand_land",
    "pumpkin:falling_concrete_powder_and_anvil_land",
    "pumpkin:tnt_primed_by_redstone_torch",
    "pumpkin:snow_golem_horizontal_pattern_spawns",
    "pumpkin:iron_golem_pattern_clears_all_four_blocks",
    "pumpkin:different_item_entities_do_not_merge",
    "pumpkin:item_entities_stack_up_to_combined_count",
];

#[must_use]
pub const fn builtin_game_test_ids() -> &'static [&'static str] {
    BUILTIN_TEST_IDS
}

pub fn ensure_builtin_game_tests_registered() {
    REGISTER_BUILTIN_TESTS.call_once(|| {
        redstone_gates::register();
        composter_and_cauldron::register();
        note_and_comparators::register();
        redstone_inputs::register();
        target_and_projectiles::register();
        redstone_lamps_and_bulbs::register();
        pistons::register();
        dispensers_and_crafters::register();
        dispenser_utilities::register();
        doors_and_gates::register();
        fluids_and_sponge::register();
        bubble_columns::register();
        containers_and_commands::register();
        scaffolding::register();
        furnace_smelting::register();
        rails::register();
        tripwire::register();
        golems::register();
        entities_and_ai::register();
        redstone_wire_and_power::register();
        redstone_logic_circuits::register();
        comparators_extended::register();
        buttons_levers_plates_variants::register();
        doors_trapdoors_gates_variants::register();
        pistons_extended::register();
        dispensers_and_droppers_extended::register();
        containers_and_hoppers_extended::register();
        walls_stairs_and_shapes::register();
        plants_and_support_blocks::register();
        lamps_bulbs_and_noteblocks_extended::register();
        rails_and_fluids_extended::register();
        commands_and_entities_extended::register();
    });
}

pub(super) async fn build_stone_floor(
    helper: &GameTestHelper,
    width: i32,
    depth: i32,
) -> GameTestResult<()> {
    for x in 0..width {
        for z in 0..depth {
            helper
                .set_block_default(&BlockPos::new(x, 0, z), &Block::STONE)
                .await?;
        }
    }
    Ok(())
}
