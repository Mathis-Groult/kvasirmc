//! Golden for `spawn_category_for_position`, one scene per branch
//! (`tests/golden/natural_spawner_category.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! The function draws from the global RNG, so a scene runs [`ATTEMPTS`] attempts and records
//! facts that do not depend on the draws: whether any attempt spawned, the sorted entity types seen,
//! and (only where a cap forces it) the largest batch.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::Arc;

use pumpkin_data::Block;
use pumpkin_data::entity::{EntityType, MobCategory};
use pumpkin_data::{BlockState, BlockStateId};
use pumpkin_util::GameMode;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_world::chunk::ChunkData;
use pumpkin_world::chunk::format::LightContainer;
use pumpkin_world::chunk::palette::BiomePalette;
use pumpkin_world::world::BlockFlags;
use rustc_hash::FxHashSet;

use crate::command::commands::script_golden::{self, TestResult};
use crate::entity::EntityBase;
use crate::entity::ai::test_support;
use crate::entity::player::Player;
use crate::world::World;
use crate::world::natural_spawner::{SpawnState, spawn_category_for_position};

const ATTEMPTS: usize = 1200;
const CENTRE: BlockPos = BlockPos::new(72, 64, 8);
const CHUNK: Vector2<i32> = Vector2::new(4, 0);
const FAR_Z: f64 = 68.5;

fn put(world: &Arc<World>, pos: &BlockPos, state: BlockStateId) {
    world.set_block_state(
        pos,
        state,
        BlockFlags::FORCE_STATE | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK,
    );
}

fn chunks() -> impl Iterator<Item = (i32, i32)> {
    (2..=7).flat_map(|cx| (-2..=3).map(move |cz| (cx, cz)))
}

fn set_biome(world: &Arc<World>, name: &str) {
    let id = pumpkin_data::chunk::Biome::from_name(name)
        .expect("biome")
        .id;
    for (cx, cz) in chunks() {
        world.level.read_chunk_sync(&Vector2::new(cx, cz), |chunk| {
            let mut sections = chunk.section.biome_sections.write().unwrap();
            let len = sections.len();
            *sections = vec![BiomePalette::Homogeneous(id); len].into_boxed_slice();
        });
    }
}

fn set_light(world: &Arc<World>, sky: u8, block: u8) {
    for (cx, cz) in chunks() {
        world.level.read_chunk_sync(&Vector2::new(cx, cz), |chunk| {
            let count = chunk.section.count;
            let mut light = chunk.light_engine.lock().unwrap();
            light.sky_light = (0..count)
                .map(|_| LightContainer::new_filled(sky))
                .collect();
            light.block_light = (0..count)
                .map(|_| LightContainer::new_filled(block))
                .collect();
        });
    }
}

fn set_active(world: &Arc<World>, active: bool) {
    let set: FxHashSet<Vector2<i32>> = if active {
        chunks().map(|(x, z)| Vector2::new(x, z)).collect()
    } else {
        FxHashSet::default()
    };
    *world.active_chunks.write().unwrap() = Arc::new(set);
}

/// Stone at `base - 2`, then `floor` (or `fluid`) at `base - 1`, `fluid` (or air) up to `base + 2`.
fn lay(world: &Arc<World>, fluid: Option<&Block>, floor: &Block, base: i32) {
    let air = Block::AIR.default_state.id;
    let above = fluid.map_or(air, |block| block.default_state.id);
    let under = fluid.map_or(floor, |block| block).default_state.id;
    for x in 48..=96 {
        for z in -16..=32 {
            put(
                world,
                &BlockPos::new(x, base - 2, z),
                Block::STONE.default_state.id,
            );
            put(world, &BlockPos::new(x, base - 1, z), under);
            for y in base..=base + 2 {
                put(world, &BlockPos::new(x, y, z), above);
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Show {
    Spawned,
    Types,
    Largest,
    Shape,
}

struct Scene<'a> {
    name: &'a str,
    category: &'static MobCategory,
    pos: BlockPos,
    chunk: Vector2<i32>,
    thunder: bool,
    attempts: usize,
    show: Show,
    /// Print whether the share of attempts that spawned, in percent, lies in this band.
    rate: Option<(f64, f64)>,
    /// Print whether this type ever spawned on the exact block column of the position.
    exact: Option<&'static EntityType>,
}

const fn scene<'a>(name: &'a str, category: &'static MobCategory) -> Scene<'a> {
    Scene {
        name,
        category,
        pos: CENTRE,
        chunk: CHUNK,
        thunder: false,
        attempts: ATTEMPTS,
        show: Show::Types,
        rate: None,
        exact: None,
    }
}

/// What the attempts of one scene produced.
#[derive(Default)]
struct Tally {
    spawned: usize,
    hit_attempts: usize,
    inside: bool,
    seen: BTreeSet<&'static str>,
    largest: BTreeMap<&'static str, usize>,
    exact: bool,
    n: f64,
    x: [f64; 2],
    z: [f64; 2],
    yaw: [f64; 2],
    yaw_max: f64,
    pitch_max: f32,
}

fn moments(sum: &mut [f64; 2], value: f64) {
    sum[0] += value;
    sum[1] += value * value;
}

impl Tally {
    fn add(&mut self, scene: &Scene, mob: &Arc<dyn EntityBase>) {
        let entity = mob.get_entity();
        let at = entity.pos.load();
        let dx = at.x - f64::from(scene.pos.0.x);
        let dz = at.z - f64::from(scene.pos.0.z);
        self.inside &= (at.y - f64::from(scene.pos.0.y)).abs() < f64::EPSILON
            && dx.abs() <= 40.0
            && dz.abs() <= 40.0
            && at.x.rem_euclid(1.0) == 0.5
            && at.z.rem_euclid(1.0) == 0.5;
        let name = entity.entity_type.resource_name;
        self.seen.insert(name);
        if Some(entity.entity_type) == scene.exact {
            self.exact |= (dx - 0.5).abs() < f64::EPSILON && (dz - 0.5).abs() < f64::EPSILON;
        }
        self.n += 1.0;
        moments(&mut self.x, dx);
        moments(&mut self.z, dz);
        let yaw = f64::from(entity.yaw.load());
        moments(&mut self.yaw, yaw);
        self.yaw_max = self.yaw_max.max(yaw);
        self.pitch_max = self.pitch_max.max(entity.pitch.load().abs());
    }

    fn mean(&self, sum: &[f64; 2]) -> f64 {
        sum[0] / self.n
    }

    fn std(&self, sum: &[f64; 2]) -> f64 {
        let mean = self.mean(sum);
        (sum[1] / self.n - mean * mean).sqrt()
    }
}

fn collect(world: &Arc<World>, scene: &Scene, state: &SpawnState) -> Tally {
    let mut tally = Tally {
        inside: true,
        ..Tally::default()
    };
    for _ in 0..scene.attempts {
        let batch = spawn_category_for_position(
            scene.category,
            world,
            scene.pos,
            &scene.chunk,
            state,
            scene.thunder,
        );
        tally.spawned += batch.len();
        if let Some(first) = batch.first() {
            tally.hit_attempts += 1;
            let name = first.get_entity().entity_type.resource_name;
            let largest = tally.largest.entry(name).or_default();
            *largest = (*largest).max(batch.len());
        }
        for mob in &batch {
            tally.add(scene, mob);
        }
    }
    tally
}

fn largest(tally: &Tally, out: &mut String) {
    let sizes: Vec<String> = tally
        .largest
        .iter()
        .map(|(name, size)| format!("{name}={size}"))
        .collect();
    let _ = write!(out, " largest {sizes:?}");
}

fn within(value: f64, low: f64, high: f64) -> bool {
    (low..=high).contains(&value)
}

/// Where the spawned mobs stand and face. The walk is symmetric around the position and the block
/// centre adds 0.5; yaw is uniform in 0..360 (mean 180, spread 104) and pitch is 0.
fn shape(tally: &Tally, out: &mut String) {
    let _ = write!(
        out,
        " mean_x_ok {} mean_z_ok {} std_x_ok {} std_z_ok {}",
        within(tally.mean(&tally.x), 0.2, 0.8),
        within(tally.mean(&tally.z), 0.2, 0.8),
        within(tally.std(&tally.x), 3.4, 4.0),
        within(tally.std(&tally.z), 3.4, 4.0),
    );
    let _ = write!(
        out,
        " yaw_ok {} yaw_max_350 {} pitch_zero {}",
        within(tally.mean(&tally.yaw), 170.0, 190.0) && within(tally.std(&tally.yaw), 95.0, 112.0),
        tally.yaw_max > 350.0,
        tally.pitch_max == 0.0,
    );
}

fn run(world: &Arc<World>, scene: &Scene, state: &SpawnState, out: &mut String) {
    let tally = collect(world, scene, state);
    let _ = write!(out, "{}: spawned {}", scene.name, tally.spawned > 0);
    if scene.show != Show::Spawned {
        let _ = write!(out, " types {:?}", tally.seen);
    }
    let _ = write!(out, " in_range {}", tally.inside);
    if scene.show == Show::Largest {
        largest(&tally, out);
    }
    if scene.show == Show::Shape {
        largest(&tally, out);
        shape(&tally, out);
    }
    if scene.show == Show::Largest {
        let _ = write!(out, " total_under_100 {}", tally.spawned < 100);
    }
    if let Some((low, high)) = scene.rate {
        let percent = 100.0 * tally.hit_attempts as f64 / scene.attempts as f64;
        let _ = write!(out, " rate_ok {}", within(percent, low, high));
    }
    if let Some(kind) = scene.exact {
        let _ = write!(out, " {}_at_exact {}", kind.resource_name, tally.exact);
    }
    out.push('\n');
}

fn walk_charge(world: &Arc<World>, state: &SpawnState, entity: &'static EntityType) {
    for x in 44..=100 {
        for z in -20..=36 {
            state.after_spawn(entity, &BlockPos::new(x, 64, z), world);
        }
    }
}

fn fresh() -> SpawnState {
    SpawnState::empty()
}

/// A scene with fewer attempts and no type list: only whether anything spawned matters.
fn plain<'a>(name: &'a str, category: &'static MobCategory) -> Scene<'a> {
    Scene {
        attempts: 200,
        show: Show::Spawned,
        ..scene(name, category)
    }
}

/// Both a monster and a creature scene: creatures may spawn far from every player, so only they
/// show what happens when the nearest-player distance is wrong.
fn both(world: &Arc<World>, name: &str, out: &mut String) {
    run(
        world,
        &plain(&format!("{name}_monster"), &MobCategory::MONSTER),
        &fresh(),
        out,
    );
    run(
        world,
        &plain(&format!("{name}_creature"), &MobCategory::CREATURE),
        &fresh(),
        out,
    );
}

async fn player_scenes(
    world: &Arc<World>,
    survival: &Arc<Player>,
    spectator: &Arc<Player>,
    out: &mut String,
) -> TestResult {
    // Branch: players.
    both(world, "no_player", out);
    spectator
        .get_entity()
        .set_pos(Vector3::new(72.5, 64.0, FAR_Z));
    world.add_player(spectator)?;
    both(world, "spectator_only", out);
    world.add_player(survival)?;
    run(
        world,
        &scene("survival_far", &MobCategory::MONSTER),
        &fresh(),
        out,
    );

    // Branch: nearest player distance (too near, vertical only, beyond despawn range).
    let near = test_support::java_player(world, GameMode::Survival);
    near.get_entity().set_pos(Vector3::new(72.5, 64.0, 8.5));
    world.add_player(&near)?;
    run(
        world,
        &plain("second_player_too_near", &MobCategory::MONSTER),
        &fresh(),
        out,
    );
    near.get_entity().set_pos(Vector3::new(72.5, 89.0, 8.5));
    run(
        world,
        &plain("player_25_above", &MobCategory::MONSTER),
        &fresh(),
        out,
    );
    near.get_entity().set_pos(Vector3::new(72.5, 39.0, 8.5));
    run(
        world,
        &plain("player_25_below", &MobCategory::MONSTER),
        &fresh(),
        out,
    );
    near.get_entity()
        .set_pos(Vector3::new(72.5 + 40.0, 64.0, 8.5));
    run(
        world,
        &plain("player_x_offset", &MobCategory::MONSTER),
        &fresh(),
        out,
    );
    world.remove_player(&near, false).await;
    spectator
        .get_entity()
        .set_pos(Vector3::new(72.5, 64.0, 8.5));
    run(
        world,
        &plain("spectator_near_ignored", &MobCategory::MONSTER),
        &fresh(),
        out,
    );
    spectator
        .get_entity()
        .set_pos(Vector3::new(72.5, 64.0, FAR_Z));
    survival
        .get_entity()
        .set_pos(Vector3::new(72.5, 64.0, 8.5 + 200.0));
    run(
        world,
        &plain("monster_player_200_away", &MobCategory::MONSTER),
        &fresh(),
        out,
    );
    survival
        .get_entity()
        .set_pos(Vector3::new(72.5, 64.0, FAR_Z));

    // Branch: a player 20 blocks off needs several steps of the walk to reach a legal position.
    survival
        .get_entity()
        .set_pos(Vector3::new(72.5, 64.0, 8.5 - 20.0));
    let mut ring = plain("player_20_behind", &MobCategory::MONSTER);
    ring.attempts = 4000;
    ring.rate = Some((43.0, 51.0));
    run(world, &ring, &fresh(), out);
    survival
        .get_entity()
        .set_pos(Vector3::new(72.5, 64.0, FAR_Z));
    Ok(())
}

fn solid_not_full_cube() -> Option<&'static Block> {
    (0..BlockStateId::COUNT)
        .filter_map(BlockStateId::new)
        .map(BlockState::from_id)
        .find(|state| state.is_solid_block() && !state.is_full_cube())
        .map(|state| Block::from_state_id(state.id))
}

fn block_scenes(world: &Arc<World>, out: &mut String) {
    // Branch: block at the position.
    let mut blocks = vec![
        ("stone", &Block::STONE),
        ("glass", &Block::GLASS),
        ("slab", &Block::OAK_SLAB),
        ("torch", &Block::TORCH),
    ];
    blocks.extend(solid_not_full_cube().map(|block| ("solid_not_full", block)));
    for (label, block) in blocks {
        put(world, &CENTRE, block.default_state.id);
        run(
            world,
            &plain(
                &format!("block_{label}_{}", block.name),
                &MobCategory::MONSTER,
            ),
            &fresh(),
            out,
        );
    }
    put(world, &CENTRE, Block::AIR.default_state.id);

    // Branch: chunk and world border.
    let mut s = plain("other_chunk_active", &MobCategory::MONSTER);
    s.chunk = Vector2::new(5, 1);
    run(world, &s, &fresh(), out);
    set_active(world, false);
    run(
        world,
        &plain("inactive_own_chunk", &MobCategory::MONSTER),
        &fresh(),
        out,
    );
    s.chunk = Vector2::new(40, 40);
    s.name = "other_chunk_inactive";
    run(world, &s, &fresh(), out);
    set_active(world, true);
    world
        .worldborder
        .lock()
        .unwrap()
        .set_diameter(world, 4.0, None);
    run(
        world,
        &plain("tiny_border", &MobCategory::MONSTER),
        &fresh(),
        out,
    );
    world
        .worldborder
        .lock()
        .unwrap()
        .set_diameter(world, 59_999_968.0, None);
}

fn light_scenes(world: &Arc<World>, out: &mut String) {
    // Branch: light and thunder.
    set_light(world, 15, 0);
    run(
        world,
        &plain("sky_15", &MobCategory::MONSTER),
        &fresh(),
        out,
    );
    let mut s = plain("sky_15_thunder", &MobCategory::MONSTER);
    s.thunder = true;
    run(world, &s, &fresh(), out);
    set_light(world, 0, 15);
    run(
        world,
        &plain("block_light_15", &MobCategory::MONSTER),
        &fresh(),
        out,
    );
    set_light(world, 0, 0);
}

fn category_scenes(world: &Arc<World>, out: &mut String) {
    // Branch: categories and biome lists.
    lay(world, None, &Block::GRASS_BLOCK, 64);
    set_light(world, 15, 0);
    let mut s = scene("plains_creature_dry", &MobCategory::CREATURE);
    s.attempts = 1500;
    s.show = Show::Shape;
    run(world, &s, &fresh(), out);
    for (label, category) in [
        ("ambient", &MobCategory::AMBIENT),
        ("water_creature", &MobCategory::WATER_CREATURE),
        ("water_ambient", &MobCategory::WATER_AMBIENT),
        (
            "underground_water",
            &MobCategory::UNDERGROUND_WATER_CREATURE,
        ),
        ("axolotls", &MobCategory::AXOLOTLS),
    ] {
        run(
            world,
            &plain(&format!("plains_{label}_dry"), category),
            &fresh(),
            out,
        );
    }
    set_biome(world, "mushroom_fields");
    run(
        world,
        &plain("mushroom_monster_empty", &MobCategory::MONSTER),
        &fresh(),
        out,
    );
    run(
        world,
        &plain("mushroom_creature", &MobCategory::CREATURE),
        &fresh(),
        out,
    );
    set_biome(world, "plains");
}

fn group_scenes(world: &Arc<World>, out: &mut String) {
    // Branch: group data and the cluster cap.
    set_biome(world, "soul_sand_valley");
    let mut s = scene("soul_sand_valley_monster", &MobCategory::MONSTER);
    s.show = Show::Largest;
    run(world, &s, &fresh(), out);
    let charged = fresh();
    walk_charge(world, &charged, &EntityType::ENDERMAN);
    run(
        world,
        &plain("soul_sand_valley_charged", &MobCategory::MONSTER),
        &charged,
        out,
    );
    set_biome(world, "plains");
}

fn parrot_scenes(world: &Arc<World>, survival: &Arc<Player>, out: &mut String) {
    // Branch: mobs that vanish when far away (parrot) keep out beyond 128 blocks, others stay.
    set_biome(world, "jungle");
    run(
        world,
        &plain("jungle_creature_near", &MobCategory::CREATURE),
        &fresh(),
        out,
    );
    survival
        .get_entity()
        .set_pos(Vector3::new(72.5, 64.0, 8.5 + 200.0));
    run(
        world,
        &plain("jungle_creature_200_away", &MobCategory::CREATURE),
        &fresh(),
        out,
    );
    // Exactly 128 blocks from the position (not beyond it): a parrot still spawns there.
    survival
        .get_entity()
        .set_pos(Vector3::new(72.5, 64.0, 8.5 + 128.0));
    let mut exact = plain("jungle_creature_128_away", &MobCategory::CREATURE);
    exact.attempts = 1500;
    exact.exact = Some(&EntityType::PARROT);
    run(world, &exact, &fresh(), out);
    survival
        .get_entity()
        .set_pos(Vector3::new(72.5, 64.0, FAR_Z));
    set_biome(world, "plains");
}

fn water_scenes(world: &Arc<World>, out: &mut String) {
    lay(world, None, &Block::STONE, 64);
    set_light(world, 0, 0);

    // Branch: water, at sea level minus a few blocks and deep underground.
    for (biome, y, categories) in [
        (
            "ocean",
            55,
            [
                ("water_creature", &MobCategory::WATER_CREATURE),
                ("water_ambient", &MobCategory::WATER_AMBIENT),
                ("monster", &MobCategory::MONSTER),
            ],
        ),
        (
            "lush_caves",
            28,
            [
                ("axolotls", &MobCategory::AXOLOTLS),
                ("ambient", &MobCategory::AMBIENT),
                (
                    "underground_water",
                    &MobCategory::UNDERGROUND_WATER_CREATURE,
                ),
            ],
        ),
        (
            "dripstone_caves",
            28,
            [
                (
                    "underground_water",
                    &MobCategory::UNDERGROUND_WATER_CREATURE,
                ),
                ("ambient", &MobCategory::AMBIENT),
                ("monster", &MobCategory::MONSTER),
            ],
        ),
    ] {
        set_biome(world, biome);
        lay(world, Some(&Block::WATER), &Block::STONE, y);
        for (label, category) in categories {
            let name = format!("{biome}_{label}_wet");
            let mut s = scene(&name, category);
            s.pos = BlockPos::new(72, y, 8);
            s.attempts = 300;
            run(world, &s, &fresh(), out);
        }
        lay(world, None, &Block::STONE, y);
        for (label, category) in categories {
            let name = format!("{biome}_{label}_dry");
            let mut s = scene(&name, category);
            s.pos = BlockPos::new(72, y, 8);
            s.attempts = 300;
            run(world, &s, &fresh(), out);
        }
    }
    reduced_scenes(world, out);
    set_biome(world, "plains");
}

/// Rivers cut water-ambient spawns to 2 % of the tries: one attempt is three tries at most, so
/// the share of attempts that spawn shows how many clusters an attempt makes.
fn reduced_scenes(world: &Arc<World>, out: &mut String) {
    set_biome(world, "river");
    lay(world, Some(&Block::WATER), &Block::STONE, 55);
    let mut s = scene("river_water_ambient_wet", &MobCategory::WATER_AMBIENT);
    s.pos = BlockPos::new(72, 55, 8);
    s.attempts = 20_000;
    s.rate = Some((4.9, 6.5));
    run(world, &s, &fresh(), out);
}

#[tokio::test(flavor = "multi_thread")]
async fn natural_spawner_category_golden() -> TestResult {
    let folder = tempfile::tempdir()?;
    let server = test_support::server(&folder).await?;
    let world = server.worlds.load()[0].clone();
    for (cx, cz) in chunks() {
        world
            .level
            .loaded_chunks
            .insert(Vector2::new(cx, cz), ChunkData::empty_sync(cx, cz));
    }
    set_active(&world, true);
    set_light(&world, 0, 0);
    lay(&world, None, &Block::STONE, 64);
    set_biome(&world, "plains");
    let survival = test_support::java_player(&world, GameMode::Survival);
    survival
        .get_entity()
        .set_pos(Vector3::new(72.5, 64.0, FAR_Z));
    let spectator = test_support::java_player(&world, GameMode::Spectator);
    let mut out = String::new();
    player_scenes(&world, &survival, &spectator, &mut out).await?;
    block_scenes(&world, &mut out);
    light_scenes(&world, &mut out);
    category_scenes(&world, &mut out);
    group_scenes(&world, &mut out);
    parrot_scenes(&world, &survival, &mut out);
    water_scenes(&world, &mut out);
    world.remove_player(&survival, false).await;
    world.remove_player(&spectator, false).await;
    server.stop();
    script_golden::bless_or_compare("natural_spawner_category.txt", &out)
}
