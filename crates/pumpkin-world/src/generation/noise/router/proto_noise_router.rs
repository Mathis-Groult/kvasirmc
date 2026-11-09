use pumpkin_data::{
    chunk::DoublePerlinNoiseParameters,
    noise_router::{BaseNoiseFunctionComponent, BaseNoiseRouters, SplineRepr},
};
use pumpkin_util::random::xoroshiro128::XoroshiroSplitter;
use std::sync::Arc;

use crate::{
    GlobalRandomConfig,
    generation::noise::{
        perlin::DoublePerlinNoiseSampler, router::find_top_surface::FindTopSurface,
    },
};

use super::{
    chunk_noise_router::{ChunkNoiseFunctionComponent, StaticChunkNoiseFunctionComponentImpl},
    density_function::{
        NoiseFunctionComponentRange, PassThrough, StaticIndependentChunkNoiseFunctionComponentImpl,
        Wrapper,
        beardifier::Beardifier,
        math::{Binary, Clamp, Constant, Lerp, Linear, Rounding, Unary},
        misc::{
            ClampedYGradient, DistanceToPoint, EndIsland, Gradient, IntervalSelect, RangeChoice,
            Slice,
        },
        noise::{InterpolatedNoiseSampler, Noise, ShiftA, ShiftB, ShiftedNoise},
        spline::{Spline, SplineFunction, SplinePoint, SplineValue},
    },
    density_volume::DensityVolume,
};
use pumpkin_util::math::vector3::Vector3;

mod component_ranges;
mod proto_stack;

pub static BEARDIFIER_ZERO_CONSTANT: IndependentProtoNoiseFunctionComponent =
    IndependentProtoNoiseFunctionComponent::Constant(Constant::new(0.0));

pub enum IndependentProtoNoiseFunctionComponent {
    Constant(Constant),
    EndIsland(EndIsland),
    Noise(Noise),
    ShiftA(ShiftA),
    ShiftB(ShiftB),
    InterpolatedNoise(InterpolatedNoiseSampler),
    ClampedYGradient(ClampedYGradient),
    Gradient(Gradient),
    DistanceToPoint(DistanceToPoint),
}

impl NoiseFunctionComponentRange for IndependentProtoNoiseFunctionComponent {
    #[inline]
    fn min(&self) -> f32 {
        match self {
            Self::Constant(c) => c.min(),
            Self::EndIsland(e) => e.min(),
            Self::Noise(n) => n.min(),
            Self::ShiftA(s) => s.min(),
            Self::ShiftB(s) => s.min(),
            Self::InterpolatedNoise(i) => i.min(),
            Self::ClampedYGradient(c) => c.min(),
            Self::Gradient(g) => g.min(),
            Self::DistanceToPoint(d) => d.min(),
        }
    }

    #[inline]
    fn max(&self) -> f32 {
        match self {
            Self::Constant(c) => c.max(),
            Self::EndIsland(e) => e.max(),
            Self::Noise(n) => n.max(),
            Self::ShiftA(s) => s.max(),
            Self::ShiftB(s) => s.max(),
            Self::InterpolatedNoise(i) => i.max(),
            Self::ClampedYGradient(c) => c.max(),
            Self::Gradient(g) => g.max(),
            Self::DistanceToPoint(d) => d.max(),
        }
    }
}

impl StaticIndependentChunkNoiseFunctionComponentImpl for IndependentProtoNoiseFunctionComponent {
    #[inline]
    fn sample(&self, pos: &Vector3<i32>) -> f32 {
        match self {
            Self::Constant(c) => c.sample(pos),
            Self::EndIsland(e) => e.sample(pos),
            Self::Noise(n) => n.sample(pos),
            Self::ShiftA(s) => s.sample(pos),
            Self::ShiftB(s) => s.sample(pos),
            Self::InterpolatedNoise(i) => i.sample(pos),
            Self::ClampedYGradient(c) => c.sample(pos),
            Self::Gradient(g) => g.sample(pos),
            Self::DistanceToPoint(d) => d.sample(pos),
        }
    }

    #[inline]
    fn sample_volume(&self, buffer: &mut [f32], volume: &DensityVolume) {
        match self {
            Self::Constant(c) => c.sample_volume(buffer, volume),
            Self::EndIsland(e) => e.sample_volume(buffer, volume),
            Self::Noise(n) => n.sample_volume(buffer, volume),
            Self::ShiftA(s) => s.sample_volume(buffer, volume),
            Self::ShiftB(s) => s.sample_volume(buffer, volume),
            Self::InterpolatedNoise(i) => i.sample_volume(buffer, volume),
            Self::ClampedYGradient(c) => c.sample_volume(buffer, volume),
            Self::Gradient(g) => g.sample_volume(buffer, volume),
            Self::DistanceToPoint(d) => d.sample_volume(buffer, volume),
        }
    }
}

pub enum DependentProtoNoiseFunctionComponent {
    Linear(Linear),
    Unary(Unary),
    Binary(Binary),
    ShiftedNoise(ShiftedNoise),
    IntervalSelect(IntervalSelect),
    FindTopSurface(FindTopSurface),
    Clamp(Clamp),
    RangeChoice(RangeChoice),
    Spline(SplineFunction),
    Lerp(Lerp),
    Rounding(Rounding),
    Slice(Slice),
}

impl NoiseFunctionComponentRange for DependentProtoNoiseFunctionComponent {
    #[inline]
    fn min(&self) -> f32 {
        match self {
            Self::Linear(l) => l.min(),
            Self::Unary(u) => u.min(),
            Self::Binary(b) => b.min(),
            Self::ShiftedNoise(s) => s.min(),
            Self::IntervalSelect(i) => i.min(),
            Self::FindTopSurface(f) => f.min(),
            Self::Clamp(c) => c.min(),
            Self::RangeChoice(r) => r.min(),
            Self::Spline(s) => s.min(),
            Self::Lerp(l) => l.min(),
            Self::Rounding(r) => r.min(),
            Self::Slice(s) => s.min(),
        }
    }

    #[inline]
    fn max(&self) -> f32 {
        match self {
            Self::Linear(l) => l.max(),
            Self::Unary(u) => u.max(),
            Self::Binary(b) => b.max(),
            Self::ShiftedNoise(s) => s.max(),
            Self::IntervalSelect(i) => i.max(),
            Self::FindTopSurface(f) => f.max(),
            Self::Clamp(c) => c.max(),
            Self::RangeChoice(r) => r.max(),
            Self::Spline(s) => s.max(),
            Self::Lerp(l) => l.max(),
            Self::Rounding(r) => r.max(),
            Self::Slice(s) => s.max(),
        }
    }
}

impl StaticChunkNoiseFunctionComponentImpl for DependentProtoNoiseFunctionComponent {
    #[inline]
    fn sample(
        &self,
        component_stack: &mut [ChunkNoiseFunctionComponent],
        pos: &Vector3<i32>,
    ) -> f32 {
        match self {
            Self::Linear(l) => l.sample(component_stack, pos),
            Self::Unary(u) => u.sample(component_stack, pos),
            Self::Binary(b) => b.sample(component_stack, pos),
            Self::ShiftedNoise(s) => s.sample(component_stack, pos),
            Self::IntervalSelect(i) => i.sample(component_stack, pos),
            Self::FindTopSurface(f) => f.sample(component_stack, pos),
            Self::Clamp(c) => c.sample(component_stack, pos),
            Self::RangeChoice(r) => r.sample(component_stack, pos),
            Self::Spline(s) => s.sample(component_stack, pos),
            Self::Lerp(l) => l.sample(component_stack, pos),
            Self::Rounding(r) => r.sample(component_stack, pos),
            Self::Slice(s) => s.sample(component_stack, pos),
        }
    }

    #[inline]
    fn sample_volume(
        &self,
        component_stack: &mut [ChunkNoiseFunctionComponent],
        buffer: &mut [f32],
        volume: &DensityVolume,
    ) {
        match self {
            Self::Linear(l) => l.sample_volume(component_stack, buffer, volume),
            Self::Unary(u) => u.sample_volume(component_stack, buffer, volume),
            Self::Binary(b) => b.sample_volume(component_stack, buffer, volume),
            Self::ShiftedNoise(s) => {
                s.sample_volume(component_stack, buffer, volume);
            }
            Self::IntervalSelect(i) => {
                i.sample_volume(component_stack, buffer, volume);
            }
            Self::FindTopSurface(f) => {
                f.sample_volume(component_stack, buffer, volume);
            }
            Self::Clamp(c) => c.sample_volume(component_stack, buffer, volume),
            Self::RangeChoice(r) => {
                r.sample_volume(component_stack, buffer, volume);
            }
            Self::Spline(s) => s.sample_volume(component_stack, buffer, volume),
            Self::Lerp(l) => l.sample_volume(component_stack, buffer, volume),
            Self::Rounding(r) => r.sample_volume(component_stack, buffer, volume),
            Self::Slice(s) => s.sample_volume(component_stack, buffer, volume),
        }
    }
}

pub enum ProtoNoiseFunctionComponent {
    Independent(IndependentProtoNoiseFunctionComponent),
    Dependent(DependentProtoNoiseFunctionComponent),
    Wrapper(Wrapper),
    Beardifier(Beardifier),
    PassThrough(PassThrough),
}

impl NoiseFunctionComponentRange for ProtoNoiseFunctionComponent {
    #[inline]
    fn min(&self) -> f32 {
        match self {
            Self::Independent(independent) => independent.min(),
            Self::Dependent(dependent) => dependent.min(),
            Self::Wrapper(wrapper) => wrapper.min(),
            Self::Beardifier(beardifier) => beardifier.min(),
            Self::PassThrough(pass_through) => pass_through.min(),
        }
    }

    #[inline]
    fn max(&self) -> f32 {
        match self {
            Self::Independent(independent) => independent.max(),
            Self::Dependent(dependent) => dependent.max(),
            Self::Wrapper(wrapper) => wrapper.max(),
            Self::Beardifier(beardifier) => beardifier.max(),
            Self::PassThrough(pass_through) => pass_through.max(),
        }
    }
}

pub struct ProtoNoiseRouter {
    pub full_component_stack: Box<[ProtoNoiseFunctionComponent]>,
    pub barrier_noise: usize,
    pub fluid_level_floodedness_noise: usize,
    pub fluid_level_spread_noise: usize,
    pub lava_noise: usize,
    pub erosion: usize,
    pub depth: usize,
    pub final_density: usize,
    pub vein_toggle: usize,
    pub vein_ridged: usize,
    pub vein_gap: usize,
}

pub struct ProtoSurfaceEstimator {
    pub full_component_stack: Box<[ProtoNoiseFunctionComponent]>,
}

pub struct ProtoMultiNoiseRouter {
    pub full_component_stack: Box<[ProtoNoiseFunctionComponent]>,
    pub temperature: usize,
    pub vegetation: usize,
    pub continents: usize,
    pub erosion: usize,
    pub depth: usize,
    pub ridges: usize,
}

pub struct ProtoNoiseRouters {
    pub noise: ProtoNoiseRouter,
    pub surface_estimator: ProtoSurfaceEstimator,
    /// Shared with the background stronghold ring search.
    pub multi_noise: Arc<ProtoMultiNoiseRouter>,
}

pub struct DoublePerlinNoiseBuilder;

impl DoublePerlinNoiseBuilder {
    #[must_use]
    pub fn get_noise_sampler_for_id(
        base_random_deriver: &XoroshiroSplitter,
        parameters: &DoublePerlinNoiseParameters,
    ) -> DoublePerlinNoiseSampler {
        let mut random = base_random_deriver.from_lo_and_hi(parameters.lo, parameters.hi);
        DoublePerlinNoiseSampler::from_params(&mut random, parameters, false)
    }

    #[must_use]
    pub fn get_noise_sampler(
        base_random_deriver: &XoroshiroSplitter,
        legacy_random_source: bool,
        world_seed: u64,
        parameters: &DoublePerlinNoiseParameters,
    ) -> DoublePerlinNoiseSampler {
        if legacy_random_source {
            use pumpkin_util::random::RandomImpl;
            use pumpkin_util::random::legacy_rand::LegacyRand;
            let mut root = LegacyRand::from_seed(world_seed);
            let factory_seed = root.next_i64() as u64;
            let mut noise_rand =
                LegacyRand::from_seed((parameters.string_hash as i64 ^ factory_seed as i64) as u64);
            DoublePerlinNoiseSampler::from_params(&mut noise_rand, parameters, false)
        } else {
            Self::get_noise_sampler_for_id(base_random_deriver, parameters)
        }
    }
}

fn build_spline_recursive(spline: &SplineRepr) -> SplineValue {
    match spline {
        SplineRepr::Fixed { value } => SplineValue::Fixed(*value),
        SplineRepr::Standard {
            location_function_index,
            points,
        } => {
            let mut points_vec = Vec::with_capacity(points.len());

            for point in *points {
                let value = build_spline_recursive(point.value);
                points_vec.push(SplinePoint::new(point.location, value, point.derivative));
            }

            SplineValue::Spline(Spline::new(*location_function_index, points_vec.into()))
        }
    }
}

impl ProtoNoiseRouters {
    /// Converts a stack of base components into the components a router runs.
    #[must_use]
    pub fn generate_proto_stack(
        base_stack: &[BaseNoiseFunctionComponent],
        random_config: &GlobalRandomConfig,
    ) -> Box<[ProtoNoiseFunctionComponent]> {
        proto_stack::build(base_stack, random_config)
    }

    #[must_use]
    pub fn generate(base: &BaseNoiseRouters, random_config: &GlobalRandomConfig) -> Self {
        let noise_stack =
            Self::generate_proto_stack(base.noise.full_component_stack, random_config);
        let surface_stack =
            Self::generate_proto_stack(base.surface_estimator.full_component_stack, random_config);
        let multi_noise_stack =
            Self::generate_proto_stack(base.multi_noise.full_component_stack, random_config);

        Self {
            noise: ProtoNoiseRouter {
                full_component_stack: noise_stack,
                barrier_noise: base.noise.barrier_noise,
                fluid_level_floodedness_noise: base.noise.fluid_level_floodedness_noise,
                fluid_level_spread_noise: base.noise.fluid_level_spread_noise,
                lava_noise: base.noise.lava_noise,
                erosion: base.noise.erosion,
                depth: base.noise.depth,
                final_density: base.noise.final_density,
                vein_toggle: base.noise.vein_toggle,
                vein_ridged: base.noise.vein_ridged,
                vein_gap: base.noise.vein_gap,
            },
            surface_estimator: ProtoSurfaceEstimator {
                full_component_stack: surface_stack,
            },
            multi_noise: Arc::new(ProtoMultiNoiseRouter {
                full_component_stack: multi_noise_stack,
                temperature: base.multi_noise.temperature,
                vegetation: base.multi_noise.vegetation,
                continents: base.multi_noise.continents,
                erosion: base.multi_noise.erosion,
                depth: base.multi_noise.depth,
                ridges: base.multi_noise.ridges,
            }),
        }
    }
}
