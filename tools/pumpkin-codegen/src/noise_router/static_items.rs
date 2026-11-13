//! The fixed part of the generated file: types and traits that do not depend on the data.

use proc_macro2::TokenStream;
use quote::quote;

/// The evaluation context trait the compiled functions read noise and blending from.
pub(super) fn noise_evaluation_context_tokens() -> TokenStream {
    quote! {
        use crate::chunk::DoublePerlinNoiseParameters;

        pub trait NoiseEvaluationContext {
            fn sample_noise(&mut self, noise_id: DoublePerlinNoiseParameters, x: f64, y: f64, z: f64) -> f32;
            fn sample_shift_a(&mut self, noise_id: DoublePerlinNoiseParameters, pos: &pumpkin_util::math::vector3::Vector3<i32>) -> f32;
            fn sample_shift_b(&mut self, noise_id: DoublePerlinNoiseParameters, pos: &pumpkin_util::math::vector3::Vector3<i32>) -> f32;
            fn sample_shifted_noise(&mut self, noise_id: DoublePerlinNoiseParameters, shift_x: f32, shift_y: f32, shift_z: f32, xz_scale: f64, y_scale: f64) -> f32;
            fn sample_interpolated_noise(&mut self, pos: &pumpkin_util::math::vector3::Vector3<i32>) -> f32;
            fn sample_beardifier(&mut self, pos: &pumpkin_util::math::vector3::Vector3<i32>) -> f32;
            fn sample_blend_alpha(&mut self, pos: &pumpkin_util::math::vector3::Vector3<i32>) -> f32;
            fn sample_blend_offset(&mut self, pos: &pumpkin_util::math::vector3::Vector3<i32>) -> f32;
            fn sample_blend_density(&mut self, input_val: f32, pos: &pumpkin_util::math::vector3::Vector3<i32>) -> f32;
            fn sample_end_islands(&mut self, pos: &pumpkin_util::math::vector3::Vector3<i32>) -> f32;
            fn sample_wrapper(&mut self, wrapper_index: usize, wrapper_type: WrapperType, pos: &pumpkin_util::math::vector3::Vector3<i32>, eval_input: &dyn Fn(&pumpkin_util::math::vector3::Vector3<i32>, &mut Self) -> f32) -> f32;
            fn sample_spline(&mut self, spline_index: usize, location_value: f32, pos: &pumpkin_util::math::vector3::Vector3<i32>) -> f32;
            fn sample_find_top_surface(&mut self, density_fn: &dyn Fn(&pumpkin_util::math::vector3::Vector3<i32>, &mut Self) -> f32, upper_bound_fn: &dyn Fn(&pumpkin_util::math::vector3::Vector3<i32>, &mut Self) -> f32, lower_bound: i32, cell_height: i32, pos: &pumpkin_util::math::vector3::Vector3<i32>) -> f32;
        }
    }
}

/// Parameter structs and enums of the sampling density functions.
pub(super) fn sample_data_tokens() -> TokenStream {
    quote! {
        pub struct NoiseData {
            pub noise_id: DoublePerlinNoiseParameters,
            pub xz_scale: f64,
            pub y_scale: f64,
        }

        pub struct FindTopSurfaceData {
            pub lower_bound: i32,
            pub cell_height: i32,
        }

        pub struct ShiftedNoiseData {
            pub xz_scale: f64,
            pub y_scale: f64,
            pub noise_id: DoublePerlinNoiseParameters,
        }

        pub struct InterpolatedNoiseSamplerData {
            pub xz_scale: f64,
            pub y_scale: f64,
            pub xz_factor: f64,
            pub y_factor: f64,
            pub smear_scale_multiplier: f64,
        }

        pub struct ClampedYGradientData {
            pub from_y: f32,
            pub to_y: f32,
            pub from_value: f32,
            pub to_value: f32,
        }

        #[derive(Copy, Clone)]
        pub enum Axis {
            X,
            Y,
            Z,
        }

        #[derive(Copy, Clone)]
        pub enum Tiling {
            ClampToEdge,
            Repeat,
            MirroredRepeat,
        }

        pub struct GradientData {
            pub axis: Axis,
            pub tiling: Tiling,
            pub from_coordinate: i32,
            pub to_coordinate: i32,
            pub from_value: f32,
            pub to_value: f32,
        }

        #[derive(Copy, Clone)]
        pub enum DistanceMetric {
            Euclidean,
            EuclideanSquared,
            Manhattan,
            Chebyshev,
        }

        pub struct DistanceToPointData {
            pub point: [i32; 3],
            pub metric: DistanceMetric,
        }

        #[derive(Copy, Clone)]
        pub enum RoundingOperation {
            Floor,
            Round,
            Ceil,
            Truncate,
        }

        pub struct RoundingData {
            pub operation: RoundingOperation,
        }
    }
}

/// Binary and linear operations and their parameter structs.
pub(super) fn binary_and_linear_tokens() -> TokenStream {
    quote! {
        #[derive(Copy, Clone)]
        pub enum BinaryOperation {
            Add,
            Mul,
            Min,
            Max,
            Sub,
            Div,
            Pow,
        }

        pub struct BinaryData {
            pub operation: BinaryOperation,
        }

        impl BinaryData {
            #[inline]
            #[must_use]
            pub const fn apply_density(&self, a: f32, b: f32) -> f32 {
                match self.operation {
                    BinaryOperation::Add => a + b,
                    BinaryOperation::Mul => a * b,
                    BinaryOperation::Min => a.min(b),
                    BinaryOperation::Max => a.max(b),
                    BinaryOperation::Sub => a - b,
                    BinaryOperation::Div => if b == 0.0 { 0.0 } else { a / b },
                    BinaryOperation::Pow => a, // const evaluation fallback
                }
            }
        }

        #[derive(Copy, Clone)]
        pub enum LinearOperation {
            Add,
            Mul,
        }

        pub struct LinearData {
            pub operation: LinearOperation,
            pub argument: f32,
        }

        impl LinearData {
            #[inline]
            #[must_use]
            pub const fn apply_density(&self, density: f32) -> f32 {
                match self.operation {
                    LinearOperation::Add => density + self.argument,
                    LinearOperation::Mul => density * self.argument,
                }
            }
        }
    }
}

/// Unary, clamp and range-choice operations and their parameter structs.
pub(super) fn unary_clamp_and_range_tokens() -> TokenStream {
    quote! {
        #[derive(Copy, Clone)]
        pub enum UnaryOperation {
            Abs,
            Square,
            Cube,
            HalfNegative,
            QuarterNegative,
            Squeeze,
            Invert,
            Negate,
            Sqrt,
            Log,
            Sign,
        }

        pub struct UnaryData {
            pub operation: UnaryOperation,
        }

        impl UnaryData {
            #[inline]
            #[must_use]
            pub fn apply_density(&self, density: f32) -> f32 {
                match self.operation {
                    UnaryOperation::Abs => density.abs(),
                    UnaryOperation::Square => density * density,
                    UnaryOperation::Cube => density * density * density,
                    UnaryOperation::HalfNegative => {
                        if density > 0.0 {
                            density
                        } else {
                            density * 0.5
                        }
                    }
                    UnaryOperation::QuarterNegative => {
                        if density > 0.0 {
                            density
                        } else {
                            density * 0.25
                        }
                    }
                    UnaryOperation::Squeeze => {
                        let clamped = density.clamp(-1.0, 1.0);
                        clamped / 2.0 - clamped * clamped * clamped / 24.0
                    }
                    UnaryOperation::Invert => {
                        if density == 0.0 { f32::INFINITY } else { 1.0 / density }
                    }
                    UnaryOperation::Negate => -density,
                    UnaryOperation::Sqrt => density.sqrt(),
                    UnaryOperation::Log => density.ln(),
                    UnaryOperation::Sign => {
                        if density > 0.0 { 1.0 } else if density < 0.0 { -1.0 } else { 0.0 }
                    }
                }
            }
        }

        pub struct ClampData {
            pub min_value: f32,
            pub max_value: f32,
        }

        impl ClampData {
            #[inline]
            #[must_use]
            pub const fn apply_density(&self, density: f32) -> f32 {
                density.clamp(self.min_value, self.max_value)
            }
        }

        pub struct RangeChoiceData {
            pub min_inclusive: f32,
            pub max_exclusive: f32,
        }
    }
}

/// Spline and wrapper representations.
pub(super) fn spline_and_wrapper_tokens() -> TokenStream {
    quote! {
        pub struct SplinePoint {
            pub location: f32,
            pub value: &'static SplineRepr,
            pub derivative: f32,
        }

        pub enum SplineRepr {
            Standard {
                location_function_index: usize,
                points: &'static [SplinePoint],
            },
            Fixed { value: f32 },
        }

        #[derive(Copy, Clone, PartialEq, Eq)]
        pub enum WrapperType {
            Interpolated { cell_size_xz: i32, cell_size_y: i32 },
            Cache,
        }
    }
}

/// The component enum of the base noise router.
pub(super) fn base_component_tokens() -> TokenStream {
    quote! {
        pub enum BaseNoiseFunctionComponent {
            Beardifier,
            BlendAlpha,
            BlendOffset,
            BlendDensity {
                input_index: usize,
            },
            FindTopSurface {
                density_index: usize,
                upper_bound_index: usize,
                data: &'static FindTopSurfaceData,
            },
            EndIslands,
            Noise {
                data: &'static NoiseData,
            },
            ShiftA {
                noise_id: DoublePerlinNoiseParameters,
            },
            ShiftB {
                noise_id: DoublePerlinNoiseParameters,
            },
            ShiftedNoise {
                shift_x_index: usize,
                shift_y_index: usize,
                shift_z_index: usize,
                data: &'static ShiftedNoiseData,
            },
            InterpolatedNoiseSampler {
                data: &'static InterpolatedNoiseSamplerData,
            },
            IntervalSelect {
                input_index: usize,
                thresholds: &'static [f32],
                functions_indices: &'static [usize],
            },
            Wrapper {
                input_index: usize,
                wrapper: WrapperType,
            },
            Constant {
                value: f32,
            },
            ClampedYGradient {
                data: &'static ClampedYGradientData,
            },
            Gradient {
                data: &'static GradientData,
            },
            DistanceToPoint {
                data: &'static DistanceToPointData,
            },
            Lerp {
                alpha_index: usize,
                first_index: usize,
                second_index: usize,
            },
            Rounding {
                input_index: usize,
                multiple_index: usize,
                data: &'static RoundingData,
            },
            Slice {
                input_index: usize,
                axis: Axis,
                coordinate: i32,
            },
            Binary {
                argument1_index: usize,
                argument2_index: usize,
                data: &'static BinaryData,
            },
            Linear {
                input_index: usize,
                data: &'static LinearData,
            },
            Unary {
                input_index: usize,
                data: &'static UnaryData,
            },
            Clamp {
                input_index: usize,
                data: &'static ClampData,
            },
            RangeChoice {
                input_index: usize,
                when_in_range_index: usize,
                when_out_range_index: usize,
                data: &'static RangeChoiceData,
            },
            Spline {
                spline: &'static SplineRepr,
            },
        }
    }
}

/// The base noise router structs.
pub(super) fn base_router_struct_tokens() -> TokenStream {
    quote! {
        pub struct BaseNoiseRouter {
            pub full_component_stack: &'static [BaseNoiseFunctionComponent],
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

        pub struct BaseSurfaceEstimator {
            pub full_component_stack: &'static [BaseNoiseFunctionComponent],
        }

        pub struct BaseMultiNoiseRouter {
            pub full_component_stack: &'static [BaseNoiseFunctionComponent],
            pub temperature: usize,
            pub vegetation: usize,
            pub continents: usize,
            pub erosion: usize,
            pub depth: usize,
            pub ridges: usize,
        }

        pub struct BaseNoiseRouters {
            pub noise: BaseNoiseRouter,
            pub surface_estimator: BaseSurfaceEstimator,
            pub multi_noise: BaseMultiNoiseRouter,
        }
    }
}

/// Accessors of the base noise routers.
pub(super) fn base_router_impl_tokens() -> TokenStream {
    quote! {
        impl BaseNoiseRouters {
            #[must_use]
            pub fn from_name(name: &str) -> Option<&'static Self> {
                let name = name.strip_prefix("minecraft:").unwrap_or(name);
                match name {
                    "overworld" => Some(&OVERWORLD_BASE_NOISE_ROUTER),
                    "amplified" => Some(&AMPLIFIED_BASE_NOISE_ROUTER),
                    "large_biomes" => Some(&LARGE_BIOMES_BASE_NOISE_ROUTER),
                    "nether" => Some(&NETHER_BASE_NOISE_ROUTER),
                    "end" => Some(&END_BASE_NOISE_ROUTER),
                    _ => None,
                }
            }

            #[must_use]
            pub fn from_dimension(dimension: &crate::dimension::Dimension) -> &'static Self {
                if dimension == &crate::dimension::Dimension::OVERWORLD {
                    &OVERWORLD_BASE_NOISE_ROUTER
                } else if dimension == &crate::dimension::Dimension::THE_NETHER {
                    &NETHER_BASE_NOISE_ROUTER
                } else {
                    &END_BASE_NOISE_ROUTER
                }
            }
        }
    }
}
