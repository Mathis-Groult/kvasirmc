//! A record of what `Slice::sample_volume` does, made before it is split
//! (`tests/golden/slice_sample_volume.txt`; rewrite it on purpose with `KVASIR_BLESS=1`).
//!
//! The input stack holds four distance functions (different points and metrics) and a
//! constant, so every `input_index` reads another value and a stack cut one element too
//! short or too long changes the result. For each axis and each input index the slice is
//! sampled over volumes that differ in sizes, minimums (negative too), steps (all different)
//! and in how they meet the slice coordinate: size 1 at the coordinate (direct path), size 1
//! next to it, a longer extent starting at it, and so on. A line holds the FNV hash of the
//! whole buffer. The buffer is also compared point by point with `Slice::sample` and with a
//! sentinel-filled start, so the volume path must equal the point path, every cell written.

use std::fmt::Write;

use pumpkin_data::noise_router::{Axis, DistanceMetric, DistanceToPointData};
use pumpkin_util::math::vector3::Vector3;

use super::math::Constant;
use super::misc::{DistanceToPoint, Slice};
use crate::generation::noise::router::chunk_noise_router::{
    ChunkNoiseFunctionComponent, StaticChunkNoiseFunctionComponentImpl,
};
use crate::generation::noise::router::density_volume::DensityVolume;
use crate::generation::noise::router::proto_noise_router::IndependentProtoNoiseFunctionComponent;

static EUCLID: DistanceToPointData = DistanceToPointData {
    point: [3, -5, 7],
    metric: DistanceMetric::Euclidean,
};
static SQUARED: DistanceToPointData = DistanceToPointData {
    point: [-11, 2, 13],
    metric: DistanceMetric::EuclideanSquared,
};
static MANHATTAN: DistanceToPointData = DistanceToPointData {
    point: [17, 19, -23],
    metric: DistanceMetric::Manhattan,
};
static CHEBYSHEV: DistanceToPointData = DistanceToPointData {
    point: [-29, 31, 37],
    metric: DistanceMetric::Chebyshev,
};

const COORDINATE: i32 = 5;
const INPUT_COUNT: usize = 5;

fn inputs() -> [IndependentProtoNoiseFunctionComponent; INPUT_COUNT] {
    [
        IndependentProtoNoiseFunctionComponent::DistanceToPoint(DistanceToPoint::new(&EUCLID)),
        IndependentProtoNoiseFunctionComponent::DistanceToPoint(DistanceToPoint::new(&SQUARED)),
        IndependentProtoNoiseFunctionComponent::Constant(Constant::new(42.5)),
        IndependentProtoNoiseFunctionComponent::DistanceToPoint(DistanceToPoint::new(&MANHATTAN)),
        IndependentProtoNoiseFunctionComponent::DistanceToPoint(DistanceToPoint::new(&CHEBYSHEV)),
    ]
}

const fn axis_name(axis: Axis) -> char {
    match axis {
        Axis::X => 'x',
        Axis::Y => 'y',
        Axis::Z => 'z',
    }
}

/// Sizes `(x, y, z)`, minimums and steps; every axis has its own step so swapped steps show.
struct Case {
    size: [usize; 3],
    min: [i32; 3],
    step: [i32; 3],
}

impl Case {
    const fn volume(&self) -> DensityVolume {
        DensityVolume::new(
            self.size[0],
            self.size[1],
            self.size[2],
            self.min[0],
            self.min[1],
            self.min[2],
            self.step[0],
            self.step[1],
            self.step[2],
        )
    }
}

const STEP: [i32; 3] = [2, 3, 4];

/// Cases for the axis `axis` (0, 1, 2): the slice axis varies, the others get fixed odd shapes.
fn cases(axis: usize) -> Vec<Case> {
    let mut out = Vec::new();
    let base_size = [3, 4, 5];
    let base_min = [-7, 9, -13];
    // The slice-axis size and minimum: (size, min) pairs around the coordinate.
    let along = [
        (1, COORDINATE),
        (1, COORDINATE + 1),
        (1, COORDINATE - 1),
        (1, -COORDINATE),
        (2, COORDINATE),
        (3, COORDINATE),
        (4, COORDINATE - 2),
        (5, COORDINATE - 3),
        (3, COORDINATE + 1),
        (6, -11),
    ];
    for (size, min) in along {
        for step in [STEP, [1, 1, 1], [4, 2, 3]] {
            let mut case = Case {
                size: base_size,
                min: base_min,
                step,
            };
            case.size[axis] = size;
            case.min[axis] = min;
            out.push(case);
        }
    }
    // Other axes at size 1 as well, and a unit cube.
    let mut flat = Case {
        size: [1, 1, 1],
        min: base_min,
        step: STEP,
    };
    flat.min[axis] = COORDINATE;
    out.push(flat);
    out.push(Case {
        size: [1, 1, 1],
        min: [COORDINATE, COORDINATE, COORDINATE],
        step: STEP,
    });
    let mut thin = Case {
        size: [2, 1, 3],
        min: [COORDINATE, COORDINATE, COORDINATE],
        step: STEP,
    };
    thin.size[axis] = 1;
    out.push(thin);
    out
}

fn fnv(buffer: &[f32]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for value in buffer {
        for byte in value.to_bits().to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}

fn run_case(
    slice: &Slice,
    stack: &mut [ChunkNoiseFunctionComponent],
    volume: &DensityVolume,
) -> (u64, bool) {
    let mut buffer = vec![-12_345.0f32; volume.size()];
    slice.sample_volume(stack, &mut buffer, volume);
    let mut expected = vec![0.0f32; volume.size()];
    volume.fill_with(&mut expected, |pos: &Vector3<i32>| slice.sample(stack, pos));
    let same = buffer
        .iter()
        .zip(&expected)
        .all(|(a, b)| a.to_bits() == b.to_bits());
    (fnv(&buffer), same)
}

fn report() -> String {
    let inputs = inputs();
    let mut report = String::new();
    for (axis, axis_value) in [(0, Axis::X), (1, Axis::Y), (2, Axis::Z)] {
        for input_index in 0..INPUT_COUNT {
            let slice = Slice::new(input_index, axis_value, COORDINATE, -1.0e9, 1.0e9);
            let mut stack: Vec<ChunkNoiseFunctionComponent> = inputs
                .iter()
                .map(ChunkNoiseFunctionComponent::Independent)
                .collect();
            for (n, case) in cases(axis).iter().enumerate() {
                let volume = case.volume();
                let (hash, same) = run_case(&slice, &mut stack, &volume);
                let _ = writeln!(
                    report,
                    "{} in{input_index} #{n} size{:?} min{:?} step{:?} {hash:016x}{}",
                    axis_name(axis_value),
                    case.size,
                    case.min,
                    case.step,
                    if same { "" } else { " MISMATCH" },
                );
            }
        }
    }
    report
}

#[test]
fn slice_sample_volume() -> Result<(), Box<dyn std::error::Error>> {
    let report = report();
    assert!(
        !report.contains("MISMATCH"),
        "sample_volume differs from per-point sample:\n{report}"
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("slice_sample_volume.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::write(&path, &report)?;
        return Ok(());
    }
    assert_eq!(std::fs::read_to_string(&path)?, report);
    Ok(())
}
