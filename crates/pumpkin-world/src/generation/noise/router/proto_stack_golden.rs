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
            if let Ok(value) = sampled {
                hash.float(value);
            } else {
                panics += 1;
                hash.bytes(b"panic");
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
        for seed in [0u64, 1_779_920_288_596_261_407] {
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

// The real stacks only reach some sign cases of the range arithmetic (the ranges of two products
// are mostly of mixed sign), so the ranges of binary, linear and unary components are also
// recorded for synthetic stacks whose inputs have every kind of range: positive, negative,
// containing zero, starting or ending at zero, and a single point.

use pumpkin_data::noise_router::{
    BinaryData, BinaryOperation, LinearData, LinearOperation, UnaryData, UnaryOperation,
};

const RANGES: [(f32, f32); 12] = [
    (1.0, 2.0),
    (-3.0, -1.0),
    (-2.0, 5.0),
    (0.0, 4.0),
    (-4.0, 0.0),
    (0.0, 0.0),
    (2.0, 2.0),
    (-1.0, -1.0),
    (0.5, 0.75),
    (-2.0, -2.0),
    (-3.0, -0.5),
    (3.0, 6.0),
];

const BINARY: [(BinaryOperation, &str); 7] = [
    (BinaryOperation::Add, "add"),
    (BinaryOperation::Mul, "mul"),
    (BinaryOperation::Min, "min"),
    (BinaryOperation::Max, "max"),
    (BinaryOperation::Sub, "sub"),
    (BinaryOperation::Div, "div"),
    (BinaryOperation::Pow, "pow"),
];

const UNARY: [(UnaryOperation, &str); 11] = [
    (UnaryOperation::Abs, "abs"),
    (UnaryOperation::Square, "square"),
    (UnaryOperation::Cube, "cube"),
    (UnaryOperation::HalfNegative, "half_negative"),
    (UnaryOperation::QuarterNegative, "quarter_negative"),
    (UnaryOperation::Squeeze, "squeeze"),
    (UnaryOperation::Invert, "invert"),
    (UnaryOperation::Negate, "negate"),
    (UnaryOperation::Sqrt, "sqrt"),
    (UnaryOperation::Log, "log"),
    (UnaryOperation::Sign, "sign"),
];

/// Adds the constants for `value` once and returns the index of its constant.
fn constant(stack: &mut Vec<BaseNoiseFunctionComponent>, value: f32) -> usize {
    stack.push(BaseNoiseFunctionComponent::Constant { value });
    stack.len() - 1
}

/// A component whose range is `low..=high`: an interval select over two constants.
fn with_range(stack: &mut Vec<BaseNoiseFunctionComponent>, (low, high): (f32, f32)) -> usize {
    let low_index = constant(stack, low);
    let high_index = constant(stack, high);
    stack.push(BaseNoiseFunctionComponent::IntervalSelect {
        input_index: low_index,
        thresholds: &[],
        functions_indices: Box::leak(Box::new([low_index, high_index])),
    });
    stack.len() - 1
}

fn range_lines() -> Result<String, std::fmt::Error> {
    let mut stack = Vec::new();
    let inputs: Vec<usize> = RANGES.iter().map(|r| with_range(&mut stack, *r)).collect();
    // What each component after the inputs is, to label the lines.
    let mut labels = Vec::new();
    for (a, first) in inputs.iter().enumerate() {
        for (b, second) in inputs.iter().enumerate() {
            for (operation, name) in BINARY {
                stack.push(BaseNoiseFunctionComponent::Binary {
                    argument1_index: *first,
                    argument2_index: *second,
                    data: Box::leak(Box::new(BinaryData { operation })),
                });
                labels.push(format!("binary {name} {:?} {:?}", RANGES[a], RANGES[b]));
            }
        }
    }
    for (a, input) in inputs.iter().enumerate() {
        for argument in [-2.0f32, -1.0, -0.5, 0.0, 0.5, 1.0, 3.0] {
            for (operation, name) in [(LinearOperation::Add, "add"), (LinearOperation::Mul, "mul")]
            {
                stack.push(BaseNoiseFunctionComponent::Linear {
                    input_index: *input,
                    data: Box::leak(Box::new(LinearData {
                        operation,
                        argument,
                    })),
                });
                labels.push(format!("linear {name} {argument} {:?}", RANGES[a]));
            }
        }
        for (operation, name) in UNARY {
            stack.push(BaseNoiseFunctionComponent::Unary {
                input_index: *input,
                data: Box::leak(Box::new(UnaryData { operation })),
            });
            labels.push(format!("unary {name} {:?}", RANGES[a]));
        }
    }
    // Interval selects over three ranges: the covering range.
    for (a, first) in inputs.iter().enumerate() {
        for (b, second) in inputs.iter().enumerate() {
            stack.push(BaseNoiseFunctionComponent::IntervalSelect {
                input_index: *first,
                thresholds: &[],
                functions_indices: Box::leak(Box::new([
                    *first,
                    *second,
                    inputs[(a + b) % RANGES.len()],
                ])),
            });
            labels.push(format!(
                "interval {:?} {:?} {:?}",
                RANGES[a],
                RANGES[b],
                RANGES[(a + b) % RANGES.len()]
            ));
        }
    }

    let config = GlobalRandomConfig::new(0, false);
    let proto = ProtoNoiseRouters::generate_proto_stack(&stack, &config);
    let first = proto.len() - labels.len();
    let mut out = String::new();
    for (offset, label) in labels.iter().enumerate() {
        let component = &proto[first + offset];
        writeln!(
            out,
            "{label} => {:08x} {:08x}",
            component.min().to_bits(),
            component.max().to_bits()
        )?;
    }
    Ok(out)
}

#[test]
fn component_ranges_are_computed_as_before() -> Result<(), Box<dyn std::error::Error>> {
    let now = range_lines()?;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("proto_stack_ranges.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("golden path has no parent")?)?;
        std::fs::write(&path, &now)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, now);
    Ok(())
}
