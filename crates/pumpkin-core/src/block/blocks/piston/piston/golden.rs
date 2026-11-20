//! A record of the piston move (`tests/golden/piston_move.txt`; rewrite it on purpose with
//! `KVASIR_BLESS=1`), made before `handle_synced_block_event` and `move_piston` were split.
//!
//! Every scene builds a piston and its surroundings in "piston frame" coordinates `(a, b, c)`:
//! `a` runs along the facing, `b` and `c` across it, so each of the six facings reads the same
//! layout. A script then sets blocks (a power source, with block updates) or calls the synced
//! block event directly at given ticks, and the world is ticked with its block entities. The
//! record holds the blocks around the piston at the start and, after each tick, only what
//! changed there.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::sync::Arc;

use pumpkin_data::block_properties::{ObserverLikeProperties, PistonType};
use pumpkin_data::{Block, BlockDirection, BlockStateId};
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_world::world::BlockFlags;

use super::{PistonBlock, PistonProps};
use crate::block::entities::piston::PistonBlockEntity;
use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::ai::test_support;
use crate::server::Server;
use crate::world::World;

/// A position in piston-frame coordinates.
type Rel = (i32, i32, i32);

const ORIGIN: BlockPos = BlockPos(pumpkin_util::math::vector3::Vector3 { x: 8, y: 70, z: 8 });

/// What a scene puts in a cell.
#[derive(Clone, Copy)]
enum Cell {
    Air,
    Block(&'static Block),
    /// A piston at rest or extended (its head is a separate cell), facing along `toward`.
    Piston {
        sticky: bool,
        extended: bool,
        toward: Rel,
    },
    Head {
        sticky: bool,
        toward: Rel,
    },
    Observer {
        toward: Rel,
    },
}

const STONE: Cell = Cell::Block(&Block::STONE);
const SLIME: Cell = Cell::Block(&Block::SLIME_BLOCK);
const HONEY: Cell = Cell::Block(&Block::HONEY_BLOCK);
const OBSIDIAN: Cell = Cell::Block(&Block::OBSIDIAN);
const POWER: Cell = Cell::Block(&Block::REDSTONE_BLOCK);
const AIR: Cell = Cell::Air;
const FWD: Rel = (1, 0, 0);
const BACK: Rel = (-1, 0, 0);
const UP: Rel = (0, 1, 0);
const BEHIND: Rel = (-1, 0, 0);

enum Action {
    Set(Rel, Cell),
    /// `handle_synced_block_event` called directly with this event type.
    Call(u8),
}

struct Scene {
    name: String,
    sticky: bool,
    facing: BlockDirection,
    cells: Vec<(Rel, Cell)>,
    script: Vec<(u32, Action)>,
    ticks: u32,
}

impl Scene {
    fn new(name: impl Into<String>, sticky: bool, facing: BlockDirection, ticks: u32) -> Self {
        Self {
            name: name.into(),
            sticky,
            facing,
            cells: Vec::new(),
            script: Vec::new(),
            ticks,
        }
    }

    fn cells(mut self, cells: impl IntoIterator<Item = (Rel, Cell)>) -> Self {
        self.cells.extend(cells);
        self
    }

    fn at(mut self, tick: u32, action: Action) -> Self {
        self.script.push((tick, action));
        self
    }

    /// Power on at `on` and off at `off` (a redstone block behind the piston).
    fn pulse(self, on: u32, off: u32) -> Self {
        self.at(on, Action::Set(BEHIND, POWER))
            .at(off, Action::Set(BEHIND, AIR))
    }
}

/// The frame of a facing: the unit vectors of `a`, `b` and `c` in world axes.
struct Frame {
    a: [i32; 3],
    b: [i32; 3],
    c: [i32; 3],
}

impl Frame {
    fn of(facing: BlockDirection) -> Self {
        match facing {
            BlockDirection::East => Self::new([1, 0, 0], [0, 1, 0], [0, 0, 1]),
            BlockDirection::West => Self::new([-1, 0, 0], [0, 1, 0], [0, 0, 1]),
            BlockDirection::South => Self::new([0, 0, 1], [0, 1, 0], [1, 0, 0]),
            BlockDirection::North => Self::new([0, 0, -1], [0, 1, 0], [1, 0, 0]),
            BlockDirection::Up => Self::new([0, 1, 0], [1, 0, 0], [0, 0, 1]),
            BlockDirection::Down => Self::new([0, -1, 0], [1, 0, 0], [0, 0, 1]),
        }
    }

    const fn new(a: [i32; 3], b: [i32; 3], c: [i32; 3]) -> Self {
        Self { a, b, c }
    }

    fn offset(&self, (a, b, c): Rel) -> [i32; 3] {
        let mut out = [0; 3];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = a * self.a[i] + b * self.b[i] + c * self.c[i];
        }
        out
    }

    fn pos(&self, rel: Rel) -> BlockPos {
        let [x, y, z] = self.offset(rel);
        BlockPos::new(ORIGIN.0.x + x, ORIGIN.0.y + y, ORIGIN.0.z + z)
    }

    fn direction(&self, toward: Rel) -> BlockDirection {
        match self.offset(toward) {
            [1, 0, 0] => BlockDirection::East,
            [-1, 0, 0] => BlockDirection::West,
            [0, 1, 0] => BlockDirection::Up,
            [0, -1, 0] => BlockDirection::Down,
            [0, 0, 1] => BlockDirection::South,
            _ => BlockDirection::North,
        }
    }

    fn state(&self, cell: Cell) -> BlockStateId {
        match cell {
            Cell::Air => Block::AIR.default_state.id,
            Cell::Block(block) => block.default_state.id,
            Cell::Piston {
                sticky,
                extended,
                toward,
            } => {
                let block = if sticky {
                    &Block::STICKY_PISTON
                } else {
                    &Block::PISTON
                };
                let mut props = PistonProps::default(block);
                props.facing = self.direction(toward).to_facing();
                props.extended = extended;
                props.to_state_id(block)
            }
            Cell::Head { sticky, toward } => {
                let mut props = pumpkin_data::block_properties::PistonHeadLikeProperties::default(
                    &Block::PISTON_HEAD,
                );
                props.facing = self.direction(toward).to_facing();
                props.r#type = if sticky {
                    PistonType::Sticky
                } else {
                    PistonType::Normal
                };
                props.to_state_id(&Block::PISTON_HEAD)
            }
            Cell::Observer { toward } => {
                let mut props = ObserverLikeProperties::default(&Block::OBSERVER);
                props.facing = self.direction(toward).to_facing();
                props.to_state_id(&Block::OBSERVER)
            }
        }
    }
}

/// Region recorded around the piston, in frame coordinates.
const A_RANGE: std::ops::RangeInclusive<i32> = -2..=16;
const CROSS_RANGE: std::ops::RangeInclusive<i32> = -3..=3;

/// The block and its properties, and the moving piston's block entity.
fn describe(world: &Arc<World>, pos: &BlockPos) -> Option<String> {
    let (block, state) = world.get_block_and_state_id(pos);
    if block == &Block::AIR {
        return None;
    }
    let mut out = block.name.to_string();
    let with_props = [
        &Block::PISTON,
        &Block::STICKY_PISTON,
        &Block::PISTON_HEAD,
        &Block::MOVING_PISTON,
        &Block::OBSERVER,
        &Block::REDSTONE_LAMP,
    ];
    if with_props.contains(&block)
        && let Some(props) = block.properties(state)
    {
        let _ = write!(out, "{:?}", props.to_props());
    }
    if block == &Block::MOVING_PISTON
        && let Some(entity) = world.get_block_entity(pos)
        && let Some(piston) = entity.as_any().downcast_ref::<PistonBlockEntity>()
    {
        let _ = write!(
            out,
            "<{:?} ext={} src={} {:.2}/{:.2} {}>",
            piston.facing,
            piston.extending,
            piston.source,
            piston.last_progress.load(),
            piston.current_progress.load(),
            Block::from_state_id(piston.pushed_block_state.id).name,
        );
    }
    Some(out)
}

fn snapshot(world: &Arc<World>, frame: &Frame) -> BTreeMap<Rel, String> {
    let mut seen = BTreeMap::new();
    for a in A_RANGE {
        for b in CROSS_RANGE {
            for c in CROSS_RANGE {
                if let Some(text) = describe(world, &frame.pos((a, b, c))) {
                    seen.insert((a, b, c), text);
                }
            }
        }
    }
    seen
}

fn put(world: &Arc<World>, pos: &BlockPos, state: BlockStateId) {
    world.set_block_state(
        pos,
        state,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

/// Empties the area (and its block entities) the previous scene left behind: replacing the
/// chunk data does not.
fn clear_area(world: &Arc<World>) {
    for dx in -17..=17 {
        for dy in -17..=17 {
            for dz in -17..=17 {
                let pos = BlockPos::new(ORIGIN.0.x + dx, ORIGIN.0.y + dy, ORIGIN.0.z + dz);
                if world.get_block(&pos) != &Block::AIR {
                    world.remove_block_entity(&pos);
                    put(world, &pos, Block::AIR.default_state.id);
                }
            }
        }
    }
}

fn load_area(world: &Arc<World>) {
    for cx in -1..=1 {
        for cz in -1..=1 {
            world.level.loaded_chunks.insert(
                Vector2::new(cx, cz),
                pumpkin_world::chunk::ChunkData::empty_sync(cx, cz),
            );
        }
    }
}

fn run_tick(world: &Arc<World>, server: &Arc<Server>) {
    world.tick(server);
    let chunks: rustc_hash::FxHashSet<Vector2<i32>> = (-1..=1)
        .flat_map(|cx| (-1..=1).map(move |cz| Vector2::new(cx, cz)))
        .collect();
    let entities = world.collect_block_entities(&Arc::new(chunks));
    world.tick_block_entities(&entities);
}

fn show(now: &BTreeMap<Rel, String>) -> String {
    now.iter()
        .map(|((a, b, c), text)| format!("{a},{b},{c}:{text}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// What changed from `before` to `now`.
fn changes(before: &BTreeMap<Rel, String>, now: &BTreeMap<Rel, String>) -> String {
    let mut parts = Vec::new();
    for (rel, text) in now {
        if before.get(rel) != Some(text) {
            parts.push(format!("{},{},{}:{text}", rel.0, rel.1, rel.2));
        }
    }
    for rel in before.keys().filter(|rel| !now.contains_key(rel)) {
        parts.push(format!("{},{},{}:-", rel.0, rel.1, rel.2));
    }
    parts.join(" ")
}

/// The packets players online were sent while a scene ran, one line per tick that sent any.
struct Listeners<'a> {
    online: &'a mut [script_golden::Online],
    record: String,
    /// Each piston sound's id and pitch: the pitch is random, so it is checked, not recorded.
    pitches: std::cell::RefCell<Vec<(Vec<u8>, f32)>>,
}

/// A sound packet with its random pitch (kept in `pitches`) and seed blanked.
fn normalise(pitches: &std::cell::RefCell<Vec<(Vec<u8>, f32)>>, packet: &[u8]) -> Vec<u8> {
    let mut bytes = script_golden::blank_seed(packet);
    if packet.len() == SOUND_LEN && packet[0] == SOUND_PACKET_ID {
        let at = packet.len() - 12;
        let pitch =
            f32::from_be_bytes([packet[at], packet[at + 1], packet[at + 2], packet[at + 3]]);
        pitches.borrow_mut().push((packet[1..3].to_vec(), pitch));
        bytes[at..at + 4].fill(0);
    }
    bytes
}

/// A sound effect packet for a piston: its length and packet id.
const SOUND_LEN: usize = 32;
const SOUND_PACKET_ID: u8 = 119;

fn run_scene(
    world: &Arc<World>,
    server: &Arc<Server>,
    scene: &Scene,
    mut listeners: Option<&mut Listeners>,
) -> String {
    load_area(world);
    clear_area(world);
    let frame = Frame::of(scene.facing);
    let block = if scene.sticky {
        &Block::STICKY_PISTON
    } else {
        &Block::PISTON
    };
    put(
        world,
        &frame.pos((0, 0, 0)),
        frame.state(Cell::Piston {
            sticky: scene.sticky,
            extended: false,
            toward: FWD,
        }),
    );
    for (rel, cell) in &scene.cells {
        put(world, &frame.pos(*rel), frame.state(*cell));
    }
    if let Some(listeners) = listeners.as_deref_mut() {
        script_golden::packets(listeners.online);
    }
    let mut out = String::new();
    let mut last = snapshot(world, &frame);
    let _ = writeln!(out, "{}: start {}", scene.name, show(&last));
    for tick in 0..=scene.ticks {
        for (at, action) in &scene.script {
            if *at != tick {
                continue;
            }
            match action {
                Action::Set(rel, cell) => {
                    world.set_block_state(
                        &frame.pos(*rel),
                        frame.state(*cell),
                        BlockFlags::NOTIFY_ALL,
                    );
                }
                Action::Call(kind) => {
                    let done = PistonBlock::handle_synced_block_event(
                        block,
                        world,
                        &frame.pos((0, 0, 0)),
                        *kind,
                        frame.direction(FWD).to_index(),
                    );
                    let _ = writeln!(out, "{}: t{tick} call {kind} -> {done}", scene.name);
                }
            }
        }
        if tick > 0 {
            run_tick(world, server);
        }
        if let Some(listeners) = listeners.as_deref_mut() {
            let pitches = &listeners.pitches;
            let sent = script_golden::packets_normalised(listeners.online, &|packet| {
                normalise(pitches, packet)
            });
            let _ = writeln!(listeners.record, "{}: t{tick}{sent}", scene.name);
        }
        let now = snapshot(world, &frame);
        let diff = changes(&last, &now);
        if !diff.is_empty() {
            let _ = writeln!(out, "{}: t{tick} {diff}", scene.name);
        }
        last = now;
    }
    out
}

/// A row of `count` blocks from `a = 1`.
fn row(count: i32, cell: Cell) -> Vec<(Rel, Cell)> {
    (1..=count).map(|a| ((a, 0, 0), cell)).collect()
}

const FACINGS: [BlockDirection; 6] = [
    BlockDirection::East,
    BlockDirection::West,
    BlockDirection::South,
    BlockDirection::North,
    BlockDirection::Up,
    BlockDirection::Down,
];

fn scenes() -> Vec<Scene> {
    let mut all = Vec::new();
    all.extend(basic());
    all.extend(power_sources());
    all.extend(loads());
    all.extend(block_kinds());
    all.extend(sticky_groups());
    all.extend(pistons_in_line());
    all.extend(timing());
    all.extend(direct_calls());
    all.extend(neighbours());
    all.extend(noise());
    all
}

fn kind(sticky: bool) -> &'static str {
    if sticky { "sticky" } else { "normal" }
}

/// A stone ahead, pushed by a pulse and pulled back by a sticky piston, in every facing.
fn basic() -> Vec<Scene> {
    let mut out = Vec::new();
    for facing in FACINGS {
        for sticky in [false, true] {
            let name = format!("basic {facing:?} {}", kind(sticky));
            out.push(
                Scene::new(name, sticky, facing, 14)
                    .cells(row(1, STONE))
                    .pulse(0, 7),
            );
        }
    }
    out
}

/// Where a redstone block powers the piston: each side (the block update reaches the piston),
/// and the places above it a piston also reads, where the power is there before the piston
/// hears of any change (the front side never counts).
fn power_sources() -> Vec<Scene> {
    let sides: [Rel; 6] = [
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
        (1, 0, 0),
    ];
    // Above the piston in the world: for a piston facing east the frame's `b` axis is up, for
    // one facing up the `a` axis is.
    let above_east: [Rel; 5] = [(0, 2, 0), (1, 1, 0), (-1, 1, 0), (0, 1, 1), (0, 1, -1)];
    let above_up: [Rel; 5] = [(2, 0, 0), (1, 1, 0), (1, -1, 0), (1, 0, 1), (1, 0, -1)];
    let glass = Cell::Block(&Block::GLASS);
    let mut out = Vec::new();
    for facing in [BlockDirection::East, BlockDirection::Up] {
        for spot in sides {
            let name = format!("power {facing:?} at {},{},{}", spot.0, spot.1, spot.2);
            out.push(
                Scene::new(name, false, facing, 14)
                    .cells(row(2, STONE).into_iter().skip(1))
                    .at(0, Action::Set(spot, POWER))
                    .at(7, Action::Set(spot, AIR)),
            );
        }
        let above = if facing == BlockDirection::East {
            above_east
        } else {
            above_up
        };
        for spot in above {
            let name = format!("quasi {facing:?} at {},{},{}", spot.0, spot.1, spot.2);
            out.push(
                Scene::new(name, false, facing, 16)
                    .cells(row(2, STONE).into_iter().skip(1))
                    .cells([(spot, POWER)])
                    .at(0, Action::Set(BEHIND, glass))
                    .at(7, Action::Set(spot, AIR))
                    .at(8, Action::Set(BEHIND, AIR)),
            );
        }
    }
    out
}

/// A row of 0..14 blocks: the push limit is 12.
fn loads() -> Vec<Scene> {
    let mut out = Vec::new();
    for (sticky, counts) in [(false, vec![0, 1, 11, 12, 13, 14]), (true, vec![1, 12, 13])] {
        for count in counts {
            let name = format!("load {count} {}", kind(sticky));
            out.push(
                Scene::new(name, sticky, BlockDirection::East, 14)
                    .cells(row(count, STONE))
                    .pulse(0, 7),
            );
        }
    }
    out
}

fn floor() -> Vec<(Rel, Cell)> {
    let mut cells = Vec::new();
    for a in 0..=4 {
        for c in -1..=1 {
            cells.push(((a, -1, c), Cell::Block(&Block::GRASS_BLOCK)));
        }
    }
    cells
}

/// One block of each kind in front of the piston: movable, immovable, destroyed.
fn block_kinds() -> Vec<Scene> {
    let kinds: [(&str, &'static Block); 24] = [
        ("obsidian", &Block::OBSIDIAN),
        ("bedrock", &Block::BEDROCK),
        ("chest", &Block::CHEST),
        ("furnace", &Block::FURNACE),
        ("poppy", &Block::POPPY),
        ("short_grass", &Block::SHORT_GRASS),
        ("torch", &Block::TORCH),
        ("glowstone", &Block::GLOWSTONE),
        ("tnt", &Block::TNT),
        ("pressure_plate", &Block::STONE_PRESSURE_PLATE),
        ("lever", &Block::LEVER),
        ("anvil", &Block::ANVIL),
        ("leaves", &Block::OAK_LEAVES),
        ("sand", &Block::SAND),
        ("enchanting_table", &Block::ENCHANTING_TABLE),
        ("jukebox", &Block::JUKEBOX),
        ("ender_chest", &Block::ENDER_CHEST),
        ("glass", &Block::GLASS),
        ("crying_obsidian", &Block::CRYING_OBSIDIAN),
        ("cobweb", &Block::COBWEB),
        ("glazed_terracotta", &Block::WHITE_GLAZED_TERRACOTTA),
        ("barrier", &Block::BARRIER),
        ("end_portal_frame", &Block::END_PORTAL_FRAME),
        ("redstone_wire", &Block::REDSTONE_WIRE),
    ];
    let mut out = Vec::new();
    for (name, block) in kinds {
        for sticky in [false, true] {
            let scene = Scene::new(
                format!("kind {name} {}", kind(sticky)),
                sticky,
                BlockDirection::East,
                14,
            )
            .cells(floor())
            .cells([((1, 0, 0), Cell::Block(block))])
            .pulse(0, 7);
            out.push(scene);
        }
    }
    out
}

/// Slime and honey: what sticks to what.
fn sticky_groups() -> Vec<Scene> {
    let groups: Vec<(&str, Vec<(Rel, Cell)>)> = vec![
        ("slime_row", row(3, SLIME)),
        (
            "slime_stone_top",
            vec![((1, 0, 0), SLIME), ((1, 1, 0), STONE)],
        ),
        (
            "slime_l",
            row(3, SLIME)
                .into_iter()
                .chain([((3, 1, 0), SLIME), ((3, 2, 0), SLIME)])
                .collect(),
        ),
        (
            "slime_plate_one",
            (-1..=1)
                .flat_map(|b| (-1..=1).map(move |c| ((1, b, c), SLIME)))
                .collect(),
        ),
        (
            "slime_plate",
            (1..=2)
                .flat_map(|a| (-1..=1).flat_map(move |b| (-1..=1).map(move |c| ((a, b, c), SLIME))))
                .collect(),
        ),
        (
            "slime_obsidian_side",
            vec![((1, 0, 0), SLIME), ((1, 1, 0), OBSIDIAN)],
        ),
        (
            "slime_obsidian_far",
            vec![
                ((1, 0, 0), SLIME),
                ((2, 0, 0), SLIME),
                ((2, 1, 0), OBSIDIAN),
            ],
        ),
        (
            "honey_stone_side",
            vec![((1, 0, 0), HONEY), ((1, 1, 0), STONE)],
        ),
        (
            "honey_slime_side",
            vec![((1, 0, 0), HONEY), ((1, 1, 0), SLIME)],
        ),
        (
            "slime_honey_line",
            vec![((1, 0, 0), SLIME), ((2, 0, 0), HONEY)],
        ),
        (
            "slime_over_limit",
            row(11, STONE)
                .into_iter()
                .chain([
                    ((12, 0, 0), SLIME),
                    ((12, 1, 0), STONE),
                    ((12, 2, 0), STONE),
                ])
                .collect(),
        ),
        (
            "slime_at_limit",
            row(10, STONE)
                .into_iter()
                .chain([((11, 0, 0), SLIME), ((11, 1, 0), STONE)])
                .collect(),
        ),
        (
            "slime_poppy_side",
            vec![((1, 0, 0), SLIME), ((1, 1, 0), Cell::Block(&Block::POPPY))],
        ),
        (
            "slime_above_piston",
            vec![((1, 0, 0), STONE), ((0, 1, 0), SLIME), ((1, 1, 0), SLIME)],
        ),
        (
            "slime_back_side",
            vec![((1, 0, 0), SLIME), ((0, 0, 1), SLIME), ((1, 0, 1), STONE)],
        ),
    ];
    let mut out = Vec::new();
    for (name, cells) in groups {
        for sticky in [false, true] {
            out.push(
                Scene::new(
                    format!("group {name} {}", kind(sticky)),
                    sticky,
                    BlockDirection::East,
                    14,
                )
                .cells(cells.clone())
                .pulse(0, 7),
            );
        }
    }
    out
}

fn piston_cell(sticky: bool, extended: bool, toward: Rel) -> Cell {
    Cell::Piston {
        sticky,
        extended,
        toward,
    }
}

/// Pistons among the pushed blocks: only a retracted one moves.
fn pistons_in_line() -> Vec<Scene> {
    let others: Vec<(&str, Vec<(Rel, Cell)>)> = vec![
        (
            "rest_fwd",
            vec![
                ((1, 0, 0), STONE),
                ((2, 0, 0), piston_cell(false, false, FWD)),
            ],
        ),
        (
            "rest_back",
            vec![
                ((1, 0, 0), STONE),
                ((2, 0, 0), piston_cell(false, false, BACK)),
            ],
        ),
        ("rest_up", vec![((1, 0, 0), piston_cell(true, false, UP))]),
        (
            "extended_fwd",
            vec![
                ((1, 0, 0), piston_cell(false, true, FWD)),
                (
                    (2, 0, 0),
                    Cell::Head {
                        sticky: false,
                        toward: FWD,
                    },
                ),
            ],
        ),
        (
            "sticky_back",
            vec![((1, 0, 0), piston_cell(true, false, BACK))],
        ),
    ];
    let mut out = Vec::new();
    for (name, cells) in others {
        for sticky in [false, true] {
            out.push(
                Scene::new(
                    format!("line {name} {}", kind(sticky)),
                    sticky,
                    BlockDirection::East,
                    14,
                )
                .cells(cells.clone())
                .pulse(0, 7),
            );
        }
    }
    out
}

/// Retracting before the head arrives, and flicker.
fn timing() -> Vec<Scene> {
    let mut out = Vec::new();
    for sticky in [false, true] {
        for off in 1..=5 {
            out.push(
                Scene::new(
                    format!("quick off {off} {}", kind(sticky)),
                    sticky,
                    BlockDirection::East,
                    14,
                )
                .cells(row(1, STONE))
                .pulse(0, off),
            );
        }
    }
    out.push(
        Scene::new("flicker 0 2 3 5 sticky", true, BlockDirection::East, 16)
            .cells(row(1, STONE))
            .pulse(0, 2)
            .pulse(3, 5),
    );
    out.push(
        Scene::new(
            "flicker 0 1 2 3 sticky slime",
            true,
            BlockDirection::East,
            16,
        )
        .cells(row(2, SLIME))
        .pulse(0, 1)
        .pulse(2, 3),
    );
    out.push(
        Scene::new("quick off 1 sticky slime", true, BlockDirection::East, 14)
            .cells(row(2, SLIME))
            .pulse(0, 1),
    );
    out
}

/// The synced block event called by hand: each type, powered or not, extended or not.
fn direct_calls() -> Vec<Scene> {
    let mut out = Vec::new();
    for sticky in [false, true] {
        for extended in [false, true] {
            for powered in [false, true] {
                for event in 0..=2 {
                    let name = format!(
                        "call {event} {} {} {}",
                        kind(sticky),
                        if extended { "extended" } else { "rest" },
                        if powered { "powered" } else { "unpowered" },
                    );
                    let mut scene = Scene::new(name, sticky, BlockDirection::East, 8);
                    if extended {
                        scene = scene.cells([
                            ((0, 0, 0), piston_cell(sticky, true, FWD)),
                            (
                                (1, 0, 0),
                                Cell::Head {
                                    sticky,
                                    toward: FWD,
                                },
                            ),
                            ((2, 0, 0), STONE),
                        ]);
                    } else {
                        scene = scene.cells(row(1, STONE));
                    }
                    if powered {
                        scene = scene.cells([(BEHIND, POWER)]);
                    }
                    out.push(scene.at(0, Action::Call(event)));
                }
            }
        }
    }
    out
}

/// Block updates around the move: observers watching the cells that change, and a lamp.
fn neighbours() -> Vec<Scene> {
    let mut out = Vec::new();
    let watching_down = Cell::Observer { toward: (0, -1, 0) };
    let lamp = Cell::Block(&Block::REDSTONE_LAMP);
    for sticky in [false, true] {
        out.push(
            Scene::new(
                format!("observed {}", kind(sticky)),
                sticky,
                BlockDirection::East,
                14,
            )
            .cells(row(2, STONE))
            .cells([
                ((1, 1, 0), watching_down),
                ((1, 2, 0), lamp),
                ((2, 1, 0), watching_down),
                ((2, 2, 0), lamp),
                ((3, 1, 0), watching_down),
                ((3, 2, 0), lamp),
            ])
            .pulse(0, 7),
        );
        out.push(
            Scene::new(
                format!("observed broken {}", kind(sticky)),
                sticky,
                BlockDirection::East,
                14,
            )
            .cells(floor())
            .cells([
                ((1, 0, 0), STONE),
                ((2, 0, 0), Cell::Block(&Block::POPPY)),
                ((2, 1, 0), watching_down),
                ((2, 2, 0), lamp),
            ])
            .pulse(0, 7),
        );
    }
    out
}

/// Block updates that are not a power change, next to a piston at rest, extending, extended and
/// retracting: `try_move` must not move it for any of them.
fn noise() -> Vec<Scene> {
    let side: Rel = (0, 0, 1);
    let mut out = Vec::new();
    for facing in [BlockDirection::East, BlockDirection::Up] {
        for sticky in [false, true] {
            let name = |what: &str| format!("noise {what} {facing:?} {}", kind(sticky));
            let base = |what: &str, ticks| {
                Scene::new(name(what), sticky, facing, ticks).cells(row(1, STONE))
            };
            out.push(
                base("rest", 8)
                    .at(0, Action::Set(side, STONE))
                    .at(3, Action::Set(side, AIR)),
            );
            // Powered from tick 0; the update arrives while the head moves, and once extended.
            for at in [1, 2, 3, 5] {
                out.push(
                    base(&format!("powered at {at}"), 14)
                        .at(0, Action::Set(BEHIND, POWER))
                        .at(at, Action::Set(side, STONE))
                        .at(at + 3, Action::Set(side, AIR)),
                );
            }
            // Power taken away at tick 6 or 8; the update arrives while the head returns.
            for (off, at) in [(6, 7), (6, 8), (6, 9), (8, 9), (8, 10), (8, 11)] {
                out.push(
                    base(&format!("released {off} at {at}"), 16)
                        .pulse(0, off)
                        .at(at, Action::Set(side, STONE))
                        .at(at + 2, Action::Set(side, AIR)),
                );
            }
            // Power taken away before the head arrives: the update comes after.
            for at in [2, 3, 4] {
                out.push(
                    base(&format!("early release at {at}"), 14)
                        .pulse(0, 1)
                        .at(at, Action::Set(side, STONE)),
                );
            }
        }
    }
    out
}

fn expected_for(file: &str) -> Option<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(file);
    std::fs::read_to_string(path).ok()
}

#[tokio::test(flavor = "multi_thread")]
async fn piston_moves_are_the_same() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let blessing = std::env::var_os("KVASIR_BLESS").is_some();
    let expected = expected_for("piston_move.txt");

    let mut now = String::new();
    for scene in scenes() {
        now.push_str(&run_scene(&world, &server, &scene, None));
        // Stop at the first scene that differs, not after the whole run.
        if let (false, Some(expected)) = (blessing, expected.as_ref()) {
            assert!(
                expected.starts_with(&now),
                "scene {} differs from tests/golden/piston_move.txt",
                scene.name
            );
        }
    }
    script_golden::bless_or_compare("piston_move.txt", &now)?;
    server.stop();
    Ok(())
}

/// The scenes whose packets are recorded: the sounds, game events and updates a player hears.
fn heard_scenes() -> Vec<Scene> {
    scenes()
        .into_iter()
        .filter(|scene| {
            scene.name.starts_with("basic East")
                || scene.name.starts_with("basic Up")
                || scene.name.starts_with("group slime_l")
                || scene.name.starts_with("group slime_honey_line")
                || scene.name.starts_with("kind poppy")
                || scene.name.starts_with("kind glass")
                || scene.name.starts_with("quick off")
                || scene.name.starts_with("flicker")
                || scene.name.starts_with("call")
                || scene.name.starts_with("observed")
                || scene.name.starts_with("line rest_fwd")
                || scene.name.starts_with("load 12")
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn piston_moves_send_the_same_packets() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    let mut online = script_golden::join(&world).await?;
    online.truncate(3);
    for who in &online {
        who.player
            .living_entity
            .entity
            .set_pos(pumpkin_util::math::vector3::Vector3::new(
                f64::from(ORIGIN.0.x) + 0.5,
                f64::from(ORIGIN.0.y) + 6.0,
                f64::from(ORIGIN.0.z) + 0.5,
            ));
    }
    let mut listeners = Listeners {
        online: &mut online,
        record: String::new(),
        pitches: std::cell::RefCell::new(Vec::new()),
    };
    for scene in heard_scenes() {
        run_scene(&world, &server, &scene, Some(&mut listeners));
    }
    let record = std::mem::take(&mut listeners.record);
    check_pitches(&listeners.pitches.borrow());
    script_golden::bless_or_compare("piston_move_packets.txt", &record)?;
    for who in &online {
        world.remove_player(&who.player, false).await;
    }
    server.stop();
    Ok(())
}

/// A piston's sound pitch is random: between 0.6 and 0.85 when it extends, below 0.75 when it
/// retracts. Over the many sounds of the run each kind must stay in its range and reach below
/// 0.65 (so the range cannot start lower, or be a constant).
fn check_pitches(heard: &[(Vec<u8>, f32)]) {
    let mut kinds: BTreeMap<&[u8], Vec<f32>> = BTreeMap::new();
    for (id, pitch) in heard {
        kinds.entry(id).or_default().push(*pitch);
    }
    assert_eq!(kinds.len(), 2, "extend and contract sounds");
    for (id, pitches) in kinds {
        let (low, high) = pitches.iter().fold((f32::MAX, f32::MIN), |(low, high), p| {
            (low.min(*p), high.max(*p))
        });
        assert!(pitches.len() >= 40, "{id:?}: {} sounds", pitches.len());
        assert!(low >= 0.6 && high < 0.85, "{id:?}: pitch {low}..{high}");
        assert!(low < 0.65, "{id:?}: lowest pitch {low}");
    }
}
