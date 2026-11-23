//! Characterisation tests for `place_structure_with_controller_rotation`: a recording
//! world sees every call the placement makes, in order, for each structure rotation,
//! controller rotation, padding and block kind (plain, block entity, test block).

#![expect(clippy::expect_used, reason = "unit test assertions")]

use std::fmt::Write as _;
use std::sync::Mutex;

use async_trait::async_trait;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_nbt::{NbtCompound, tag::NbtTag};
use pumpkin_util::math::position::BlockPos;
use pumpkin_world::world::BlockFlags;

use super::{
    GameTestPosition, GameTestStructureBlock, GameTestStructureTemplate, TestBlockMode,
    place_structure, place_structure_with_controller_rotation,
};
use crate::error::{GameTestError, GameTestResult};
use crate::model::GameTestRotation;
use crate::world::GameTestWorld;

const SURFACE: i32 = 63;
const ROTATIONS: [GameTestRotation; 4] = [
    GameTestRotation::None,
    GameTestRotation::Clockwise90,
    GameTestRotation::Clockwise180,
    GameTestRotation::Counterclockwise90,
];

/// Records every world call as one text line; fails the call with index `fail_at`.
#[derive(Default)]
struct RecordingWorld {
    log: Mutex<Vec<String>>,
    fail_at: Option<usize>,
}

impl RecordingWorld {
    fn failing_at(index: usize) -> Self {
        Self {
            log: Mutex::new(Vec::new()),
            fail_at: Some(index),
        }
    }

    fn record(&self, line: String) -> GameTestResult<()> {
        let mut log = self
            .log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.fail_at == Some(log.len()) {
            return Err(GameTestError::World(format!("injected failure at {line}")));
        }
        log.push(line);
        Ok(())
    }

    fn lines(&self) -> Vec<String> {
        self.log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

fn pos(position: &BlockPos) -> String {
    format!("({},{},{})", position.0.x, position.0.y, position.0.z)
}

fn nbt_text(nbt: &NbtCompound) -> String {
    let mut keys: Vec<_> = nbt.child_tags.keys().collect();
    keys.sort();
    let parts: Vec<String> = keys
        .into_iter()
        .map(|key| match nbt.child_tags.get(key) {
            Some(NbtTag::Compound(inner)) => format!("{key}={{{}}}", nbt_text(inner)),
            Some(tag) => format!("{key}={tag:?}"),
            None => format!("{key}=?"),
        })
        .collect();
    parts.join(",")
}

#[async_trait]
impl GameTestWorld for RecordingWorld {
    async fn block_state_id(&self, _position: &BlockPos) -> BlockStateId {
        Block::AIR.default_state.id
    }

    async fn set_block_state(
        &self,
        position: &BlockPos,
        block_state_id: BlockStateId,
        flags: BlockFlags,
    ) -> GameTestResult<()> {
        self.record(format!(
            "set {} state={} flags={:#x}",
            pos(position),
            block_state_id.as_u16(),
            flags.bits()
        ))
    }

    /// Adds a distinct offset per rotation so the line shows which rotation was applied.
    async fn rotate_block_state(
        &self,
        block_state_id: BlockStateId,
        rotation: GameTestRotation,
    ) -> GameTestResult<BlockStateId> {
        let raw = block_state_id.as_u16();
        let rotated = BlockStateId::new_or_air(raw + 1000 * (rotation as u16 + 1));
        self.record(format!("rotate {raw} {rotation:?} -> {}", rotated.as_u16()))?;
        Ok(rotated)
    }

    async fn set_block_entity_nbt(
        &self,
        position: &BlockPos,
        nbt: &NbtCompound,
    ) -> GameTestResult<()> {
        self.record(format!("nbt {} {}", pos(position), nbt_text(nbt)))
    }

    async fn clear_non_player_entities(
        &self,
        min: &BlockPos,
        max: &BlockPos,
    ) -> GameTestResult<()> {
        self.record(format!("clear_entities {}..{}", pos(min), pos(max)))
    }

    async fn clear_scheduled_block_ticks(
        &self,
        min: &BlockPos,
        max: &BlockPos,
    ) -> GameTestResult<()> {
        self.record(format!("clear_ticks {}..{}", pos(min), pos(max)))
    }

    async fn clear_block_events(&self, min: &BlockPos, max: &BlockPos) -> GameTestResult<()> {
        self.record(format!("clear_events {}..{}", pos(min), pos(max)))
    }

    async fn set_test_instance_running(&self, _position: &BlockPos) -> GameTestResult<()> {
        Ok(())
    }

    async fn set_test_instance_success(&self, _position: &BlockPos) -> GameTestResult<()> {
        Ok(())
    }

    async fn set_test_instance_failure(
        &self,
        _position: &BlockPos,
        _message: &str,
        _marker: Option<(BlockPos, String)>,
    ) -> GameTestResult<()> {
        Ok(())
    }

    async fn trigger_test_block(&self, _position: &BlockPos) -> GameTestResult<()> {
        Ok(())
    }

    async fn reset_test_block(&self, _position: &BlockPos) -> GameTestResult<()> {
        Ok(())
    }

    async fn test_block_triggered(&self, _position: &BlockPos) -> GameTestResult<bool> {
        Ok(false)
    }

    async fn test_block_message(&self, _position: &BlockPos) -> GameTestResult<String> {
        Ok(String::new())
    }

    async fn surface_height(&self, x: i32, z: i32) -> i32 {
        // Not fallible: a surface query is recorded straight into the log.
        self.log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(format!("surface {x},{z}"));
        SURFACE
    }
}

/// Collapses each run of air-clearing sets into one line (first, last, count) so a
/// padded test box stays readable while its bounds and size are still pinned.
fn collapse(lines: &[String]) -> Vec<String> {
    let air = format!("state={}", Block::AIR.default_state.id.as_u16());
    let clear_flags = (BlockFlags::NOTIFY_LISTENERS
        | BlockFlags::SKIP_DROPS
        | BlockFlags::SKIP_REDSTONE_WIRE_STATE_REPLACEMENT
        | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK)
        .bits();
    let tail = format!("{air} flags={clear_flags:#x}");
    let mut out: Vec<String> = Vec::new();
    let mut run: Vec<&String> = Vec::new();
    let flush = |run: &mut Vec<&String>, out: &mut Vec<String>| {
        if let (Some(first), Some(last)) = (run.first(), run.last()) {
            out.push(format!(
                "air-clear x{} first {first} last {last}",
                run.len()
            ));
        }
        run.clear();
    };
    for line in lines {
        if line.starts_with("set ") && line.ends_with(&tail) {
            run.push(line);
        } else {
            flush(&mut run, &mut out);
            out.push(line.clone());
        }
    }
    flush(&mut run, &mut out);
    out
}

fn block(position: [i32; 3], state: BlockStateId) -> GameTestStructureBlock {
    GameTestStructureBlock {
        position,
        state,
        nbt: None,
        test_mode: None,
    }
}

/// 2x1x3 template with a plain block, a block entity, a test block and a block that
/// carries both NBT and a test mode (the NBT wins).
fn mixed_template() -> GameTestStructureTemplate {
    let stone = Block::STONE.default_state.id;
    let granite = Block::GRANITE.default_state.id;
    let diorite = Block::DIORITE.default_state.id;
    let andesite = Block::ANDESITE.default_state.id;
    let mut chest_nbt = NbtCompound::new();
    chest_nbt.put_string("id", "minecraft:chest".to_string());
    chest_nbt.put_int("marker", 7);
    let mut both_nbt = NbtCompound::new();
    both_nbt.put_string("id", "minecraft:sign".to_string());

    let mut chest = block([1, 0, 2], granite);
    chest.nbt = Some(chest_nbt);
    let mut accept = block([0, 0, 1], diorite);
    accept.test_mode = Some(TestBlockMode::Accept);
    let mut both = block([1, 0, 0], andesite);
    both.nbt = Some(both_nbt);
    both.test_mode = Some(TestBlockMode::Fail);

    GameTestStructureTemplate::new(
        [2, 1, 3],
        vec![block([0, 0, 0], stone), chest, accept, both],
    )
    .expect("mixed template is valid")
}

fn empty_template(size: [i32; 3]) -> GameTestStructureTemplate {
    GameTestStructureTemplate::empty(size).expect("empty template is valid")
}

/// Compares with the recorded text; `PLACEMENT_BLESS=1` rewrites the record instead.
fn check(name: &str, actual: &str) {
    let path = format!(
        "{}/src/structure/placement_tests/{name}.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    if std::env::var_os("PLACEMENT_BLESS").is_some() {
        std::fs::write(&path, actual).expect("record is writable");
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(actual, expected, "record {path}");
}

async fn scene(
    template: &GameTestStructureTemplate,
    rotation: GameTestRotation,
    controller: GameTestRotation,
    position: GameTestPosition,
    padding: i32,
) -> String {
    let world = RecordingWorld::default();
    let result = place_structure_with_controller_rotation(
        &world,
        template,
        "pumpkin:demo",
        rotation,
        controller,
        position,
        padding,
    )
    .await;
    let mut text = String::new();
    match result {
        Ok(instance) => {
            let _ = writeln!(
                text,
                "ok controller={} origin={} source={:?} size={:?} rotation={:?}",
                pos(instance.test_instance_pos()),
                pos(instance.origin()),
                instance.source_size(),
                instance.size(),
                instance.rotation()
            );
        }
        Err(error) => {
            let _ = writeln!(text, "err {error:?}");
        }
    }
    for line in collapse(&world.lines()) {
        let _ = writeln!(text, "  {line}");
    }
    text
}

#[tokio::test]
async fn places_blocks_under_each_structure_rotation() {
    let template = mixed_template();
    let mut all = String::new();
    for rotation in ROTATIONS {
        let _ = writeln!(all, "== {rotation:?}");
        all += &scene(
            &template,
            rotation,
            GameTestRotation::None,
            GameTestPosition::new(10, Some(70), 20),
            0,
        )
        .await;
    }
    check("rotations", &all);
}

#[tokio::test]
async fn stores_the_controller_rotation_apart_from_the_structure_rotation() {
    let template = empty_template([3, 2, 1]);
    let mut all = String::new();
    for controller in ROTATIONS {
        let _ = writeln!(all, "== controller {controller:?}");
        all += &scene(
            &template,
            GameTestRotation::Clockwise90,
            controller,
            GameTestPosition::new(-4, Some(5), 9),
            0,
        )
        .await;
    }
    check("controller", &all);
}

#[tokio::test]
async fn pads_the_test_box_and_reads_the_surface_only_without_a_height() {
    let template = mixed_template();
    let mut all = String::new();
    for (name, y, padding) in [("surface pad 2", None, 2), ("fixed y pad 1", Some(-3), 1)] {
        let _ = writeln!(all, "== {name}");
        all += &scene(
            &template,
            GameTestRotation::Counterclockwise90,
            GameTestRotation::Clockwise180,
            GameTestPosition::new(3, y, -8),
            padding,
        )
        .await;
    }
    check("padding", &all);
}

#[tokio::test]
async fn place_structure_uses_no_controller_rotation() {
    let world = RecordingWorld::default();
    let template = empty_template([1, 1, 1]);
    let placed = place_structure(
        &world,
        &template,
        "pumpkin:demo",
        GameTestRotation::Clockwise180,
        GameTestPosition::new(0, Some(1), 0),
        0,
    )
    .await;
    assert!(placed.is_ok());
    let nbt_line = world
        .lines()
        .into_iter()
        .find(|line| line.starts_with("nbt "))
        .unwrap_or_default();
    assert!(nbt_line.contains("rotation=String(\"none\")"), "{nbt_line}");
}

#[tokio::test]
async fn stops_at_the_first_failing_world_call() {
    let template = mixed_template();
    let complete = RecordingWorld::default();
    let placed = place_structure_with_controller_rotation(
        &complete,
        &template,
        "pumpkin:demo",
        GameTestRotation::Clockwise90,
        GameTestRotation::None,
        GameTestPosition::new(0, Some(0), 0),
        1,
    )
    .await;
    assert!(placed.is_ok());
    let total = complete.lines().len();
    for index in 0..total {
        let world = RecordingWorld::failing_at(index);
        let failed = place_structure_with_controller_rotation(
            &world,
            &template,
            "pumpkin:demo",
            GameTestRotation::Clockwise90,
            GameTestRotation::None,
            GameTestPosition::new(0, Some(0), 0),
            1,
        )
        .await;
        assert!(failed.is_err(), "call {index} must propagate its failure");
        assert_eq!(
            world.lines().len(),
            index,
            "nothing runs after call {index}"
        );
    }
}
