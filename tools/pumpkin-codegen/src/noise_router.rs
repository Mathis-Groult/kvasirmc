mod compiled_eval;
mod component_tokens;
mod simplify;
mod static_items;
mod vanilla;
use std::{
    collections::BTreeMap,
    fs,
    hash::{DefaultHasher, Hash, Hasher},
};

use heck::ToShoutySnakeCase;

use proc_macro2::{Punct, Spacing, Span, TokenStream};
use quote::{ToTokens, TokenStreamExt, quote};
use serde::Deserialize;
use static_items::{
    base_component_tokens, base_router_impl_tokens, base_router_struct_tokens,
    binary_and_linear_tokens, noise_evaluation_context_tokens, sample_data_tokens,
    spline_and_wrapper_tokens, unary_clamp_and_range_tokens,
};
use syn::Ident;
use vanilla::load_vanilla_noise_routers;

/// Wraps an `f32` to provide a bitwise-exact `Hash` implementation for use as a map key.
#[derive(Clone, Copy)]
struct HashableF32(pub f32);

impl Hash for HashableF32 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_le_bytes().hash(state);
    }
}

impl ToTokens for HashableF32 {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let value = self.0;
        if value.is_finite() {
            value.to_tokens(tokens);
        } else {
            tokens.append(Ident::new("f32", Span::call_site()));
            tokens.append(Punct::new(':', Spacing::Joint));
            tokens.append(Punct::new(':', Spacing::Joint));
            if value.is_nan() {
                tokens.append(Ident::new("NAN", Span::call_site()));
            } else if value > 0.0 {
                tokens.append(Ident::new("INFINITY", Span::call_site()));
            } else {
                tokens.append(Ident::new("NEG_INFINITY", Span::call_site()));
            }
        }
    }
}

impl<'de> Deserialize<'de> for HashableF32 {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        f32::deserialize(deserializer).map(Self)
    }
}

#[derive(Clone, Copy)]
struct HashableF64(pub f64);

impl Hash for HashableF64 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_le_bytes().hash(state);
    }
}

impl ToTokens for HashableF64 {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        self.0.to_tokens(tokens);
    }
}

impl<'de> Deserialize<'de> for HashableF64 {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        f64::deserialize(deserializer).map(Self)
    }
}

/// Deserialized representation of a cubic spline used inside density functions.
#[derive(Deserialize, Hash, Clone)]
#[serde(tag = "_type", content = "value")]
enum SplineRepr {
    #[serde(rename(deserialize = "standard"))]
    Standard {
        #[serde(rename(deserialize = "locationFunction"))]
        location_function: Box<DensityFunctionRepr>,
        locations: Box<[HashableF32]>,
        values: Box<[Self]>,
        derivatives: Box<[HashableF32]>,
    },
    #[serde(rename(deserialize = "fixed"))]
    Fixed { value: HashableF32 },
}

impl SplineRepr {
    fn get_token_stream(
        &self,
        stack: &mut Vec<TokenStream>,
        nodes: &mut Vec<DensityFunctionRepr>,
        hash_to_index_map: &mut BTreeMap<u64, usize>,
    ) -> TokenStream {
        match self {
            Self::Fixed { value } => {
                quote! {
                    SplineRepr::Fixed { value: #value }
                }
            }
            Self::Standard {
                location_function,
                locations,
                values,
                derivatives,
            } => {
                assert_eq!(values.len(), locations.len());
                assert_eq!(values.len(), derivatives.len());

                let points = locations
                    .into_iter()
                    .zip(values)
                    .zip(derivatives)
                    .map(|((location, value), derivative)| (location, value, derivative));

                let function_index =
                    location_function.get_index_for_component(stack, nodes, hash_to_index_map);

                let point_reprs = points
                    .into_iter()
                    .map(|(location, value, derivative)| {
                        let value_repr = value.get_token_stream(stack, nodes, hash_to_index_map);

                        quote! {
                            SplinePoint {
                                location: #location,
                                value: &#value_repr,
                                derivative: #derivative,
                            }
                        }
                    })
                    .collect::<Vec<_>>();

                quote! {
                    SplineRepr::Standard {
                        location_function_index: #function_index,
                        points: &[#(#point_reprs),*],
                    }
                }
            }
        }
    }
}

/// Arithmetic operation applied to two density function arguments.
#[derive(Deserialize, Hash, Copy, Clone)]
enum BinaryOperation {
    #[serde(rename(deserialize = "ADD"))]
    Add,
    #[serde(rename(deserialize = "MUL"))]
    Mul,
    #[serde(rename(deserialize = "MIN"))]
    Min,
    #[serde(rename(deserialize = "MAX"))]
    Max,
    Sub,
    Div,
    Pow,
}

impl BinaryOperation {
    fn get_token_stream(&self) -> TokenStream {
        match self {
            Self::Add => quote! { BinaryOperation::Add },
            Self::Mul => quote! { BinaryOperation::Mul },
            Self::Min => quote! { BinaryOperation::Min },
            Self::Max => quote! { BinaryOperation::Max },
            Self::Sub => quote! { BinaryOperation::Sub },
            Self::Div => quote! { BinaryOperation::Div },
            Self::Pow => quote! { BinaryOperation::Pow },
        }
    }
}

/// Arithmetic operation applied to a single density function argument and a scalar.
#[derive(Deserialize, Hash, Copy, Clone)]
enum LinearOperation {
    #[serde(rename(deserialize = "ADD"))]
    Add,
    #[serde(rename(deserialize = "MUL"))]
    Mul,
}

impl LinearOperation {
    fn into_token_stream(self) -> TokenStream {
        match self {
            Self::Add => quote! { LinearOperation::Add },
            Self::Mul => quote! { LinearOperation::Mul },
        }
    }
}

/// Single-argument transformation applied to a density value.
#[derive(Deserialize, Hash, Copy, Clone)]
enum UnaryOperation {
    #[serde(rename(deserialize = "INVERT"))]
    Invert,
    #[serde(rename(deserialize = "ABS"))]
    Abs,
    #[serde(rename(deserialize = "SQUARE"))]
    Square,
    #[serde(rename(deserialize = "CUBE"))]
    Cube,
    #[serde(rename(deserialize = "HALF_NEGATIVE"))]
    HalfNegative,
    #[serde(rename(deserialize = "QUARTER_NEGATIVE"))]
    QuarterNegative,
    #[serde(rename(deserialize = "SQUEEZE"))]
    Squeeze,
    Negate,
    Sqrt,
    Log,
    Sign,
}

impl UnaryOperation {
    fn into_token_stream(self) -> TokenStream {
        match self {
            Self::Invert => quote! { UnaryOperation::Invert },
            Self::Abs => quote! { UnaryOperation::Abs },
            Self::Square => quote! { UnaryOperation::Square },
            Self::Cube => quote! { UnaryOperation::Cube },
            Self::HalfNegative => quote! { UnaryOperation::HalfNegative },
            Self::QuarterNegative => quote! { UnaryOperation::QuarterNegative },
            Self::Squeeze => quote! { UnaryOperation::Squeeze },
            Self::Negate => quote! { UnaryOperation::Negate },
            Self::Sqrt => quote! { UnaryOperation::Sqrt },
            Self::Log => quote! { UnaryOperation::Log },
            Self::Sign => quote! { UnaryOperation::Sign },
        }
    }
}

/// Rounding operations.
#[derive(Deserialize, Hash, Copy, Clone)]
enum RoundingOperation {
    Floor,
    Round,
    Ceil,
    Truncate,
}

impl RoundingOperation {
    fn into_token_stream(self) -> TokenStream {
        match self {
            Self::Floor => quote! { RoundingOperation::Floor },
            Self::Round => quote! { RoundingOperation::Round },
            Self::Ceil => quote! { RoundingOperation::Ceil },
            Self::Truncate => quote! { RoundingOperation::Truncate },
        }
    }
}

/// Distance metric for distance_to_point.
#[derive(Deserialize, Hash, Copy, Clone)]
enum DistanceMetric {
    Euclidean,
    EuclideanSquared,
    Manhattan,
    Chebyshev,
}

impl DistanceMetric {
    fn into_token_stream(self) -> TokenStream {
        match self {
            Self::Euclidean => quote! { DistanceMetric::Euclidean },
            Self::EuclideanSquared => quote! { DistanceMetric::EuclideanSquared },
            Self::Manhattan => quote! { DistanceMetric::Manhattan },
            Self::Chebyshev => quote! { DistanceMetric::Chebyshev },
        }
    }
}

/// Coordinate axis.
#[derive(Deserialize, Hash, Copy, Clone)]
enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    fn into_token_stream(self) -> TokenStream {
        match self {
            Self::X => quote! { Axis::X },
            Self::Y => quote! { Axis::Y },
            Self::Z => quote! { Axis::Z },
        }
    }
}

/// Gradient tiling behavior.
#[derive(Deserialize, Hash, Copy, Clone)]
enum Tiling {
    ClampToEdge,
    Repeat,
    MirroredRepeat,
}

impl Tiling {
    fn into_token_stream(self) -> TokenStream {
        match self {
            Self::ClampToEdge => quote! { Tiling::ClampToEdge },
            Self::Repeat => quote! { Tiling::Repeat },
            Self::MirroredRepeat => quote! { Tiling::MirroredRepeat },
        }
    }
}

/// Caching or interpolation wrapper applied around an inner density function.
#[derive(Copy, Clone, Deserialize, PartialEq, Eq, Hash)]
enum WrapperType {
    Interpolated { cell_size_xz: i32, cell_size_y: i32 },
    Cache,
}

impl WrapperType {
    fn into_token_stream(self) -> TokenStream {
        match self {
            Self::Interpolated {
                cell_size_xz,
                cell_size_y,
            } => quote! {
                WrapperType::Interpolated {
                    cell_size_xz: #cell_size_xz,
                    cell_size_y: #cell_size_y,
                }
            },
            Self::Cache => quote! { WrapperType::Cache },
        }
    }
}

#[derive(Deserialize, Hash, Clone)]
struct NoiseData {
    #[serde(rename(deserialize = "noise"))]
    noise_id: String,
    #[serde(rename(deserialize = "xzScale"))]
    xz_scale: HashableF64,
    #[serde(rename(deserialize = "yScale"))]
    y_scale: HashableF64,
}

#[derive(Deserialize, Hash, Clone)]
struct ShiftedNoiseData {
    #[serde(rename(deserialize = "xzScale"))]
    xz_scale: HashableF64,
    #[serde(rename(deserialize = "yScale"))]
    y_scale: HashableF64,
    #[serde(rename(deserialize = "noise"))]
    noise_id: String,
}

#[derive(Deserialize, Hash, Clone)]
struct InterpolatedNoiseSamplerData {
    #[serde(rename(deserialize = "xzScale"))]
    xz_scale: HashableF64,
    #[serde(rename(deserialize = "yScale"))]
    y_scale: HashableF64,
    #[serde(rename(deserialize = "xzFactor"))]
    xz_factor: HashableF64,
    #[serde(rename(deserialize = "yFactor"))]
    y_factor: HashableF64,
    #[serde(rename(deserialize = "smearScaleMultiplier"))]
    smear_scale_multiplier: HashableF64,
}

#[derive(Deserialize, Hash, Clone)]
struct ClampedYGradientData {
    #[serde(rename(deserialize = "fromY"))]
    from_y: i32,
    #[serde(rename(deserialize = "toY"))]
    to_y: i32,
    #[serde(rename(deserialize = "fromValue"))]
    from_value: HashableF32,
    #[serde(rename(deserialize = "toValue"))]
    to_value: HashableF32,
}

#[derive(Deserialize, Hash, Clone)]
struct GradientData {
    axis: Axis,
    tiling: Tiling,
    from_coordinate: i32,
    to_coordinate: i32,
    from_value: HashableF32,
    to_value: HashableF32,
}

#[derive(Deserialize, Hash, Clone)]
struct DistanceToPointData {
    point: [i32; 3],
    metric: DistanceMetric,
}

#[derive(Deserialize, Hash, Clone)]
struct RoundingData {
    operation: RoundingOperation,
}

#[derive(Deserialize, Hash, Clone)]
struct BinaryData {
    #[serde(rename(deserialize = "type"))]
    operation: BinaryOperation,
    #[serde(rename(deserialize = "minValue"))]
    min_value: HashableF32,
    #[serde(rename(deserialize = "maxValue"))]
    max_value: HashableF32,
}

#[derive(Deserialize, Hash, Clone)]
struct LinearData {
    #[serde(rename(deserialize = "specificType"))]
    operation: LinearOperation,
    argument: HashableF32,
    #[serde(rename(deserialize = "minValue"))]
    min_value: HashableF32,
    #[serde(rename(deserialize = "maxValue"))]
    max_value: HashableF32,
}

#[derive(Deserialize, Hash, Clone)]
struct FindTopSurfaceData {
    #[serde(rename(deserialize = "lowerBound"))]
    lower_bound: i32,
    #[serde(rename(deserialize = "cellHeight"))]
    cell_height: i32,
}

#[derive(Deserialize, Hash, Clone)]
struct UnaryData {
    #[serde(rename(deserialize = "type"))]
    operation: UnaryOperation,
    #[serde(rename(deserialize = "minValue"))]
    min_value: HashableF32,
    #[serde(rename(deserialize = "maxValue"))]
    max_value: HashableF32,
}

#[derive(Deserialize, Hash, Clone)]
struct ClampData {
    #[serde(rename(deserialize = "minValue"))]
    min_value: HashableF32,
    #[serde(rename(deserialize = "maxValue"))]
    max_value: HashableF32,
}

#[derive(Deserialize, Hash, Clone)]
struct RangeChoiceData {
    #[serde(rename(deserialize = "minInclusive"))]
    min_inclusive: HashableF32,
    #[serde(rename(deserialize = "maxExclusive"))]
    max_exclusive: HashableF32,
}

#[derive(Deserialize, Hash, Clone)]
struct SplineData {
    #[serde(rename(deserialize = "minValue"))]
    min_value: HashableF32,
    #[serde(rename(deserialize = "maxValue"))]
    max_value: HashableF32,
}

/// Deserialized representation of any density function node in the noise router tree.
#[derive(Deserialize, Hash, Clone)]
#[serde(tag = "_class", content = "value")]
enum DensityFunctionRepr {
    Beardifier,
    BlendAlpha,
    BlendOffset,
    BlendDensity {
        input: Box<Self>,
    },
    FindTopSurface {
        density: Box<Self>,
        #[serde(rename(deserialize = "upperBound"))]
        upper_bound: Box<Self>,
        #[serde(flatten)]
        data: FindTopSurfaceData,
    },
    EndIslands,
    Noise {
        #[serde(flatten)]
        data: NoiseData,
    },
    ShiftA {
        #[serde(rename(deserialize = "offsetNoise"))]
        noise_id: String,
    },
    ShiftB {
        #[serde(rename(deserialize = "offsetNoise"))]
        noise_id: String,
    },
    ShiftedNoise {
        #[serde(rename(deserialize = "shiftX"))]
        shift_x: Box<Self>,
        #[serde(rename(deserialize = "shiftY"))]
        shift_y: Box<Self>,
        #[serde(rename(deserialize = "shiftZ"))]
        shift_z: Box<Self>,
        #[serde(flatten)]
        data: ShiftedNoiseData,
    },
    InterpolatedNoiseSampler {
        #[serde(flatten)]
        data: InterpolatedNoiseSamplerData,
    },
    #[serde(rename(deserialize = "IntervalSelect"))]
    IntervalSelect {
        input: Box<Self>,
        thresholds: Box<[HashableF32]>,
        functions: Box<[Self]>,
    },
    #[serde(rename(deserialize = "Wrapping"))]
    Wrapper {
        #[serde(rename(deserialize = "wrapped"))]
        input: Box<Self>,
        #[serde(rename(deserialize = "type"))]
        wrapper: WrapperType,
    },
    Constant {
        value: HashableF32,
    },
    #[serde(rename(deserialize = "YClampedGradient"))]
    ClampedYGradient {
        #[serde(flatten)]
        data: ClampedYGradientData,
    },
    Gradient {
        data: GradientData,
    },
    DistanceToPoint {
        data: DistanceToPointData,
    },
    Lerp {
        alpha: Box<Self>,
        first: Box<Self>,
        second: Box<Self>,
    },
    Rounding {
        input: Box<Self>,
        multiple: Box<Self>,
        data: RoundingData,
    },
    Slice {
        axis: Axis,
        coordinate: i32,
        input: Box<Self>,
    },
    #[serde(rename(deserialize = "BinaryOperation"))]
    Binary {
        argument1: Box<Self>,
        argument2: Box<Self>,
        #[serde(flatten)]
        data: BinaryData,
    },
    #[serde(rename(deserialize = "LinearOperation"))]
    Linear {
        input: Box<Self>,
        #[serde(flatten)]
        data: LinearData,
    },
    #[serde(rename(deserialize = "UnaryOperation"))]
    Unary {
        input: Box<Self>,
        #[serde(flatten)]
        data: UnaryData,
    },
    Clamp {
        input: Box<Self>,
        #[serde(flatten)]
        data: ClampData,
    },
    RangeChoice {
        input: Box<Self>,
        #[serde(rename(deserialize = "whenInRange"))]
        when_in_range: Box<Self>,
        #[serde(rename(deserialize = "whenOutOfRange"))]
        when_out_range: Box<Self>,
        #[serde(flatten)]
        data: RangeChoiceData,
    },
    Spline {
        spline: SplineRepr,
        #[serde(flatten)]
        data: SplineData,
    },
}

const AXIS_X: u8 = 1;
const AXIS_Y: u8 = 2;
const AXIS_Z: u8 = 4;
const AXES_XZ: u8 = AXIS_X | AXIS_Z;
const AXES_ALL: u8 = AXIS_X | AXIS_Y | AXIS_Z;

impl Axis {
    const fn as_axes(self) -> u8 {
        match self {
            Self::X => AXIS_X,
            Self::Y => AXIS_Y,
            Self::Z => AXIS_Z,
        }
    }
}

fn noise_domain_axes(xz_scale: f64, y_scale: f64) -> u8 {
    let mut axes = AXES_ALL;
    if y_scale == 0.0 {
        axes &= !AXIS_Y;
    }
    if xz_scale == 0.0 {
        axes &= !AXES_XZ;
    }
    axes
}

impl SplineRepr {
    fn for_each_function(&mut self, f: &mut dyn FnMut(&mut DensityFunctionRepr)) {
        if let Self::Standard {
            location_function,
            values,
            ..
        } = self
        {
            f(location_function);
            for value in values.iter_mut() {
                value.for_each_function(f);
            }
        }
    }

    fn domain_axes(&self) -> u8 {
        match self {
            Self::Fixed { .. } => 0,
            Self::Standard {
                location_function,
                values,
                ..
            } => values
                .iter()
                .fold(location_function.domain_axes(), |axes, value| {
                    axes | value.domain_axes()
                }),
        }
    }
}

/// Top-level container for all dimension noise router representations deserialized from JSON.
#[derive(Deserialize)]
struct NoiseRouterReprs {
    overworld: NoiseRouterRepr,
    #[serde(rename(deserialize = "large_biomes"))]
    overworld_large_biomes: NoiseRouterRepr,
    #[serde(rename(deserialize = "amplified"))]
    overworld_amplified: NoiseRouterRepr,
    nether: NoiseRouterRepr,
    end: NoiseRouterRepr,
    #[serde(rename(deserialize = "floating_islands"))]
    end_islands: NoiseRouterRepr,
}

/// Deserialized noise router for a single dimension, containing all density function roots.
#[derive(Deserialize)]
struct NoiseRouterRepr {
    #[serde(rename(deserialize = "barrierNoise"))]
    barrier_noise: DensityFunctionRepr,
    #[serde(rename(deserialize = "fluidLevelFloodednessNoise"))]
    fluid_level_floodedness_noise: DensityFunctionRepr,
    #[serde(rename(deserialize = "fluidLevelSpreadNoise"))]
    fluid_level_spread_noise: DensityFunctionRepr,
    #[serde(rename(deserialize = "lavaNoise"))]
    lava_noise: DensityFunctionRepr,
    temperature: DensityFunctionRepr,
    vegetation: DensityFunctionRepr,
    continents: DensityFunctionRepr,
    erosion: DensityFunctionRepr,
    depth: DensityFunctionRepr,
    ridges: DensityFunctionRepr,
    #[serde(rename(deserialize = "preliminarySurfaceLevel"))]
    preliminary_surface_level: DensityFunctionRepr,
    #[serde(rename(deserialize = "finalDensity"))]
    final_density: DensityFunctionRepr,
    #[serde(rename(deserialize = "veinToggle"))]
    vein_toggle: DensityFunctionRepr,
    #[serde(rename(deserialize = "veinRidged"))]
    vein_ridged: DensityFunctionRepr,
    #[serde(rename(deserialize = "veinGap"))]
    vein_gap: DensityFunctionRepr,
}

impl NoiseRouterRepr {
    fn slice_uniform_axes(&mut self) {
        self.barrier_noise.slice_uniform_axes(AXES_ALL);
        self.fluid_level_floodedness_noise
            .slice_uniform_axes(AXES_ALL);
        self.fluid_level_spread_noise.slice_uniform_axes(AXES_ALL);
        self.lava_noise.slice_uniform_axes(AXES_ALL);
        self.temperature.slice_uniform_axes(AXES_ALL);
        self.vegetation.slice_uniform_axes(AXES_ALL);
        self.continents.slice_uniform_axes(AXES_ALL);
        self.erosion.slice_uniform_axes(AXES_ALL);
        self.depth.slice_uniform_axes(AXES_ALL);
        self.ridges.slice_uniform_axes(AXES_ALL);
        self.preliminary_surface_level.slice_uniform_axes(AXES_ALL);
        self.final_density.slice_uniform_axes(AXES_ALL);
        self.vein_toggle.slice_uniform_axes(AXES_ALL);
        self.vein_ridged.slice_uniform_axes(AXES_ALL);
        self.vein_gap.slice_uniform_axes(AXES_ALL);
    }

    fn optimize(&mut self) {
        self.barrier_noise.optimize();
        self.fluid_level_floodedness_noise.optimize();
        self.fluid_level_spread_noise.optimize();
        self.lava_noise.optimize();
        self.temperature.optimize();
        self.vegetation.optimize();
        self.continents.optimize();
        self.erosion.optimize();
        self.depth.optimize();
        self.ridges.optimize();
        self.preliminary_surface_level.optimize();
        self.final_density.optimize();
        self.vein_toggle.optimize();
        self.vein_ridged.optimize();
        self.vein_gap.optimize();
    }

    fn into_token_stream_compiled(mut self, dim_name: &str) -> (TokenStream, TokenStream) {
        self.optimize();
        self.slice_uniform_axes();

        let mut noise_component_stack = Vec::new();
        let mut noise_nodes = Vec::new();
        let mut noise_lookup_map = BTreeMap::new();

        let final_density = self.final_density.get_index_for_component(
            &mut noise_component_stack,
            &mut noise_nodes,
            &mut noise_lookup_map,
        );
        let barrier_noise = self.barrier_noise.get_index_for_component(
            &mut noise_component_stack,
            &mut noise_nodes,
            &mut noise_lookup_map,
        );
        let fluid_level_floodedness_noise =
            self.fluid_level_floodedness_noise.get_index_for_component(
                &mut noise_component_stack,
                &mut noise_nodes,
                &mut noise_lookup_map,
            );
        let fluid_level_spread_noise = self.fluid_level_spread_noise.get_index_for_component(
            &mut noise_component_stack,
            &mut noise_nodes,
            &mut noise_lookup_map,
        );
        let lava_noise = self.lava_noise.get_index_for_component(
            &mut noise_component_stack,
            &mut noise_nodes,
            &mut noise_lookup_map,
        );
        let vein_toggle = self.vein_toggle.get_index_for_component(
            &mut noise_component_stack,
            &mut noise_nodes,
            &mut noise_lookup_map,
        );
        let vein_ridged = self.vein_ridged.get_index_for_component(
            &mut noise_component_stack,
            &mut noise_nodes,
            &mut noise_lookup_map,
        );
        let vein_gap = self.vein_gap.get_index_for_component(
            &mut noise_component_stack,
            &mut noise_nodes,
            &mut noise_lookup_map,
        );
        let noise_erosion = self.erosion.get_index_for_component(
            &mut noise_component_stack,
            &mut noise_nodes,
            &mut noise_lookup_map,
        );
        let noise_depth = self.depth.get_index_for_component(
            &mut noise_component_stack,
            &mut noise_nodes,
            &mut noise_lookup_map,
        );

        let mut surface_component_stack = Vec::new();
        let mut surface_nodes = Vec::new();
        let mut surface_lookup_map = BTreeMap::new();
        let _ = self.preliminary_surface_level.get_index_for_component(
            &mut surface_component_stack,
            &mut surface_nodes,
            &mut surface_lookup_map,
        );

        let mut multinoise_component_stack = Vec::new();
        let mut multinoise_nodes = Vec::new();
        let mut multinoise_lookup_map = BTreeMap::new();
        let ridges = self.ridges.get_index_for_component(
            &mut multinoise_component_stack,
            &mut multinoise_nodes,
            &mut multinoise_lookup_map,
        );
        let temperature = self.temperature.get_index_for_component(
            &mut multinoise_component_stack,
            &mut multinoise_nodes,
            &mut multinoise_lookup_map,
        );
        let vegetation = self.vegetation.get_index_for_component(
            &mut multinoise_component_stack,
            &mut multinoise_nodes,
            &mut multinoise_lookup_map,
        );
        let continents = self.continents.get_index_for_component(
            &mut multinoise_component_stack,
            &mut multinoise_nodes,
            &mut multinoise_lookup_map,
        );
        let multi_erosion = self.erosion.get_index_for_component(
            &mut multinoise_component_stack,
            &mut multinoise_nodes,
            &mut multinoise_lookup_map,
        );
        let multi_depth = self.depth.get_index_for_component(
            &mut multinoise_component_stack,
            &mut multinoise_nodes,
            &mut multinoise_lookup_map,
        );

        let mut compiled_fns = Vec::new();
        let fn_prefix = format!("eval_{}", dim_name);
        for (i, node) in noise_nodes.iter().enumerate() {
            compiled_fns.push(node.emit_compiled_eval_fn(i, &fn_prefix, &noise_lookup_map));
        }

        let compiled_mod_ident =
            syn::Ident::new(&format!("{}_compiled", dim_name), Span::call_site());
        let compiled_mod_ts = quote! {
            pub mod #compiled_mod_ident {
                use super::*;
                #(#compiled_fns)*
            }
        };

        let base_routers_ts = quote! {
            BaseNoiseRouters {
                noise: BaseNoiseRouter {
                    full_component_stack: &[#(#noise_component_stack),*],
                    barrier_noise: #barrier_noise,
                    fluid_level_floodedness_noise: #fluid_level_floodedness_noise,
                    fluid_level_spread_noise: #fluid_level_spread_noise,
                    lava_noise: #lava_noise,
                    erosion: #noise_erosion,
                    depth: #noise_depth,
                    final_density: #final_density,
                    vein_toggle: #vein_toggle,
                    vein_ridged: #vein_ridged,
                    vein_gap: #vein_gap,
                },
                surface_estimator: BaseSurfaceEstimator {
                    full_component_stack: &[#(#surface_component_stack),*],
                },
                multi_noise: BaseMultiNoiseRouter {
                    full_component_stack: &[#(#multinoise_component_stack),*],
                    temperature: #temperature,
                    vegetation: #vegetation,
                    continents: #continents,
                    erosion: #multi_erosion,
                    depth: #multi_depth,
                    ridges: #ridges,
                },
            }
        };

        (base_routers_ts, compiled_mod_ts)
    }
}

/// Reads vanilla datapack noise_settings and density_function files and emits the complete noise-router constants `TokenStream`.
pub fn build() -> TokenStream {
    let mut reprs: NoiseRouterReprs = load_vanilla_noise_routers();

    let _ = reprs.end_islands;

    let (overworld_router, overworld_compiled) =
        reprs.overworld.into_token_stream_compiled("overworld");
    let (amplified_router, amplified_compiled) = reprs
        .overworld_amplified
        .into_token_stream_compiled("amplified");
    let (large_biomes_router, large_biomes_compiled) = reprs
        .overworld_large_biomes
        .into_token_stream_compiled("large_biomes");
    let (nether_router, nether_compiled) = reprs.nether.into_token_stream_compiled("nether");
    let (end_router, end_compiled) = reprs.end.into_token_stream_compiled("end");

    let noise_evaluation_context = noise_evaluation_context_tokens();
    let sample_data = sample_data_tokens();
    let binary_and_linear = binary_and_linear_tokens();
    let unary_clamp_and_range = unary_clamp_and_range_tokens();
    let spline_and_wrapper = spline_and_wrapper_tokens();
    let base_component = base_component_tokens();
    let base_router_struct = base_router_struct_tokens();
    let base_router_impl = base_router_impl_tokens();

    quote! {
        #noise_evaluation_context

        #overworld_compiled
        #amplified_compiled
        #large_biomes_compiled
        #nether_compiled
        #end_compiled

        #sample_data

        #binary_and_linear

        #unary_clamp_and_range

        #spline_and_wrapper

        #base_component

        #base_router_struct

        pub const OVERWORLD_BASE_NOISE_ROUTER: BaseNoiseRouters = #overworld_router;
        pub const AMPLIFIED_BASE_NOISE_ROUTER: BaseNoiseRouters = #amplified_router;
        pub const LARGE_BIOMES_BASE_NOISE_ROUTER: BaseNoiseRouters = #large_biomes_router;
        pub const NETHER_BASE_NOISE_ROUTER: BaseNoiseRouters = #nether_router;
        pub const END_BASE_NOISE_ROUTER: BaseNoiseRouters = #end_router;

        #base_router_impl
    }
}
