//! A record of what `Player::calculate_respawn_point` returns and changes, made before the
//! function was split (`tests/golden/calculate_respawn_point.txt`; rewrite it on purpose with
//! `KVASIR_BLESS=1`).
//!
//! Each scene starts its own real server (overworld, nether and end), lays stone floors at
//! y = 64 and the scene's blocks (beds, respawn anchors, walls, water), gives a player in the
//! scene's home world the stored respawn point (dimension, block, yaw 120, forced or not) and
//! calls the function once. One line per scene: the returned point (position, yaw, pitch,
//! dimension) or `None`, then the block and state at the stored position afterwards (an anchor
//! loses a charge), and for the chunk scenes the chunks the call loaded.
//! Not covered: the plugin that moves the spawn (none is loaded).

use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pumpkin_data::block_properties::{
    BedPart, HorizontalFacing, RespawnAnchorLikeProperties, WhiteBedLikeProperties,
};
use pumpkin_data::dimension::Dimension;
use pumpkin_data::{Block, BlockStateId};
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_world::world::BlockFlags;
use uuid::Uuid;

use crate::entity::ai::test_support::{self, Identity};
use crate::entity::player::RespawnPoint;
use crate::server::Server;
use crate::world::World;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SLEEPER: Identity = Identity {
    name: "Sleeper",
    uuid: Uuid::from_u128(0x0000_00e1),
    entity_id: 9_201,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dim {
    Overworld,
    Nether,
    End,
}

/// The dimension the stored respawn point names.
#[derive(Clone, Copy)]
enum Stored {
    Known(Dim),
    /// A dimension no world of the server has.
    Unknown,
}

#[derive(Clone, Copy)]
enum What {
    Air,
    Stone,
    Water,
    Bed(HorizontalFacing, BedPart, bool),
    Anchor(u8),
}

struct Put {
    dim: Dim,
    at: BlockPos,
    what: What,
}

/// How the scene's chunks are made.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Chunks {
    /// Chunks -1..=1 around the origin are loaded, with stone floors.
    Loaded,
    /// Nothing is loaded; a stand-in chunk system answers the requests, and the line lists
    /// the chunks that ended up loaded.
    Requested,
}

struct Scene {
    name: &'static str,
    home: Dim,
    /// `None`: the player has no respawn point.
    stored: Option<(Stored, BlockPos, bool)>,
    puts: Vec<Put>,
    chunks: Chunks,
    /// Every dimension is one standalone world with no server behind it (a dead server link).
    server_gone: bool,
}

const HEAD: BlockPos = BlockPos::new(8, 65, 8);

fn scene(name: &'static str, home: Dim, stored: Option<(Stored, BlockPos, bool)>) -> Scene {
    Scene {
        name,
        home,
        stored,
        puts: Vec::new(),
        chunks: Chunks::Loaded,
        server_gone: false,
    }
}

fn put(dim: Dim, at: BlockPos, what: What) -> Put {
    Put { dim, at, what }
}

fn offset(at: BlockPos, facing: HorizontalFacing, back: i32) -> BlockPos {
    let (dx, dz) = match facing {
        HorizontalFacing::North => (0, -1),
        HorizontalFacing::South => (0, 1),
        HorizontalFacing::West => (-1, 0),
        HorizontalFacing::East => (1, 0),
    };
    BlockPos::new(at.0.x - dx * back, at.0.y, at.0.z - dz * back)
}

/// A bed whose head stands at `head`; the foot is behind it.
fn bed(dim: Dim, head: BlockPos, facing: HorizontalFacing, occupied: bool) -> Vec<Put> {
    vec![
        put(dim, head, What::Bed(facing, BedPart::Head, occupied)),
        put(
            dim,
            offset(head, facing, 1),
            What::Bed(facing, BedPart::Foot, occupied),
        ),
    ]
}

/// A solid block of stone at every cell of the box, in `dim`.
fn stone_box(dim: Dim, x: (i32, i32), y: (i32, i32), z: (i32, i32)) -> Vec<Put> {
    let mut cells = Vec::new();
    for px in x.0..=x.1 {
        for py in y.0..=y.1 {
            for pz in z.0..=z.1 {
                cells.push(put(dim, BlockPos::new(px, py, pz), What::Stone));
            }
        }
    }
    cells
}

fn with(mut scene: Scene, puts: Vec<Vec<Put>>) -> Scene {
    scene.puts = puts.into_iter().flatten().collect();
    scene
}

fn bed_point(dim: Dim) -> (Stored, BlockPos, bool) {
    (Stored::Known(dim), HEAD, false)
}

fn bed_scenes() -> Vec<Scene> {
    let mut scenes = Vec::new();
    for (name, facing) in [
        ("bed facing north", HorizontalFacing::North),
        ("bed facing south", HorizontalFacing::South),
        ("bed facing west", HorizontalFacing::West),
        ("bed facing east", HorizontalFacing::East),
    ] {
        scenes.push(with(
            scene(name, Dim::Overworld, Some(bed_point(Dim::Overworld))),
            vec![bed(Dim::Overworld, HEAD, facing, false)],
        ));
    }
    scenes.push(with(
        scene(
            "occupied bed",
            Dim::Overworld,
            Some(bed_point(Dim::Overworld)),
        ),
        vec![bed(Dim::Overworld, HEAD, HorizontalFacing::North, true)],
    ));
    scenes.push(with(
        scene(
            "stored point on the foot",
            Dim::Overworld,
            Some((
                Stored::Known(Dim::Overworld),
                BlockPos::new(8, 65, 9),
                false,
            )),
        ),
        vec![bed(Dim::Overworld, HEAD, HorizontalFacing::North, false)],
    ));
    scenes.push(with(
        scene(
            "bed boxed in by stone",
            Dim::Overworld,
            Some(bed_point(Dim::Overworld)),
        ),
        vec![
            stone_box(Dim::Overworld, (5, 11), (65, 67), (5, 12)),
            bed(Dim::Overworld, HEAD, HorizontalFacing::North, false),
        ],
    ));
    scenes.push(with(
        scene(
            "bed with one free cell far right-back",
            Dim::Overworld,
            Some(bed_point(Dim::Overworld)),
        ),
        vec![
            stone_box(Dim::Overworld, (5, 11), (65, 67), (5, 12)),
            stone_box(Dim::Overworld, (9, 9), (65, 66), (10, 10))
                .into_iter()
                .map(|cell| put(cell.dim, cell.at, What::Air))
                .collect(),
            bed(Dim::Overworld, HEAD, HorizontalFacing::North, false),
        ],
    ));
    scenes.push(with(
        scene(
            "bed on a platform",
            Dim::Overworld,
            Some((
                Stored::Known(Dim::Overworld),
                BlockPos::new(8, 66, 8),
                false,
            )),
        ),
        vec![
            vec![put(Dim::Overworld, BlockPos::new(8, 65, 8), What::Stone)],
            vec![put(Dim::Overworld, BlockPos::new(8, 65, 9), What::Stone)],
            bed(
                Dim::Overworld,
                BlockPos::new(8, 66, 8),
                HorizontalFacing::North,
                false,
            ),
        ],
    ));
    scenes.extend(bed_scenes_elsewhere());
    scenes
}

fn bed_scenes_elsewhere() -> Vec<Scene> {
    let mut scenes = vec![with(
        scene(
            "bed point, bed missing",
            Dim::Overworld,
            Some(bed_point(Dim::Overworld)),
        ),
        vec![],
    )];
    scenes.push(with(
        scene(
            "bed in the overworld, player in the nether",
            Dim::Nether,
            Some(bed_point(Dim::Overworld)),
        ),
        vec![bed(Dim::Overworld, HEAD, HorizontalFacing::East, false)],
    ));
    scenes.push(with(
        scene(
            "bed only in the overworld, point names the nether",
            Dim::Overworld,
            Some(bed_point(Dim::Nether)),
        ),
        vec![bed(Dim::Overworld, HEAD, HorizontalFacing::North, false)],
    ));
    scenes.push(with(
        scene(
            "bed in the nether, point names the nether",
            Dim::End,
            Some(bed_point(Dim::Nether)),
        ),
        vec![bed(Dim::Nether, HEAD, HorizontalFacing::West, false)],
    ));
    scenes.push(with(
        scene(
            "bed point in a dimension no world has",
            Dim::Nether,
            Some((Stored::Unknown, HEAD, false)),
        ),
        vec![bed(Dim::Overworld, HEAD, HorizontalFacing::North, false)],
    ));
    scenes
}

fn anchor_point(dim: Dim) -> (Stored, BlockPos, bool) {
    (Stored::Known(dim), HEAD, false)
}

fn anchor_scenes() -> Vec<Scene> {
    let mut scenes = Vec::new();
    for (name, charges) in [
        ("anchor with 0 charges", 0),
        ("anchor with 1 charge", 1),
        ("anchor with 2 charges", 2),
        ("anchor with 3 charges", 3),
        ("anchor with 4 charges", 4),
    ] {
        scenes.push(with(
            scene(name, Dim::Nether, Some(anchor_point(Dim::Nether))),
            vec![vec![put(Dim::Nether, HEAD, What::Anchor(charges))]],
        ));
    }
    scenes.push(with(
        scene(
            "anchor in the overworld",
            Dim::Overworld,
            Some(anchor_point(Dim::Overworld)),
        ),
        vec![vec![put(Dim::Overworld, HEAD, What::Anchor(2))]],
    ));
    scenes.push(with(
        scene(
            "anchor in the nether, player in the overworld",
            Dim::Overworld,
            Some(anchor_point(Dim::Nether)),
        ),
        vec![vec![put(Dim::Nether, HEAD, What::Anchor(4))]],
    ));
    scenes.push(with(
        scene(
            "anchor boxed in by stone",
            Dim::Nether,
            Some(anchor_point(Dim::Nether)),
        ),
        vec![
            stone_box(Dim::Nether, (6, 10), (64, 68), (6, 10)),
            vec![put(Dim::Nether, HEAD, What::Anchor(3))],
        ],
    ));
    scenes.push(with(
        scene(
            "anchor with only the cell above free",
            Dim::Nether,
            Some(anchor_point(Dim::Nether)),
        ),
        vec![
            stone_box(Dim::Nether, (7, 9), (64, 67), (7, 9)),
            vec![
                put(Dim::Nether, BlockPos::new(8, 66, 8), What::Air),
                put(Dim::Nether, BlockPos::new(8, 67, 8), What::Air),
                put(Dim::Nether, HEAD, What::Anchor(1)),
            ],
        ],
    ));
    scenes.push(with(
        scene(
            "anchor point, anchor missing",
            Dim::Nether,
            Some(anchor_point(Dim::Nether)),
        ),
        vec![],
    ));
    scenes.push(with(
        scene(
            "anchor point in the end",
            Dim::End,
            Some(anchor_point(Dim::End)),
        ),
        vec![vec![put(Dim::End, HEAD, What::Anchor(4))]],
    ));
    scenes
}

fn forced(dim: Dim, at: BlockPos) -> (Stored, BlockPos, bool) {
    (Stored::Known(dim), at, true)
}

fn forced_scenes() -> Vec<Scene> {
    let at = BlockPos::new(5, 65, 9);
    let mut scenes = vec![
        scene(
            "forced point in air",
            Dim::Overworld,
            Some(forced(Dim::Overworld, at)),
        ),
        scene(
            "forced point in air, negative x and z",
            Dim::Overworld,
            Some(forced(Dim::Overworld, BlockPos::new(-3, 65, -5))),
        ),
        scene(
            "forced point in the end",
            Dim::Overworld,
            Some(forced(Dim::End, at)),
        ),
        scene(
            "forced point in a dimension no world has",
            Dim::Nether,
            Some((Stored::Unknown, at, true)),
        ),
    ];
    scenes.push(with(
        scene(
            "forced point in stone",
            Dim::Overworld,
            Some(forced(Dim::Overworld, at)),
        ),
        vec![stone_box(Dim::Overworld, (5, 5), (65, 66), (9, 9))],
    ));
    scenes.push(with(
        scene(
            "forced point with stone only above",
            Dim::Overworld,
            Some(forced(Dim::Overworld, at)),
        ),
        vec![stone_box(Dim::Overworld, (5, 5), (66, 66), (9, 9))],
    ));
    scenes.push(with(
        scene(
            "forced point with stone only at the point",
            Dim::Overworld,
            Some(forced(Dim::Overworld, at)),
        ),
        vec![stone_box(Dim::Overworld, (5, 5), (65, 65), (9, 9))],
    ));
    scenes.push(with(
        scene(
            "forced point in water",
            Dim::Overworld,
            Some(forced(Dim::Overworld, at)),
        ),
        vec![vec![
            put(Dim::Overworld, at, What::Water),
            put(Dim::Overworld, BlockPos::new(5, 66, 9), What::Water),
        ]],
    ));
    scenes.push(with(
        scene(
            "forced point on a bed",
            Dim::Overworld,
            Some(forced(Dim::Overworld, HEAD)),
        ),
        vec![bed(Dim::Overworld, HEAD, HorizontalFacing::North, false)],
    ));
    scenes.push(with(
        scene(
            "forced point on an anchor",
            Dim::Nether,
            Some(forced(Dim::Nether, HEAD)),
        ),
        vec![vec![put(Dim::Nether, HEAD, What::Anchor(4))]],
    ));
    scenes.push(with(
        scene(
            "forced point, stone in the overworld only",
            Dim::Overworld,
            Some(forced(Dim::Nether, at)),
        ),
        vec![stone_box(Dim::Overworld, (5, 5), (65, 66), (9, 9))],
    ));
    scenes
}

fn other_scenes() -> Vec<Scene> {
    let mut scenes = vec![scene("no respawn point", Dim::Overworld, None)];
    scenes.push(with(
        scene(
            "point on stone, not forced",
            Dim::Overworld,
            Some((
                Stored::Known(Dim::Overworld),
                BlockPos::new(6, 65, 6),
                false,
            )),
        ),
        vec![vec![put(
            Dim::Overworld,
            BlockPos::new(6, 65, 6),
            What::Stone,
        )]],
    ));
    scenes.push(scene(
        "point in air, not forced",
        Dim::Overworld,
        Some((
            Stored::Known(Dim::Overworld),
            BlockPos::new(6, 65, 6),
            false,
        )),
    ));
    let mut gone = with(
        scene(
            "bed, server gone",
            Dim::Overworld,
            Some(bed_point(Dim::Nether)),
        ),
        vec![bed(Dim::Overworld, HEAD, HorizontalFacing::North, false)],
    );
    gone.server_gone = true;
    scenes.push(gone);
    scenes
}

/// Points near chunk borders in worlds with nothing loaded: the line lists the chunks the call
/// fetched around each one.
fn chunk_scenes() -> Vec<Scene> {
    [
        ("chunks around (0, 0)", BlockPos::new(0, 65, 0)),
        ("chunks around (15, -1)", BlockPos::new(15, 65, -1)),
        ("chunks around (-1, 16)", BlockPos::new(-1, 65, 16)),
        ("chunks around (18, -7)", BlockPos::new(18, 65, -7)),
        ("chunks around (-3, 3)", BlockPos::new(-3, 65, 3)),
        ("chunks around (-17, -17)", BlockPos::new(-17, 65, -17)),
    ]
    .into_iter()
    .map(|(name, at)| {
        let mut chunk_scene = scene(
            name,
            Dim::Overworld,
            Some((Stored::Known(Dim::Overworld), at, false)),
        );
        chunk_scene.chunks = Chunks::Requested;
        chunk_scene
    })
    .collect()
}

fn scenes() -> Vec<Scene> {
    let mut all = other_scenes();
    all.extend(bed_scenes());
    all.extend(anchor_scenes());
    all.extend(forced_scenes());
    all.extend(chunk_scenes());
    all
}

#[derive(Clone)]
struct Worlds {
    overworld: Arc<World>,
    nether: Arc<World>,
    end: Arc<World>,
}

impl Worlds {
    fn of(server: &Server) -> Self {
        Self {
            overworld: server.get_world_from_dimension(&Dimension::OVERWORLD),
            nether: server.get_world_from_dimension(&Dimension::THE_NETHER),
            end: server.get_world_from_dimension(&Dimension::THE_END),
        }
    }

    const fn get(&self, dim: Dim) -> &Arc<World> {
        match dim {
            Dim::Overworld => &self.overworld,
            Dim::Nether => &self.nether,
            Dim::End => &self.end,
        }
    }
}

fn state_of(what: What) -> BlockStateId {
    match what {
        What::Air => Block::AIR.default_state.id,
        What::Stone => Block::STONE.default_state.id,
        What::Water => Block::WATER.default_state.id,
        What::Bed(facing, part, occupied) => {
            let mut bed = WhiteBedLikeProperties::default(&Block::RED_BED);
            bed.facing = facing;
            bed.part = part;
            bed.occupied = occupied;
            bed.to_state_id(&Block::RED_BED)
        }
        What::Anchor(charges) => {
            RespawnAnchorLikeProperties { charges }.to_state_id(&Block::RESPAWN_ANCHOR)
        }
    }
}

fn load_chunks(world: &World) {
    for x in -1..=1 {
        for z in -1..=1 {
            world.level.loaded_chunks.insert(
                Vector2::new(x, z),
                pumpkin_world::chunk::ChunkData::empty_sync(x, z),
            );
        }
    }
}

fn lay(world: &Arc<World>, at: BlockPos, state: BlockStateId) {
    world.set_block_state(&at, state, BlockFlags::NOTIFY_ALL);
}

/// Stone floors at y = 64 around the origin of every dimension, then the scene's blocks.
fn prepare(worlds: &Worlds, scene: &Scene) {
    if scene.chunks == Chunks::Requested {
        return;
    }
    for world in [&worlds.overworld, &worlds.nether, &worlds.end] {
        load_chunks(world);
        for x in -6..=14 {
            for z in -6..=14 {
                lay(world, BlockPos::new(x, 64, z), state_of(What::Stone));
            }
        }
    }
    for cell in &scene.puts {
        lay(worlds.get(cell.dim), cell.at, state_of(cell.what));
    }
}

/// Stands in for the chunk system, which does not run in a test server: answers every request
/// for a chunk in -2..=2 with an empty chunk.
fn answer_chunk_requests(world: &Arc<World>) -> (Arc<AtomicBool>, tokio::task::JoinHandle<()>) {
    let chunks: Vec<_> = (-2..=2)
        .flat_map(|x| (-2..=2).map(move |z| (x, z)))
        .map(|(x, z)| (x, z, pumpkin_world::chunk::ChunkData::empty_sync(x, z)))
        .collect();
    let stop = Arc::new(AtomicBool::new(false));
    let task = tokio::spawn({
        let listener = Arc::clone(&world.level.chunk_listener);
        let stop = Arc::clone(&stop);
        async move {
            while !stop.load(Ordering::Acquire) {
                for (x, z, chunk) in &chunks {
                    listener.process_new_chunk(Vector2::new(*x, *z), chunk);
                }
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        }
    });
    (stop, task)
}

fn dimension_of(stored: Stored) -> Dimension {
    match stored {
        Stored::Known(Dim::Overworld) => Dimension::OVERWORLD,
        Stored::Known(Dim::Nether) => Dimension::THE_NETHER,
        Stored::Known(Dim::End) => Dimension::THE_END,
        Stored::Unknown => Dimension {
            minecraft_name: "minecraft:unknown",
            ..Dimension::OVERWORLD
        },
    }
}

fn world_of(worlds: &Worlds, stored: Stored) -> &Arc<World> {
    match stored {
        Stored::Known(dim) => worlds.get(dim),
        Stored::Unknown => &worlds.overworld,
    }
}

async fn run_scene(scene: Scene) -> Result<String, Box<dyn std::error::Error>> {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let lone_folder = tempfile::tempdir()?;
    let real = Worlds::of(&server);
    let worlds = if scene.server_gone {
        let lone = test_support::world(&lone_folder);
        Worlds {
            overworld: Arc::clone(&lone),
            nether: Arc::clone(&lone),
            end: lone,
        }
    } else {
        real.clone()
    };
    prepare(&worlds, &scene);
    let (player, _tap) =
        test_support::java_player_as(real.get(scene.home), GameMode::Survival, &SLEEPER)
            .ok_or("no Java packet tap")?;
    if scene.server_gone {
        player
            .living_entity
            .entity
            .world
            .store(Arc::clone(&worlds.overworld));
    }
    *player
        .respawn_point
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) =
        scene.stored.map(|(stored, position, force)| RespawnPoint {
            dimension: dimension_of(stored),
            position,
            yaw: 120.0,
            force,
        });
    let chunk_system =
        (scene.chunks == Chunks::Requested).then(|| answer_chunk_requests(&worlds.overworld));
    let looked_in = scene
        .stored
        .map(|(stored, _, _)| Arc::clone(world_of(&worlds, stored)));
    let found = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        player.calculate_respawn_point(),
    )
    .await
    .map_err(|_| "calculate_respawn_point did not finish in 20 s")?;
    if let Some((stop, task)) = chunk_system {
        stop.store(true, Ordering::Release);
        let _ = task.await;
    }

    let mut line = format!("{}: ", scene.name);
    match &found {
        None => line.push_str("None"),
        Some(point) => write!(
            line,
            "pos {} {} {} yaw {} pitch {} in {}",
            point.position.x,
            point.position.y,
            point.position.z,
            point.yaw,
            point.pitch,
            point.dimension.minecraft_name
        )?,
    }
    if let (Some((_, at, _)), Some(world)) = (scene.stored, &looked_in) {
        let (block, state) = world.get_block_and_state_id(&at);
        write!(line, " | stored block {} state {state}", block.name)?;
    }
    write!(
        line,
        " | server linked {}",
        player.world().server.upgrade().is_some()
    )?;
    if scene.chunks == Chunks::Requested
        && let Some(world) = &looked_in
    {
        let mut loaded: Vec<_> = world
            .level
            .loaded_chunks
            .iter()
            .map(|entry| (entry.key().x, entry.key().y))
            .collect();
        loaded.sort_unstable();
        write!(line, " | chunks {loaded:?}")?;
    }
    line.push('\n');
    server.stop();
    Ok(line)
}

/// [`run_scene`] with a `Send` error, so it can run on its own task.
async fn run_scene_sendable(scene: Scene) -> Result<String, String> {
    run_scene(scene).await.map_err(|error| error.to_string())
}

/// Runs `scenes` side by side (one server each) and joins their lines in scene order.
async fn run_all(scenes: Vec<Scene>) -> Result<String, Box<dyn std::error::Error>> {
    let runs: Vec<_> = scenes
        .into_iter()
        .map(|scene| tokio::spawn(run_scene_sendable(scene)))
        .collect();
    let mut report = String::new();
    for run in runs {
        report.push_str(&run.await??);
    }
    Ok(report)
}

#[tokio::test(flavor = "multi_thread")]
async fn calculate_respawn_point_is_the_same() -> TestResult {
    let report = run_all(scenes()).await?;
    crate::command::commands::script_golden::bless_or_compare(
        "calculate_respawn_point.txt",
        &report,
    )?;
    Ok(())
}
