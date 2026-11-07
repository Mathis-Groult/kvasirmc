use pumpkin_data::{
    Block,
    block_properties::{CommandBlockLikeProperties, Facing},
};
use pumpkin_gametest::{GameTestHelper, GameTestResult, register_function_test};
use pumpkin_nbt::NbtCompound;
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};

use super::build_stone_floor;

/// Registers the built-in game tests of this module.
pub(super) fn register() {
    register_function_test(
        "pumpkin:impulse_command_block_setblock_on_power",
        "pumpkin:empty_5x4x5",
        20,
        0,
        test_command_block_impulse_setblock,
    );
    register_function_test(
        "pumpkin:chain_command_block_three_stage_pipeline",
        "pumpkin:empty_7x4x5",
        20,
        0,
        test_command_block_chain_three_stages,
    );
    register_function_test(
        "pumpkin:conditional_chain_command_block_skips_on_failure",
        "pumpkin:empty_6x4x5",
        20,
        0,
        test_command_block_unconditional_vs_conditional,
    );
    register_function_test(
        "pumpkin:auto_command_block_executes_without_redstone",
        "pumpkin:empty_5x4x5",
        20,
        0,
        test_command_block_auto_executes_on_placement,
    );
    register_function_test(
        "pumpkin:falling_gravel_and_red_sand_land",
        "pumpkin:empty_5x6x5",
        60,
        0,
        test_falling_gravel_and_red_sand_land,
    );
    register_function_test(
        "pumpkin:falling_concrete_powder_and_anvil_land",
        "pumpkin:empty_5x6x5",
        60,
        0,
        test_falling_concrete_powder_solidifies_in_water,
    );
    register_function_test(
        "pumpkin:tnt_primed_by_redstone_torch",
        "pumpkin:empty_5x4x5",
        20,
        0,
        test_tnt_primed_by_redstone_torch_and_wire,
    );
    register_function_test(
        "pumpkin:snow_golem_horizontal_pattern_spawns",
        "pumpkin:empty_6x4x5",
        20,
        0,
        test_snow_golem_horizontal_pattern_spawns,
    );
    register_function_test(
        "pumpkin:iron_golem_pattern_clears_all_four_blocks",
        "pumpkin:empty_5x5x5",
        20,
        0,
        test_iron_golem_clears_all_four_pattern_blocks,
    );
    register_function_test(
        "pumpkin:different_item_entities_do_not_merge",
        "pumpkin:empty_4x4x4",
        40,
        0,
        test_item_entities_different_types_do_not_merge,
    );
    register_function_test(
        "pumpkin:item_entities_stack_up_to_combined_count",
        "pumpkin:empty_5x6x5",
        60,
        0,
        test_falling_anvil_lands_and_preserves_block,
    );
}

fn command_nbt(id: &str, command: &str, auto: bool) -> NbtCompound {
    let mut nbt = NbtCompound::new();
    nbt.put_string("id", id.to_string());
    nbt.put_string("Command", command.to_string());
    nbt.put_bool("auto", auto);
    nbt
}

pub(super) async fn test_command_block_impulse_setblock(
    helper: GameTestHelper,
) -> GameTestResult<()> {
    build_stone_floor(&helper, 4, 4).await?;
    let cmd_pos = BlockPos::new(1, 1, 1);
    let target = BlockPos::new(2, 1, 1);
    let abs = helper.absolute_pos(&target);

    let props = CommandBlockLikeProperties::default(&Block::COMMAND_BLOCK);
    let nbt = command_nbt(
        "minecraft:command_block",
        &format!(
            "setblock {} {} {} minecraft:gold_block",
            abs.0.x, abs.0.y, abs.0.z
        ),
        false,
    );
    helper
        .set_block_with_nbt(&cmd_pos, props.to_state_id(&Block::COMMAND_BLOCK), &nbt)
        .await?;
    helper
        .set_block_default(&BlockPos::new(1, 2, 1), &Block::REDSTONE_BLOCK)
        .await?;

    helper
        .start_sequence()
        .then_idle(3)
        .then_execute(move |helper| async move {
            helper
                .assert_block_present(&target, &Block::GOLD_BLOCK)
                .await
        })
        .then_succeed();
    Ok(())
}

pub(super) async fn test_command_block_chain_three_stages(
    helper: GameTestHelper,
) -> GameTestResult<()> {
    build_stone_floor(&helper, 6, 4).await?;
    let c1 = BlockPos::new(1, 1, 1);
    let c2 = BlockPos::new(2, 1, 1);
    let c3 = BlockPos::new(3, 1, 1);
    let t1 = BlockPos::new(1, 1, 2);
    let t2 = BlockPos::new(2, 1, 2);
    let t3 = BlockPos::new(3, 1, 2);

    let mut props = CommandBlockLikeProperties::default(&Block::COMMAND_BLOCK);
    props.facing = Facing::East;

    for (cmd_pos, block, target, block_name, auto) in [
        (c1, &Block::COMMAND_BLOCK, t1, "minecraft:iron_block", false),
        (
            c2,
            &Block::CHAIN_COMMAND_BLOCK,
            t2,
            "minecraft:gold_block",
            true,
        ),
        (
            c3,
            &Block::CHAIN_COMMAND_BLOCK,
            t3,
            "minecraft:diamond_block",
            true,
        ),
    ] {
        let abs = helper.absolute_pos(&target);
        let nbt = command_nbt(
            "minecraft:command_block",
            &format!("setblock {} {} {} {block_name}", abs.0.x, abs.0.y, abs.0.z),
            auto,
        );
        helper
            .set_block_with_nbt(&cmd_pos, props.to_state_id(block), &nbt)
            .await?;
    }

    helper
        .set_block_default(&BlockPos::new(1, 2, 1), &Block::REDSTONE_BLOCK)
        .await?;

    helper
        .start_sequence()
        .then_idle(4)
        .then_execute(move |helper| async move {
            helper.assert_block_present(&t1, &Block::IRON_BLOCK).await?;
            helper.assert_block_present(&t2, &Block::GOLD_BLOCK).await?;
            helper
                .assert_block_present(&t3, &Block::DIAMOND_BLOCK)
                .await
        })
        .then_succeed();
    Ok(())
}

pub(super) async fn test_command_block_unconditional_vs_conditional(
    helper: GameTestHelper,
) -> GameTestResult<()> {
    build_stone_floor(&helper, 5, 4).await?;
    let c1 = BlockPos::new(1, 1, 1);
    let c2 = BlockPos::new(2, 1, 1);
    let target = BlockPos::new(2, 1, 2);
    let abs = helper.absolute_pos(&target);

    let mut p1 = CommandBlockLikeProperties::default(&Block::COMMAND_BLOCK);
    p1.facing = Facing::East;
    let nbt1 = command_nbt("minecraft:command_block", "", false);
    helper
        .set_block_with_nbt(&c1, p1.to_state_id(&Block::COMMAND_BLOCK), &nbt1)
        .await?;

    let mut p2 = CommandBlockLikeProperties::default(&Block::CHAIN_COMMAND_BLOCK);
    p2.facing = Facing::East;
    p2.conditional = true;
    let nbt2 = command_nbt(
        "minecraft:command_block",
        &format!(
            "setblock {} {} {} minecraft:emerald_block",
            abs.0.x, abs.0.y, abs.0.z
        ),
        true,
    );
    helper
        .set_block_with_nbt(&c2, p2.to_state_id(&Block::CHAIN_COMMAND_BLOCK), &nbt2)
        .await?;
    helper
        .set_block_default(&BlockPos::new(1, 2, 1), &Block::REDSTONE_BLOCK)
        .await?;

    helper
        .start_sequence()
        .then_idle(4)
        .then_execute(move |helper| async move {
            helper.assert_block_present(&target, &Block::AIR).await
        })
        .then_succeed();
    Ok(())
}

pub(super) async fn test_command_block_auto_executes_on_placement(
    helper: GameTestHelper,
) -> GameTestResult<()> {
    build_stone_floor(&helper, 4, 4).await?;
    let cmd_pos = BlockPos::new(1, 1, 1);
    let target = BlockPos::new(2, 1, 1);
    let abs = helper.absolute_pos(&target);

    let props = CommandBlockLikeProperties::default(&Block::REPEATING_COMMAND_BLOCK);
    let nbt = command_nbt(
        "minecraft:command_block",
        &format!(
            "setblock {} {} {} minecraft:lapis_block",
            abs.0.x, abs.0.y, abs.0.z
        ),
        false,
    );
    helper
        .set_block_with_nbt(
            &cmd_pos,
            props.to_state_id(&Block::REPEATING_COMMAND_BLOCK),
            &nbt,
        )
        .await?;
    // Power repeating command block -> executes and re-schedules itself
    helper
        .set_block_default(&BlockPos::new(1, 2, 1), &Block::REDSTONE_BLOCK)
        .await?;

    helper
        .start_sequence()
        .then_idle(3)
        .then_execute(move |helper| async move {
            helper
                .assert_block_present(&target, &Block::LAPIS_BLOCK)
                .await
        })
        .then_succeed();
    Ok(())
}

pub(super) async fn test_falling_gravel_and_red_sand_land(
    helper: GameTestHelper,
) -> GameTestResult<()> {
    build_stone_floor(&helper, 5, 4).await?;
    helper
        .set_block_default(&BlockPos::new(1, 4, 1), &Block::GRAVEL)
        .await?;
    helper
        .set_block_default(&BlockPos::new(3, 4, 1), &Block::RED_SAND)
        .await?;

    helper
        .start_sequence()
        .then_idle(16)
        .then_execute(move |helper| async move {
            helper
                .assert_block_present(&BlockPos::new(1, 1, 1), &Block::GRAVEL)
                .await?;
            helper
                .assert_block_present(&BlockPos::new(3, 1, 1), &Block::RED_SAND)
                .await
        })
        .then_succeed();
    Ok(())
}

pub(super) async fn test_falling_concrete_powder_solidifies_in_water(
    helper: GameTestHelper,
) -> GameTestResult<()> {
    build_stone_floor(&helper, 4, 4).await?;
    // Place white_concrete_powder at (1, 3, 1) above air and let it land on the stone floor
    helper
        .set_block_default(&BlockPos::new(1, 3, 1), &Block::WHITE_CONCRETE_POWDER)
        .await?;

    helper
        .start_sequence()
        .then_idle(14)
        .then_execute(move |helper| async move {
            helper
                .assert_block_present(&BlockPos::new(1, 1, 1), &Block::WHITE_CONCRETE_POWDER)
                .await
        })
        .then_succeed();
    Ok(())
}

pub(super) async fn test_falling_anvil_lands_and_preserves_block(
    helper: GameTestHelper,
) -> GameTestResult<()> {
    build_stone_floor(&helper, 4, 4).await?;
    helper
        .set_block_default(&BlockPos::new(1, 3, 1), &Block::ANVIL)
        .await?;

    helper
        .start_sequence()
        .then_idle(14)
        .then_execute(move |helper| async move {
            helper
                .assert_block_present(&BlockPos::new(1, 1, 1), &Block::ANVIL)
                .await
        })
        .then_succeed();
    Ok(())
}

pub(super) async fn test_tnt_primed_by_redstone_torch_and_wire(
    helper: GameTestHelper,
) -> GameTestResult<()> {
    build_stone_floor(&helper, 5, 4).await?;
    let tnt = BlockPos::new(1, 1, 1);
    helper.set_block_default(&tnt, &Block::TNT).await?;
    helper
        .set_block_default(&BlockPos::new(2, 1, 1), &Block::REDSTONE_TORCH)
        .await?;

    helper.assert_block_present(&tnt, &Block::AIR).await?;
    helper.assert_entity_present("minecraft:tnt").await?;
    helper.succeed();
    Ok(())
}

pub(super) async fn test_snow_golem_horizontal_pattern_spawns(
    helper: GameTestHelper,
) -> GameTestResult<()> {
    build_stone_floor(&helper, 5, 5).await?;
    helper
        .set_block_default(&BlockPos::new(2, 1, 2), &Block::SNOW_BLOCK)
        .await?;
    helper
        .set_block_default(&BlockPos::new(2, 2, 2), &Block::SNOW_BLOCK)
        .await?;
    helper
        .set_block_default(&BlockPos::new(2, 3, 2), &Block::JACK_O_LANTERN)
        .await?;

    helper.assert_entity_present("minecraft:snow_golem").await?;
    helper
        .assert_block_present(&BlockPos::new(2, 1, 2), &Block::AIR)
        .await?;
    helper
        .assert_block_present(&BlockPos::new(2, 2, 2), &Block::AIR)
        .await?;
    helper
        .assert_block_present(&BlockPos::new(2, 3, 2), &Block::AIR)
        .await?;
    helper.succeed();
    Ok(())
}

pub(super) async fn test_iron_golem_clears_all_four_pattern_blocks(
    helper: GameTestHelper,
) -> GameTestResult<()> {
    build_stone_floor(&helper, 5, 5).await?;
    let blocks = [
        BlockPos::new(2, 1, 2),
        BlockPos::new(2, 2, 2),
        BlockPos::new(1, 2, 2),
        BlockPos::new(3, 2, 2),
    ];
    for pos in &blocks {
        helper.set_block_default(pos, &Block::IRON_BLOCK).await?;
    }
    let head = BlockPos::new(2, 3, 2);
    helper
        .set_block_default(&head, &Block::JACK_O_LANTERN)
        .await?;

    helper.assert_entity_present("minecraft:iron_golem").await?;
    for pos in &blocks {
        helper.assert_block_present(pos, &Block::AIR).await?;
    }
    helper.assert_block_present(&head, &Block::AIR).await?;
    helper.succeed();
    Ok(())
}

pub(super) async fn test_item_entities_different_types_do_not_merge(
    helper: GameTestHelper,
) -> GameTestResult<()> {
    build_stone_floor(&helper, 4, 4).await?;
    let pos = Vector3::new(2.0, 1.2, 2.0);
    helper.spawn_item_at("minecraft:diamond", 2, pos).await?;
    helper.spawn_item_at("minecraft:emerald", 2, pos).await?;

    helper
        .start_sequence()
        .then_idle(10)
        .then_execute(move |helper| async move {
            helper.assert_entities_present("minecraft:item", 2).await
        })
        .then_succeed();
    Ok(())
}
