//! Turning the base components of a noise router into the components it runs.
//!
//! The base components are walked in order and each one is converted with the stack built so far,
//! because the range of a component comes from the ranges of the components it reads
//! (`component_ranges`). A component that reads nothing before it (a constant, a noise) is
//! built from the random source alone.

use pumpkin_data::{
    chunk::DoublePerlinNoiseParameters,
    noise_router::{
        Axis, BaseNoiseFunctionComponent as Base, BinaryData, FindTopSurfaceData,
        InterpolatedNoiseSamplerData, LinearData, NoiseData, RangeChoiceData, RoundingData,
        ShiftedNoiseData, SplineRepr, UnaryData, WrapperType,
    },
};
use pumpkin_util::random::legacy_rand::LegacyRand;

use super::{
    DependentProtoNoiseFunctionComponent as Dependent, DoublePerlinNoiseBuilder,
    IndependentProtoNoiseFunctionComponent as Independent, ProtoNoiseFunctionComponent,
    build_spline_recursive,
    component_ranges::{self, Range},
};
use crate::{
    GlobalRandomConfig,
    generation::noise::{
        perlin::DoublePerlinNoiseSampler, router::find_top_surface::FindTopSurface,
    },
};

use crate::generation::noise::router::density_function::{
    NoiseFunctionComponentRange, PassThrough, Wrapper,
    beardifier::Beardifier,
    math::{Binary, Clamp, Constant, Lerp, Linear, Rounding, Unary},
    misc::{
        ClampedYGradient, DistanceToPoint, EndIsland, Gradient, IntervalSelect, RangeChoice, Slice,
    },
    noise::{InterpolatedNoiseSampler, Noise, ShiftA, ShiftB, ShiftedNoise},
    spline::{SplineFunction, SplineValue},
};

/// The components a stack of base components becomes, in the same order.
pub(super) fn build(
    base_stack: &[Base],
    random_config: &GlobalRandomConfig,
) -> Box<[ProtoNoiseFunctionComponent]> {
    // Contiguous memory for our function components
    let mut stack = Vec::<ProtoNoiseFunctionComponent>::with_capacity(base_stack.len());

    for component in base_stack {
        let converted = convert(component, &stack, random_config);
        stack.push(converted);
    }

    stack.into()
}

/// The component a base component becomes, given the components before it.
fn convert(
    component: &Base,
    stack: &[ProtoNoiseFunctionComponent],
    config: &GlobalRandomConfig,
) -> ProtoNoiseFunctionComponent {
    match component {
        Base::Spline { spline } => spline_component(spline, stack),
        Base::FindTopSurface {
            density_index,
            upper_bound_index,
            data,
        } => find_top_surface(*density_index, *upper_bound_index, data, stack),
        Base::EndIslands => independent(Independent::EndIsland(EndIsland::new(config.seed))),
        Base::Noise { data } => noise(data, config),
        Base::ShiftA { noise_id } => independent(Independent::ShiftA(ShiftA::new(plain_sampler(
            noise_id, config,
        )))),
        Base::ShiftB { noise_id } => independent(Independent::ShiftB(ShiftB::new(plain_sampler(
            noise_id, config,
        )))),
        Base::BlendDensity { input_index } => blend_density(*input_index, stack),
        Base::BlendAlpha => independent(Independent::Constant(Constant::new(1.0))),
        Base::BlendOffset => independent(Independent::Constant(Constant::new(0.0))),
        Base::Beardifier => {
            ProtoNoiseFunctionComponent::Beardifier(Beardifier::new(Vec::new(), Vec::new(), None))
        }
        Base::ShiftedNoise {
            shift_x_index,
            shift_y_index,
            shift_z_index,
            data,
        } => {
            let shifts = [*shift_x_index, *shift_y_index, *shift_z_index];
            shifted_noise(shifts, data, config)
        }
        Base::RangeChoice {
            input_index,
            when_in_range_index,
            when_out_range_index,
            data,
        } => {
            let branches = [*when_in_range_index, *when_out_range_index];
            range_choice(*input_index, branches, data, stack)
        }
        Base::Binary {
            argument1_index,
            argument2_index,
            data,
        } => binary(*argument1_index, *argument2_index, data, stack),
        Base::ClampedYGradient { data } => {
            independent(Independent::ClampedYGradient(ClampedYGradient::new(data)))
        }
        Base::Gradient { data } => independent(Independent::Gradient(Gradient::new(data))),
        Base::DistanceToPoint { data } => {
            independent(Independent::DistanceToPoint(DistanceToPoint::new(data)))
        }
        Base::Lerp {
            alpha_index,
            first_index,
            second_index,
        } => lerp(*alpha_index, *first_index, *second_index, stack),
        Base::Rounding {
            input_index,
            multiple_index,
            data,
        } => rounding(*input_index, *multiple_index, data, stack),
        Base::Slice {
            input_index,
            axis,
            coordinate,
        } => slice(*input_index, *axis, *coordinate, stack),
        Base::Constant { value } => independent(Independent::Constant(Constant::new(*value))),
        Base::Wrapper {
            input_index,
            wrapper,
        } => wrapper_of(*input_index, *wrapper, stack),
        Base::Linear { input_index, data } => linear(*input_index, data, stack),
        Base::Clamp { input_index, data } => {
            dependent(Dependent::Clamp(Clamp::new(*input_index, data)))
        }
        Base::Unary { input_index, data } => unary(*input_index, data, stack),
        Base::IntervalSelect {
            input_index,
            thresholds,
            functions_indices,
        } => interval_select(*input_index, thresholds, functions_indices, stack),
        Base::InterpolatedNoiseSampler { data } => interpolated_noise(data, config),
    }
}

const fn independent(component: Independent) -> ProtoNoiseFunctionComponent {
    ProtoNoiseFunctionComponent::Independent(component)
}

const fn dependent(component: Dependent) -> ProtoNoiseFunctionComponent {
    ProtoNoiseFunctionComponent::Dependent(component)
}

/// The range of the component at `index` in the stack built so far.
fn range_at(stack: &[ProtoNoiseFunctionComponent], index: usize) -> Range {
    (stack[index].min(), stack[index].max())
}

#[expect(clippy::unreachable)]
fn spline_component(
    spline: &SplineRepr,
    stack: &[ProtoNoiseFunctionComponent],
) -> ProtoNoiseFunctionComponent {
    let spline = match build_spline_recursive(spline) {
        SplineValue::Spline(spline) => spline,
        SplineValue::Fixed(_) => unreachable!(),
    };

    dependent(Dependent::Spline(SplineFunction::new(spline, stack)))
}

fn find_top_surface(
    density_index: usize,
    upper_bound_index: usize,
    data: &'static FindTopSurfaceData,
    stack: &[ProtoNoiseFunctionComponent],
) -> ProtoNoiseFunctionComponent {
    let min_value = data.lower_bound as f32;
    let max_value = stack[upper_bound_index].max().max(min_value);

    dependent(Dependent::FindTopSurface(FindTopSurface::new(
        density_index,
        upper_bound_index,
        min_value,
        max_value,
        data,
    )))
}

fn noise(data: &'static NoiseData, config: &GlobalRandomConfig) -> ProtoNoiseFunctionComponent {
    independent(Independent::Noise(Noise::new(
        noise_sampler(&data.noise_id, config),
        data,
    )))
}

fn shifted_noise(
    shifts: [usize; 3],
    data: &'static ShiftedNoiseData,
    config: &GlobalRandomConfig,
) -> ProtoNoiseFunctionComponent {
    let [shift_x_index, shift_y_index, shift_z_index] = shifts;
    dependent(Dependent::ShiftedNoise(ShiftedNoise::new(
        shift_x_index,
        shift_y_index,
        shift_z_index,
        noise_sampler(&data.noise_id, config),
        data,
    )))
}

fn blend_density(
    input_index: usize,
    stack: &[ProtoNoiseFunctionComponent],
) -> ProtoNoiseFunctionComponent {
    let (min_value, max_value) = range_at(stack, input_index);

    ProtoNoiseFunctionComponent::PassThrough(PassThrough::new(input_index, min_value, max_value))
}

fn range_choice(
    input_index: usize,
    [when_in_range_index, when_out_range_index]: [usize; 2],
    data: &'static RangeChoiceData,
    stack: &[ProtoNoiseFunctionComponent],
) -> ProtoNoiseFunctionComponent {
    let (min_value, max_value) = component_ranges::union(
        range_at(stack, when_in_range_index),
        range_at(stack, when_out_range_index),
    );

    dependent(Dependent::RangeChoice(RangeChoice::new(
        input_index,
        when_in_range_index,
        when_out_range_index,
        min_value,
        max_value,
        data,
    )))
}

fn binary(
    first_index: usize,
    second_index: usize,
    data: &'static BinaryData,
    stack: &[ProtoNoiseFunctionComponent],
) -> ProtoNoiseFunctionComponent {
    let (min, max) = component_ranges::binary(
        data.operation,
        range_at(stack, first_index),
        range_at(stack, second_index),
    );

    dependent(Dependent::Binary(Binary::new(
        first_index,
        second_index,
        min,
        max,
        data,
    )))
}

fn lerp(
    alpha_index: usize,
    first_index: usize,
    second_index: usize,
    stack: &[ProtoNoiseFunctionComponent],
) -> ProtoNoiseFunctionComponent {
    let (min_value, max_value) =
        component_ranges::union(range_at(stack, first_index), range_at(stack, second_index));

    dependent(Dependent::Lerp(Lerp::new(
        alpha_index,
        first_index,
        second_index,
        min_value,
        max_value,
    )))
}

fn rounding(
    input_index: usize,
    multiple_index: usize,
    data: &'static RoundingData,
    stack: &[ProtoNoiseFunctionComponent],
) -> ProtoNoiseFunctionComponent {
    let (min_value, max_value) = range_at(stack, input_index);

    dependent(Dependent::Rounding(Rounding::new(
        input_index,
        multiple_index,
        min_value,
        max_value,
        data,
    )))
}

fn slice(
    input_index: usize,
    axis: Axis,
    coordinate: i32,
    stack: &[ProtoNoiseFunctionComponent],
) -> ProtoNoiseFunctionComponent {
    let (min_value, max_value) = range_at(stack, input_index);

    dependent(Dependent::Slice(Slice::new(
        input_index,
        axis,
        coordinate,
        min_value,
        max_value,
    )))
}

fn wrapper_of(
    input_index: usize,
    wrapper: WrapperType,
    stack: &[ProtoNoiseFunctionComponent],
) -> ProtoNoiseFunctionComponent {
    let (min_value, max_value) = range_at(stack, input_index);

    ProtoNoiseFunctionComponent::Wrapper(Wrapper::new(input_index, wrapper, min_value, max_value))
}

fn linear(
    input_index: usize,
    data: &'static LinearData,
    stack: &[ProtoNoiseFunctionComponent],
) -> ProtoNoiseFunctionComponent {
    let (min, max) = component_ranges::linear(data, range_at(stack, input_index));

    dependent(Dependent::Linear(Linear::new(input_index, min, max, data)))
}

fn unary(
    input_index: usize,
    data: &'static UnaryData,
    stack: &[ProtoNoiseFunctionComponent],
) -> ProtoNoiseFunctionComponent {
    let (min_value, max_value) = component_ranges::unary(data, range_at(stack, input_index));

    dependent(Dependent::Unary(Unary::new(
        input_index,
        min_value,
        max_value,
        data,
    )))
}

fn interval_select(
    input_index: usize,
    thresholds: &'static [f32],
    functions_indices: &'static [usize],
    stack: &[ProtoNoiseFunctionComponent],
) -> ProtoNoiseFunctionComponent {
    let (min_value, max_value) = component_ranges::covering(
        functions_indices
            .iter()
            .map(|&index| range_at(stack, index)),
    );

    dependent(Dependent::IntervalSelect(IntervalSelect::new(
        input_index,
        thresholds,
        functions_indices,
        min_value,
        max_value,
    )))
}

fn interpolated_noise(
    data: &'static InterpolatedNoiseSamplerData,
    config: &GlobalRandomConfig,
) -> ProtoNoiseFunctionComponent {
    let sampler = if config.legacy_random_source {
        let mut legacy_rand = LegacyRand::from_seed(config.seed.wrapping_add(0));
        InterpolatedNoiseSampler::new(data, &mut legacy_rand)
    } else {
        let mut random_generator = config.base_random_deriver.split_string("minecraft:terrain");
        InterpolatedNoiseSampler::new(data, &mut random_generator)
    };

    independent(Independent::InterpolatedNoise(sampler))
}

/// The sampler of a noise. The two nether noises are seeded from the world seed directly through
/// the legacy generator, whatever the random source; any other noise comes from the random
/// source of the world.
fn noise_sampler(
    noise_id: &DoublePerlinNoiseParameters,
    config: &GlobalRandomConfig,
) -> DoublePerlinNoiseSampler {
    if noise_id.id == DoublePerlinNoiseParameters::NETHER_TEMPERATURE.id {
        let mut legacy_rand = LegacyRand::from_seed(config.seed.wrapping_add(0));
        DoublePerlinNoiseSampler::from_params(&mut legacy_rand, noise_id, true)
    } else if noise_id.id == DoublePerlinNoiseParameters::NETHER_VEGETATION.id {
        let mut legacy_rand = LegacyRand::from_seed(config.seed.wrapping_add(1));
        DoublePerlinNoiseSampler::from_params(&mut legacy_rand, noise_id, true)
    } else {
        plain_sampler(noise_id, config)
    }
}

/// The sampler of a noise from the random source of the world.
fn plain_sampler(
    noise_id: &DoublePerlinNoiseParameters,
    config: &GlobalRandomConfig,
) -> DoublePerlinNoiseSampler {
    DoublePerlinNoiseBuilder::get_noise_sampler(
        &config.base_random_deriver,
        config.legacy_random_source,
        config.seed,
        noise_id,
    )
}
