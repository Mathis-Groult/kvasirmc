//! Golden records of whole structures: their piece layout and the blocks they place.
//!
//! A structure generator lays the pieces out (for mineshafts, strongholds and nether fortresses
//! that is their `add_*_children` recursion); the pieces are then placed chunk by chunk into stone
//! terrain through `StructurePiecesCollector::generate_in_chunk`, as the game does. Rewrite the
//! files on purpose with `KVASIR_BLESS=1`.

use std::fmt::Write;

use pumpkin_util::{
    random::{RandomGenerator, legacy_rand::LegacyRand, xoroshiro128::Xoroshiro},
    world_seed::Seed,
};

use super::{
    piece_golden::{Terrain, block_entities, fill_terrain, fingerprint, write_spawns_and_ticks},
    stage_parity::TestBlockRegistry,
};
use crate::generation::{
    get_world_gen,
    proto_chunk::ProtoChunk,
    structure::structures::{
        HeightSampler, StructureGenerator, StructureGeneratorContext, igloo::IglooGenerator,
        mansion::MansionGenerator, mineshaft::MineshaftGenerator,
        nether_fortress::NetherFortressGenerator, ocean_ruin::OceanRuinGenerator,
        ruined_portal::RuinedPortalGenerator, shipwreck::ShipwreckGenerator,
        stronghold::StrongholdGenerator,
    },
};

/// (mesa, seed, start chunk x, start chunk z) of each mineshaft in the golden.
const CASES: [(bool, u64, i32, i32); 6] = [
    (false, 1, 0, 0),
    (false, 2, 3, -2),
    (false, 3, -5, 4),
    (true, 1, 0, 0),
    (true, 2, 3, -2),
    (true, 3, -5, 4),
];

/// One structure started in chunk (`chunk_x`, `chunk_z`): a header line, a line per piece and a
/// line per chunk of the layout.
fn record_structure(
    out: &mut String,
    world_gen: &crate::generation::generator::WorldGenerator,
    (header, generator): (&str, &dyn StructureGenerator),
    (seed, chunk_x, chunk_z): (u64, i32, i32),
    (terrain, height_sampler): (Terrain, Option<&mut dyn HeightSampler>),
) -> Result<(), Box<dyn std::error::Error>> {
    let context = StructureGeneratorContext {
        seed: seed as i64,
        chunk_x,
        chunk_z,
        random: RandomGenerator::Legacy(LegacyRand::from_seed(seed)),
        sea_level: 63,
        min_y: -64,
        height: 384,
        height_sampler,
        structure_key: None,
    };
    let position = generator
        .get_structure_position(context)
        .ok_or("no structure")?;
    let mut collector = position
        .collector
        .lock()
        .map_err(|_| "collector lock poisoned")?;
    writeln!(
        out,
        "{header} seed={seed} chunk=({chunk_x},{chunk_z}) start={:?} pieces={}",
        position.start_pos.0,
        collector.pieces.len()
    )?;
    for piece in &collector.pieces {
        let piece = piece.get_structure_piece();
        let b = piece.bounding_box;
        writeln!(
            out,
            "  piece {:?} {:?} ({},{},{})..({},{},{})",
            piece.r#type, piece.facing, b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z
        )?;
    }
    let bounds = collector.get_bounding_box();
    for x in (bounds.min.x >> 4)..=(bounds.max.x >> 4) {
        for z in (bounds.min.z >> 4)..=(bounds.max.z >> 4) {
            let mut chunk = ProtoChunk::new(x, z, world_gen);
            fill_terrain(&mut chunk, terrain);
            let chunk_seed = seed ^ ((x as u64) << 32) ^ (z as u32 as u64);
            let mut random = RandomGenerator::Xoroshiro(Xoroshiro::from_seed(chunk_seed));
            collector.generate_in_chunk(&mut chunk, &TestBlockRegistry, &mut random, seed as i64);
            let (solid, hash) = fingerprint(&chunk);
            let (entities, entity_hash) = block_entities(&chunk);
            write!(
                out,
                "  chunk=({x},{z}) solid={solid} hash={hash:016x} be={entities}:{entity_hash:016x}"
            )?;
            write_spawns_and_ticks(out, &mut chunk)?;
            out.push('\n');
        }
    }
    Ok(())
}

/// Compares `now` with `tests/golden/<name>.txt`, or rewrites it under `KVASIR_BLESS`.
fn check_golden(name: &str, now: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.txt"));
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}

fn overworld() -> crate::generation::generator::WorldGenerator {
    *get_world_gen(
        Seed(1),
        pumpkin_data::dimension::Dimension::OVERWORLD,
        true,
        Vec::new(),
        "minecraft:plains".to_owned(),
    )
}

#[test]
fn mineshafts_lay_out_and_place_the_same_blocks() -> Result<(), Box<dyn std::error::Error>> {
    let world_gen = overworld();
    let mut now = String::new();
    for (is_mesa, seed, chunk_x, chunk_z) in CASES {
        let header = format!("mineshaft mesa={is_mesa}");
        let generator = MineshaftGenerator { is_mesa };
        record_structure(
            &mut now,
            &world_gen,
            (&header, &generator),
            (seed, chunk_x, chunk_z),
            (Terrain::Flat, None),
        )?;
    }
    check_golden("mineshafts", &now)
}

/// (seed, start chunk x, start chunk z) of the strongholds and fortresses in their goldens.
const STARTS: [(u64, i32, i32); 3] = [(1, 0, 0), (2, 3, -2), (3, -5, 4)];

#[test]
fn strongholds_lay_out_and_place_the_same_blocks() -> Result<(), Box<dyn std::error::Error>> {
    let world_gen = overworld();
    let mut now = String::new();
    for start in STARTS {
        record_structure(
            &mut now,
            &world_gen,
            ("stronghold", &StrongholdGenerator),
            start,
            (Terrain::Flat, None),
        )?;
    }
    check_golden("strongholds", &now)
}

#[test]
fn nether_fortresses_lay_out_and_place_the_same_blocks() -> Result<(), Box<dyn std::error::Error>> {
    let world_gen = overworld();
    let mut now = String::new();
    for start in STARTS {
        record_structure(
            &mut now,
            &world_gen,
            ("nether_fortress", &NetherFortressGenerator),
            start,
            (Terrain::Flat, None),
        )?;
    }
    check_golden("nether_fortresses", &now)
}

/// The surface the test terrain has everywhere: the block above the stone of `Terrain::Flat`.
struct FlatGround;

impl HeightSampler for FlatGround {
    fn estimate_height(&mut self, _block_x: i32, _block_z: i32) -> i32 {
        63
    }
}

#[test]
fn mansions_lay_out_and_place_the_same_blocks() -> Result<(), Box<dyn std::error::Error>> {
    let world_gen = overworld();
    let mut now = String::new();
    for start in STARTS {
        record_structure(
            &mut now,
            &world_gen,
            ("mansion", &MansionGenerator),
            start,
            (Terrain::Flat, Some(&mut FlatGround)),
        )?;
    }
    check_golden("mansions", &now)
}

/// Records the structures that `generator` starts at each of `starts` over `terrain`.
fn record_starts(
    header: &str,
    generator: &dyn StructureGenerator,
    terrain: Terrain,
    starts: &[(u64, i32, i32)],
) -> Result<String, Box<dyn std::error::Error>> {
    let world_gen = overworld();
    let mut now = String::new();
    for &start in starts {
        record_structure(
            &mut now,
            &world_gen,
            (header, generator),
            start,
            (terrain, Some(&mut FlatGround)),
        )?;
    }
    Ok(now)
}

#[test]
fn ocean_ruins_lay_out_and_place_the_same_blocks() -> Result<(), Box<dyn std::error::Error>> {
    let mut now = record_starts(
        "ocean_ruin warm",
        &OceanRuinGenerator { is_warm: true },
        Terrain::Seabed,
        &STARTS,
    )?;
    now += &record_starts(
        "ocean_ruin cold",
        &OceanRuinGenerator { is_warm: false },
        Terrain::Seabed,
        &STARTS,
    )?;
    check_golden("ocean_ruins", &now)
}

#[test]
fn shipwrecks_lay_out_and_place_the_same_blocks() -> Result<(), Box<dyn std::error::Error>> {
    let mut now = record_starts(
        "shipwreck ocean",
        &ShipwreckGenerator { is_beached: false },
        Terrain::Seabed,
        &STARTS,
    )?;
    now += &record_starts(
        "shipwreck beached",
        &ShipwreckGenerator { is_beached: true },
        Terrain::Flat,
        &STARTS,
    )?;
    check_golden("shipwrecks", &now)
}

#[test]
fn igloos_lay_out_and_place_the_same_blocks() -> Result<(), Box<dyn std::error::Error>> {
    // About half of the igloos have a basement: eight seeds give both kinds.
    let starts: Vec<_> = (1..=8).map(|seed| (seed, 0, 0)).collect();
    let now = record_starts("igloo", &IglooGenerator, Terrain::Flat, &starts)?;
    check_golden("igloos", &now)
}

/// The columns of a test terrain as the structure height sampler sees them: stone up to its top,
/// water to y = 62 over `Seabed`, air above.
struct TerrainColumns(Terrain);

impl HeightSampler for TerrainColumns {
    fn estimate_height(&mut self, block_x: i32, block_z: i32) -> i32 {
        self.0.top(block_x, block_z).map_or(-64, |top| top + 1)
    }

    fn sample_column_block(
        &mut self,
        block_x: i32,
        block_z: i32,
        y: i32,
    ) -> Option<&'static pumpkin_data::BlockState> {
        let top = self.0.top(block_x, block_z)?;
        Some(if y <= top {
            pumpkin_data::Block::STONE.default_state
        } else if matches!(self.0, Terrain::Seabed) && y <= 62 {
            pumpkin_data::Block::WATER.default_state
        } else {
            pumpkin_data::Block::AIR.default_state
        })
    }
}

/// Starts of the ruined portals: small seeds in chunk (0,0) and elsewhere, then large seeds,
/// whose first random draws differ more (the first floats of seeds 1-4 are close, so they all
/// pick the same setup).
const PORTAL_STARTS: [(u64, i32, i32); 8] = [
    (1, 0, 0),
    (2, 3, -2),
    (3, -5, 4),
    (4, 7, 1),
    (0x9E37_79B9_7F4A_7C15, 0, 0),
    (0xD1B5_4A32_D192_ED03, -2, 6),
    (0x8CB9_2BA7_2F3D_8DD7, 5, -3),
    (0x2545_F491_4F6C_DD1D, -7, -7),
];

#[test]
fn ruined_portals_lay_out_and_place_the_same_blocks() -> Result<(), Box<dyn std::error::Error>> {
    use pumpkin_data::structures::StructureKeys;
    let world_gen = overworld();
    let mut now = String::new();
    for (variant, terrain) in [
        (StructureKeys::RuinedPortal, Terrain::Flat),
        (StructureKeys::RuinedPortalDesert, Terrain::Flat),
        (StructureKeys::RuinedPortalJungle, Terrain::Hilly),
        (StructureKeys::RuinedPortalSwamp, Terrain::Seabed),
        (StructureKeys::RuinedPortalMountain, Terrain::Hilly),
        (StructureKeys::RuinedPortalOcean, Terrain::Seabed),
        (StructureKeys::RuinedPortalNether, Terrain::Flat),
    ] {
        let header = format!("ruined_portal {variant:?} over {terrain:?}");
        let generator = RuinedPortalGenerator { variant };
        for start in PORTAL_STARTS {
            record_structure(
                &mut now,
                &world_gen,
                (&header, &generator),
                start,
                (terrain, Some(&mut TerrainColumns(terrain))),
            )?;
        }
    }
    check_golden("ruined_portals", &now)
}

/// A structure built the way the game dispatches it: its generator chosen and configured from its
/// data (the jigsaw structures get their start pool, size and aliases there).
struct Dispatched(pumpkin_data::structures::StructureKeys);

impl StructureGenerator for Dispatched {
    fn get_structure_position(
        &self,
        context: StructureGeneratorContext<'_>,
    ) -> Option<crate::generation::structure::structures::StructurePosition> {
        let structure = pumpkin_data::structures::Structure::get(&self.0);
        crate::generation::structure::generate_structure_position(&self.0, structure, context)
    }
}

#[test]
fn jigsaw_structures_lay_out_and_place_the_same_blocks() -> Result<(), Box<dyn std::error::Error>> {
    use pumpkin_data::structures::StructureKeys;
    let world_gen = overworld();
    let mut now = String::new();
    for key in [
        StructureKeys::VillagePlains,
        StructureKeys::VillageDesert,
        StructureKeys::VillageSnowy,
        StructureKeys::PillagerOutpost,
        StructureKeys::BastionRemnant,
        StructureKeys::AncientCity,
        StructureKeys::TrailRuins,
        StructureKeys::TrialChambers,
    ] {
        let header = format!("jigsaw {key:?}");
        for start in [PORTAL_STARTS[1], PORTAL_STARTS[5]] {
            record_structure(
                &mut now,
                &world_gen,
                (&header, &Dispatched(key)),
                start,
                (Terrain::Flat, Some(&mut TerrainColumns(Terrain::Flat))),
            )?;
        }
    }
    check_golden("jigsaw_structures", &now)
}
