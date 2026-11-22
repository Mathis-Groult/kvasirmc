//! A record of what `TreeFeature::update_leaves` does, made before it is split
//! (`tests/golden/tree_update_leaves.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! Every scene is built on a 3 x 3 grid of chunks with flat ground: logs, roots, foliage and
//! decoration lists, plus blocks set by hand (leaves of every state, other blocks, vines). The
//! scene runs at two origins (one in negative coordinates, both with lines crossing a chunk
//! edge); seeded random scenes follow the hand-made ones. Each line has the scene name, the list
//! sizes, the state of every leaf in the scene's box after the call (block, distance, persistent,
//! waterlogged) and every block the call changed (before > after).

use std::fmt::Write;

use pumpkin_data::block_properties::{
    BlockProperties, OakLeavesLikeProperties, VineLikeProperties,
};
use pumpkin_data::{Block, BlockState, BlockStateId};
use pumpkin_util::{
    math::position::BlockPos,
    random::{RandomGenerator, RandomImpl, xoroshiro128::Xoroshiro},
    world_seed::Seed,
};

use super::TreeFeature;
use crate::generation::generator::WorldGenerator;
use crate::generation::proto_chunk::{GenerationCache, ProtoChunk};
use crate::generation::proto_chunk_test::stage_parity::TestGridCache;
use crate::generation::{
    get_world_gen,
    positions::chunk_pos::{start_block_x, start_block_z},
};

const GROUND: i32 = 64;

struct Scene {
    name: String,
    logs: Vec<BlockPos>,
    roots: Vec<BlockPos>,
    foliage: Vec<BlockPos>,
    decorations: Vec<BlockPos>,
    setup: Vec<(BlockPos, &'static BlockState)>,
}

impl Scene {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            logs: Vec::new(),
            roots: Vec::new(),
            foliage: Vec::new(),
            decorations: Vec::new(),
            setup: Vec::new(),
        }
    }

    /// A leaf block, also listed as foliage.
    fn leaf(&mut self, pos: BlockPos, state: &'static BlockState) {
        self.setup.push((pos, state));
        self.foliage.push(pos);
    }

    /// A log block, also listed as a log.
    fn log(&mut self, pos: BlockPos) {
        self.setup.push((pos, Block::OAK_LOG.default_state));
        self.logs.push(pos);
    }
}

fn grid(world_gen: &WorldGenerator) -> TestGridCache {
    let mut chunks = std::collections::HashMap::new();
    for chunk_x in -1..=1 {
        for chunk_z in -1..=1 {
            let mut chunk = ProtoChunk::new(chunk_x, chunk_z, world_gen);
            for local_x in 0..16 {
                for local_z in 0..16 {
                    let x = start_block_x(chunk_x) + local_x;
                    let z = start_block_z(chunk_z) + local_z;
                    for y in 40..GROUND {
                        chunk.set_block_state(x, y, z, Block::STONE.default_state);
                    }
                }
            }
            chunks.insert((chunk_x, chunk_z), chunk);
        }
    }
    TestGridCache {
        center_pos: (0, 0),
        chunks,
    }
}

fn leaf_state(
    block: &Block,
    distance: u8,
    persistent: bool,
    waterlogged: bool,
) -> &'static BlockState {
    let mut props = OakLeavesLikeProperties::default(block);
    props.distance = distance;
    props.persistent = persistent;
    props.waterlogged = waterlogged;
    BlockState::from_id(props.to_state_id(block))
}

fn oak(distance: u8) -> &'static BlockState {
    leaf_state(&Block::OAK_LEAVES, distance, false, false)
}

fn vine_state(faces: [bool; 5]) -> &'static BlockState {
    let mut props = VineLikeProperties::default(&Block::VINE);
    props.up = faces[0];
    props.north = faces[1];
    props.east = faces[2];
    props.south = faces[3];
    props.west = faces[4];
    BlockState::from_id(props.to_state_id(&Block::VINE))
}

fn state_id(grid: &TestGridCache, pos: BlockPos) -> BlockStateId {
    GenerationCache::get_block_state(grid, &pos.0)
}

fn short_name(block: &Block) -> &'static str {
    block.name.strip_suffix("_leaves").unwrap_or(block.name)
}

fn describe(id: BlockStateId) -> String {
    let (block, state) = BlockState::from_id_with_block(id);
    if OakLeavesLikeProperties::handles_block_id(block.id) {
        let props = OakLeavesLikeProperties::from_state_id(state.id);
        format!(
            "{}:{}{}{}",
            short_name(block),
            props.distance,
            u8::from(props.persistent),
            u8::from(props.waterlogged)
        )
    } else {
        format!("{}#{}", block.name, id.as_u16())
    }
}

fn is_leaf(id: BlockStateId) -> bool {
    let (block, _) = BlockState::from_id_with_block(id);
    OakLeavesLikeProperties::handles_block_id(block.id)
}

/// The box of everything the scene lists or sets, one block wider on every side.
fn bounds(scene: &Scene) -> ([i32; 3], [i32; 3]) {
    let mut low = [i32::MAX; 3];
    let mut high = [i32::MIN; 3];
    let lists = scene
        .logs
        .iter()
        .chain(&scene.roots)
        .chain(&scene.foliage)
        .chain(&scene.decorations)
        .copied()
        .chain(scene.setup.iter().map(|(pos, _)| *pos));
    for pos in lists {
        let coords = [pos.0.x, pos.0.y, pos.0.z];
        for axis in 0..3 {
            low[axis] = low[axis].min(coords[axis] - 1);
            high[axis] = high[axis].max(coords[axis] + 1);
        }
    }
    (low, high)
}

fn run_scene(world_gen: &WorldGenerator, scene: &Scene) -> String {
    let mut before = grid(world_gen);
    let mut after = grid(world_gen);
    for (pos, state) in &scene.setup {
        GenerationCache::set_block_state(&mut before, &pos.0, state);
        GenerationCache::set_block_state(&mut after, &pos.0, state);
    }
    TreeFeature::update_leaves(
        &mut after,
        &scene.logs,
        &scene.roots,
        &scene.foliage,
        &scene.decorations,
    );
    let (low, high) = bounds(scene);
    let mut leaves = String::new();
    let mut changed = String::new();
    for x in low[0]..=high[0] {
        for y in low[1]..=high[1] {
            for z in low[2]..=high[2] {
                let pos = BlockPos::new(x, y, z);
                let (old, new) = (state_id(&before, pos), state_id(&after, pos));
                if is_leaf(new) {
                    let _ = write!(leaves, " {x},{y},{z}={}", describe(new));
                }
                if old != new {
                    let _ = write!(changed, " {x},{y},{z}={}>{}", describe(old), describe(new));
                }
            }
        }
    }
    format!(
        "{} logs {} roots {} foliage {} decorations {} leaves{} changed{}\n",
        scene.name,
        scene.logs.len(),
        scene.roots.len(),
        scene.foliage.len(),
        scene.decorations.len(),
        leaves,
        changed
    )
}

/// A position relative to a scene's origin.
type Offset = dyn Fn(i32, i32, i32) -> BlockPos;

fn line_scenes(at: &Offset) -> Vec<Scene> {
    let mut scenes = Vec::new();

    let mut line = Scene::new("line");
    line.log(at(0, 0, 0));
    for k in 1..=9 {
        line.leaf(at(k, 0, 0), oak(7));
    }
    scenes.push(line);

    let mut flags = Scene::new("line_flags");
    flags.log(at(0, 0, 0));
    for k in 1..=9 {
        let block = match k % 3 {
            0 => &Block::OAK_LEAVES,
            1 => &Block::SPRUCE_LEAVES,
            _ => &Block::BIRCH_LEAVES,
        };
        flags.leaf(at(k, 0, 0), leaf_state(block, 7, k % 2 == 1, k % 3 == 0));
    }
    scenes.push(flags);

    let mut pre = Scene::new("line_pre_set");
    pre.log(at(0, 0, 0));
    pre.log(at(10, 0, 0));
    for (k, distance) in [7, 7, 3, 7, 1, 7, 7, 4, 7].into_iter().enumerate() {
        pre.leaf(at(k as i32 + 1, 0, 0), oak(distance));
    }
    scenes.push(pre);

    let mut beyond = Scene::new("line_beyond");
    beyond.log(at(0, 0, 0));
    for (k, distance) in [7, 7, 7, 7, 7, 7, 7, 4, 5, 6, 3, 7].into_iter().enumerate() {
        beyond.leaf(at(k as i32 + 1, 0, 0), oak(distance));
    }
    scenes.push(beyond);

    let mut star = Scene::new("star");
    star.log(at(0, 0, 0));
    for k in 1..=8 {
        for (dx, dy, dz) in [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ] {
            star.leaf(at(dx * k, dy * k, dz * k), oak(7));
        }
    }
    scenes.push(star);
    scenes
}

fn blob_scenes(at: &Offset) -> Vec<Scene> {
    let mut scenes = Vec::new();

    let mut blob = Scene::new("blob");
    for y in 0..3 {
        blob.log(at(0, y, 0));
    }
    for dx in -4..=4i32 {
        for dy in -4..=4i32 {
            for dz in -4..=4i32 {
                if dx.abs() + dy.abs() + dz.abs() <= 4
                    && !(dx == 0 && dz == 0 && (0..3).contains(&(dy + 1)))
                {
                    blob.leaf(at(dx, dy + 1, dz), oak(7));
                }
            }
        }
    }
    scenes.push(blob);

    let mut two = Scene::new("two_logs");
    for y in 0..2 {
        two.log(at(0, y, 0));
        two.log(at(7, y, 0));
    }
    for x in -1..=8 {
        for y in 0..=2 {
            for z in -1..=1 {
                let pos = at(x, y, z);
                if !two.logs.contains(&pos) {
                    two.leaf(pos, oak(7));
                }
            }
        }
    }
    scenes.push(two);

    let mut types = Scene::new("log_kinds");
    types.log(at(0, 0, 0));
    for k in 1..=3 {
        types.leaf(at(k, 0, 0), oak(7));
    }
    types
        .setup
        .push((at(4, 0, 0), Block::CRIMSON_STEM.default_state));
    for k in 5..=8 {
        types.leaf(at(k, 0, 0), oak(7));
    }
    types
        .setup
        .push((at(9, 0, 0), Block::OAK_PLANKS.default_state));
    for k in 10..=12 {
        types.leaf(at(k, 0, 0), oak(7));
    }
    types
        .setup
        .push((at(0, 1, 0), Block::OAK_PLANKS.default_state));
    types.leaf(at(0, 2, 0), oak(7));
    types.leaf(at(1, 1, 0), oak(7));
    scenes.push(types);

    let mut duplicates = Scene::new("duplicate_logs");
    duplicates.log(at(0, 0, 0));
    duplicates.logs.push(at(0, 0, 0));
    duplicates.logs.push(at(2, 0, 0));
    duplicates.leaf(at(1, 0, 0), oak(7));
    duplicates.leaf(at(2, 0, 0), oak(7));
    duplicates.leaf(at(3, 0, 0), oak(7));
    scenes.push(duplicates);
    scenes
}

fn shape_scenes(at: &Offset) -> Vec<Scene> {
    let mut scenes = Vec::new();

    for (name, root, decoration) in [
        ("root_block", true, false),
        ("decor_block", false, true),
        ("both_block", true, true),
    ] {
        let mut scene = Scene::new(name);
        scene.log(at(0, 0, 0));
        if root {
            scene.roots.push(at(1, 0, 0));
        }
        if decoration {
            scene.decorations.push(at(2, 0, 0));
        }
        for k in 1..=5 {
            if !scene.roots.contains(&at(k, 0, 0)) && !scene.decorations.contains(&at(k, 0, 0)) {
                scene.leaf(at(k, 0, 0), oak(7));
            }
        }
        scenes.push(scene);
    }

    let mut bypass = Scene::new("root_bypass");
    bypass.log(at(0, 0, 0));
    bypass.roots.push(at(1, 0, 0));
    bypass.decorations.push(at(2, 0, 0));
    for k in 0..=4 {
        bypass.leaf(at(k, 1, 0), oak(7));
    }
    for k in 3..=5 {
        bypass.leaf(at(k, 0, 0), oak(7));
    }
    scenes.push(bypass);

    let mut grown = Scene::new("box_from_roots");
    grown.log(at(0, 0, 0));
    grown.roots.push(at(6, 3, -4));
    grown.decorations.push(at(-5, -2, 4));
    for k in 1..=5 {
        grown.setup.push((at(k, 0, 0), oak(7)));
        grown.setup.push((at(0, k, 0), oak(7)));
        grown.setup.push((at(0, 0, k), oak(7)));
        grown.setup.push((at(-k, 0, 0), oak(7)));
        grown.setup.push((at(0, -k, 0), oak(7)));
        grown.setup.push((at(0, 0, -k), oak(7)));
    }
    scenes.push(grown);

    let mut edges = Scene::new("box_edges");
    edges.log(at(0, 0, 0));
    edges.decorations.push(at(-2, -2, -2));
    edges.decorations.push(at(2, 2, 2));
    for k in 1..=4 {
        for (dx, dy, dz) in [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ] {
            edges.setup.push((at(dx * k, dy * k, dz * k), oak(7)));
        }
    }
    scenes.push(edges);

    let mut no_foliage = Scene::new("logs_only");
    no_foliage.log(at(0, 0, 0));
    no_foliage.log(at(4, 0, 0));
    for k in 1..=3 {
        no_foliage.setup.push((at(k, 0, 0), oak(7)));
    }
    scenes.push(no_foliage);
    scenes
}

/// The vines around a log at `at(0, 0, 0)`, with the neighbours of the log set by `faces`.
fn vine_scene(name: &str, at: &Offset, list: u8, faces: [[bool; 5]; 5]) -> Scene {
    let mut scene = Scene::new(name);
    scene.log(at(0, 0, 0));
    scene.setup.push((at(0, 3, 0), Block::STONE.default_state));
    scene.setup.push((at(1, 0, 0), vine_state(faces[0])));
    scene.setup.push((at(-1, 0, 0), vine_state(faces[1])));
    scene.setup.push((at(0, 0, 1), vine_state(faces[2])));
    scene.setup.push((at(0, 0, -1), vine_state(faces[3])));
    scene.setup.push((at(0, 1, 0), vine_state(faces[4])));
    scene.setup.push((at(0, -1, 0), vine_state(faces[4])));
    scene
        .setup
        .push((at(1, 1, 0), Block::OAK_PLANKS.default_state));
    scene.setup.push((at(1, 2, 0), vine_state(faces[3])));
    match list {
        0 => {}
        1 => scene.roots.push(at(0, 2, 2)),
        _ => scene.decorations.push(at(2, 0, 2)),
    }
    scene
}

fn vine_scenes(at: &Offset) -> Vec<Scene> {
    let none = [false; 5];
    let all = [true; 5];
    let up_only = [true, false, false, false, false];
    let north = [false, true, false, false, false];
    let east = [false, false, true, false, false];
    let south = [false, false, false, true, false];
    let west = [false, false, false, false, true];
    let mut scenes = vec![
        vine_scene("vines_all", at, 0, [all; 5]),
        vine_scene("vines_one_face", at, 0, [north, east, south, west, up_only]),
        vine_scene("vines_other_face", at, 1, [west, south, east, north, west]),
        vine_scene(
            "vines_mixed",
            at,
            2,
            [
                [true, true, false, true, false],
                none,
                all,
                north,
                [false, true, true, false, true],
            ],
        ),
    ];

    let mut leaves_and_vines = vine_scene(
        "vines_with_leaves",
        at,
        0,
        [south, west, north, east, up_only],
    );
    for k in 1..=3 {
        leaves_and_vines.leaf(at(0, 2 + k, 0), oak(7));
        leaves_and_vines.leaf(at(-k - 1, 0, 0), oak(7));
    }
    scenes.push(leaves_and_vines);

    let mut early = vine_scene("vines_early_return", at, 0, [all; 5]);
    early.logs.clear();
    early.roots.push(at(0, 0, 0));
    early.decorations.push(at(1, 1, 1));
    scenes.push(early);

    let mut foliage_only = vine_scene("vines_foliage_only", at, 0, [all; 5]);
    foliage_only.logs.clear();
    foliage_only.roots.push(at(0, 0, 0));
    foliage_only.decorations.push(at(1, 1, 1));
    foliage_only.foliage.push(at(3, 3, 3));
    scenes.push(foliage_only);

    let mut nothing = Scene::new("early_return_leaves");
    nothing.roots.push(at(0, 0, 0));
    nothing.decorations.push(at(2, 0, 0));
    for k in 1..=3 {
        nothing.setup.push((at(k, 1, 0), oak(7)));
    }
    scenes.push(nothing);

    let mut leaves_only = Scene::new("foliage_without_logs");
    for k in 0..=4 {
        leaves_only.leaf(at(k, 0, 0), oak(3));
    }
    scenes.push(leaves_only);
    scenes
}

fn random_scene(index: u64, origin: BlockPos) -> Scene {
    let mut random = RandomGenerator::Xoroshiro(Xoroshiro::from_seed(index * 7919 + 13));
    let mut scene = Scene::new(&format!("random_{index}"));
    let leaf_blocks = [
        &Block::OAK_LEAVES,
        &Block::SPRUCE_LEAVES,
        &Block::BIRCH_LEAVES,
    ];
    for dx in -2..=2 {
        for dy in -2..=2 {
            for dz in -2..=2 {
                let pos = BlockPos::new(origin.0.x + dx, origin.0.y + dy, origin.0.z + dz);
                let kind = random.next_bounded_i32(100);
                match kind {
                    0..=44 => {
                        let block = leaf_blocks[random.next_bounded_i32(3) as usize];
                        let distance = if random.next_bool() {
                            7
                        } else {
                            1 + random.next_bounded_i32(7) as u8
                        };
                        let state =
                            leaf_state(block, distance, random.next_bool(), random.next_bool());
                        scene.setup.push((pos, state));
                        if random.next_bounded_i32(5) != 0 {
                            scene.foliage.push(pos);
                        }
                    }
                    45..=52 => scene.log(pos),
                    53..=56 => scene.setup.push((pos, Block::OAK_PLANKS.default_state)),
                    57..=60 => {
                        let faces = [
                            random.next_bool(),
                            random.next_bool(),
                            random.next_bool(),
                            random.next_bool(),
                            random.next_bool(),
                        ];
                        scene.setup.push((pos, vine_state(faces)));
                    }
                    61..=63 => scene.setup.push((pos, Block::CRIMSON_STEM.default_state)),
                    64..=66 => scene.setup.push((pos, Block::STONE.default_state)),
                    _ => {}
                }
            }
        }
    }
    for _ in 0..random.next_bounded_i32(3) {
        scene.roots.push(random_pos(&mut random, origin));
    }
    for _ in 0..random.next_bounded_i32(3) {
        scene.decorations.push(random_pos(&mut random, origin));
    }
    if index % 10 == 9 {
        scene.logs.clear();
        scene
            .setup
            .retain(|(_, state)| state.id != Block::OAK_LOG.default_state.id);
    }
    if index % 10 == 4 {
        scene.foliage.clear();
    }
    scene
}

fn random_pos(random: &mut RandomGenerator, origin: BlockPos) -> BlockPos {
    BlockPos::new(
        origin.0.x + random.next_bounded_i32(5) - 2,
        origin.0.y + random.next_bounded_i32(5) - 2,
        origin.0.z + random.next_bounded_i32(5) - 2,
    )
}

fn report() -> String {
    let world_gen = get_world_gen(
        Seed(1),
        pumpkin_data::dimension::Dimension::OVERWORLD,
        true,
        Vec::new(),
        "minecraft:plains".to_owned(),
    );
    let mut report = String::new();
    for (tag, origin) in [("a", (13, 68, 5)), ("b", (-6, 70, -9))] {
        let at = move |dx: i32, dy: i32, dz: i32| {
            BlockPos::new(origin.0 + dx, origin.1 + dy, origin.2 + dz)
        };
        let scenes = line_scenes(&at)
            .into_iter()
            .chain(blob_scenes(&at))
            .chain(shape_scenes(&at))
            .chain(vine_scenes(&at));
        for mut scene in scenes {
            scene.name = format!("{}@{tag}", scene.name);
            report.push_str(&run_scene(&world_gen, &scene));
        }
    }
    for index in 0..100u64 {
        let origin = [(14, 68, 6), (-2, 70, -3), (30, 66, 15), (1, 69, -14)][(index % 4) as usize];
        let scene = random_scene(index, BlockPos::new(origin.0, origin.1, origin.2));
        report.push_str(&run_scene(&world_gen, &scene));
    }
    report
}

#[test]
fn tree_update_leaves() -> Result<(), Box<dyn std::error::Error>> {
    let report = report();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("tree_update_leaves.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
