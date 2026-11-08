//! A record of the component stacks `ProtoNoiseRouters::generate_proto_stack` builds, made before
//! the function was split into small ones (`tests/golden/proto_stack.txt`; rewrite it on purpose
//! with `KVASIR_BLESS=1`).
//!
//! For every base router (overworld, amplified, large biomes, nether, end), a few seeds and both
//! random sources, the three stacks of the router are generated; each component gives its range
//! (`min`, `max`) and its value at a few positions, sampled as the top of the stack up to it, and
//! one line per stack holds a hash of those and how many components there are.

use std::fmt::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};

use pumpkin_data::noise_router::{
    AMPLIFIED_BASE_NOISE_ROUTER, BaseNoiseFunctionComponent, END_BASE_NOISE_ROUTER,
    LARGE_BIOMES_BASE_NOISE_ROUTER, NETHER_BASE_NOISE_ROUTER, OVERWORLD_BASE_NOISE_ROUTER,
};
use pumpkin_util::math::vector3::Vector3;

use super::chunk_density_function::ChunkNoiseFunctionBuilderOptions;
use super::chunk_noise_router::{ChunkNoiseFunctionComponent, ChunkNoiseRouter};
use super::density_function::NoiseFunctionComponentRange;
use super::proto_noise_router::{ProtoNoiseRouter, ProtoNoiseRouters};
use crate::generation::GlobalRandomConfig;

/// FNV-1a: a hash that does not change with the Rust version.
struct Fnv(u64);

impl Fnv {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }

    fn float(&mut self, value: f32) {
        self.bytes(&value.to_bits().to_le_bytes());
    }
}

const POSITIONS: [(i32, i32, i32); 4] = [(0, 64, 0), (-37, 12, 91), (200, -50, -131), (5, 300, 77)];

/// Wraps a generated stack in a router so that it can be sampled like a chunk's.
fn router_of(
    stack: Box<[super::proto_noise_router::ProtoNoiseFunctionComponent]>,
) -> ProtoNoiseRouter {
    ProtoNoiseRouter {
        full_component_stack: stack,
        barrier_noise: 0,
        fluid_level_floodedness_noise: 0,
        fluid_level_spread_noise: 0,
        lava_noise: 0,
        erosion: 0,
        depth: 0,
        final_density: 0,
        vein_toggle: 0,
        vein_ridged: 0,
        vein_gap: 0,
    }
}

/// The line of one stack: its length, the hash of every component's range and values, and how
/// many components could not be sampled on their own (those panic).
fn stack_line(base: &[BaseNoiseFunctionComponent], config: &GlobalRandomConfig) -> String {
    let proto = ProtoNoiseRouters::generate_proto_stack(base, config);
    let length = proto.len();
    let router = router_of(proto);
    let options = ChunkNoiseFunctionBuilderOptions::new(Vec::new(), Vec::new(), None);
    let mut chunk_router = ChunkNoiseRouter::generate(&router, &options);

    let mut hash = Fnv::new();
    let mut panics = 0;
    let stack = chunk_router.component_stack_mut();
    for index in 0..stack.len() {
        hash.float(stack[index].min());
        hash.float(stack[index].max());
        for (x, y, z) in POSITIONS {
            let pos = Vector3::new(x, y, z);
            let sampled = catch_unwind(AssertUnwindSafe(|| {
                ChunkNoiseFunctionComponent::sample_from_stack(&mut stack[..=index], &pos)
            }));
            match sampled {
                Ok(value) => hash.float(value),
                Err(_) => {
                    panics += 1;
                    hash.bytes(b"panic");
                }
            }
        }
    }
    format!("components={length} hash={:016x} panics={panics}", hash.0)
}

fn lines() -> Result<String, std::fmt::Error> {
    let routers = [
        ("overworld", &OVERWORLD_BASE_NOISE_ROUTER),
        ("amplified", &AMPLIFIED_BASE_NOISE_ROUTER),
        ("large_biomes", &LARGE_BIOMES_BASE_NOISE_ROUTER),
        ("nether", &NETHER_BASE_NOISE_ROUTER),
        ("end", &END_BASE_NOISE_ROUTER),
    ];
    let mut out = String::new();
    for (name, base) in routers {
        for seed in [0_u64, 1_779_920_288_596_261_407] {
            for legacy in [false, true] {
                let config = GlobalRandomConfig::new(seed, legacy);
                writeln!(
                    out,
                    "{name} seed={seed} legacy={legacy} noise: {}",
                    stack_line(base.noise.full_component_stack, &config)
                )?;
                writeln!(
                    out,
                    "{name} seed={seed} legacy={legacy} surface: {}",
                    stack_line(base.surface_estimator.full_component_stack, &config)
                )?;
                writeln!(
                    out,
                    "{name} seed={seed} legacy={legacy} multi_noise: {}",
                    stack_line(base.multi_noise.full_component_stack, &config)
                )?;
            }
        }
    }
    Ok(out)
}

#[test]
fn proto_stacks_are_built_as_before() -> Result<(), Box<dyn std::error::Error>> {
    let now = lines()?;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("proto_stack.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}
