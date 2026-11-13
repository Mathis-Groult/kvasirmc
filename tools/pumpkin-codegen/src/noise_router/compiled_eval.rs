//! Emits the compiled per-sample evaluation function of a component.

use super::{
    Axis, BinaryData, BinaryOperation, ClampData, ClampedYGradientData, DensityFunctionRepr,
    DistanceMetric, DistanceToPointData, FindTopSurfaceData, GradientData, HashableF32, LinearData,
    LinearOperation, NoiseData, RangeChoiceData, RoundingData, RoundingOperation, ShiftedNoiseData,
    SplineRepr, Tiling, UnaryData, UnaryOperation, WrapperType,
};
use heck::ToShoutySnakeCase;
use proc_macro2::{Punct, Spacing, Span, TokenStream};
use quote::{ToTokens, TokenStreamExt, quote};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    hash::{DefaultHasher, Hash, Hasher},
};
use syn::Ident;

impl DensityFunctionRepr {
    pub(super) fn get_index_for_component_readonly(
        &self,
        hash_to_index_map: &BTreeMap<u64, usize>,
    ) -> usize {
        *hash_to_index_map.get(&self.unique_id()).unwrap_or(&0)
    }

    pub(super) fn emit_compiled_eval_fn(
        &self,
        index: usize,
        fn_prefix: &str,
        hash_to_index_map: &BTreeMap<u64, usize>,
    ) -> TokenStream {
        let fn_name = syn::Ident::new(&format!("{}_{}", fn_prefix, index), Span::call_site());
        match self {
            Self::Constant { value } => emit_constant(&fn_name, value),
            Self::ClampedYGradient { data } => emit_clamped_y_gradient(&fn_name, data),
            Self::Gradient { data } => emit_gradient(&fn_name, data),
            Self::DistanceToPoint { data } => emit_distance_to_point(&fn_name, data),
            Self::Lerp {
                alpha,
                first,
                second,
            } => emit_lerp(&fn_name, alpha, first, second, fn_prefix, hash_to_index_map),
            Self::Rounding {
                input,
                multiple,
                data,
            } => emit_rounding(
                &fn_name,
                input,
                multiple,
                data,
                fn_prefix,
                hash_to_index_map,
            ),
            Self::Slice {
                axis,
                coordinate,
                input,
            } => emit_slice(
                &fn_name,
                axis,
                coordinate,
                input,
                fn_prefix,
                hash_to_index_map,
            ),
            Self::Linear { input, data } => {
                emit_linear(&fn_name, input, data, fn_prefix, hash_to_index_map)
            }
            Self::Unary { input, data } => {
                emit_unary(&fn_name, input, data, fn_prefix, hash_to_index_map)
            }
            Self::Clamp { input, data } => {
                emit_clamp(&fn_name, input, data, fn_prefix, hash_to_index_map)
            }
            Self::Binary {
                argument1,
                argument2,
                data,
            } => emit_binary(
                &fn_name,
                argument1,
                argument2,
                data,
                fn_prefix,
                hash_to_index_map,
            ),
            Self::RangeChoice {
                input,
                when_in_range,
                when_out_range,
                data,
            } => emit_range_choice(
                &fn_name,
                input,
                when_in_range,
                when_out_range,
                data,
                fn_prefix,
                hash_to_index_map,
            ),
            Self::Noise { data } => emit_noise(&fn_name, data),
            Self::ShiftA { noise_id } => emit_shift_a(&fn_name, noise_id),
            Self::ShiftB { noise_id } => emit_shift_b(&fn_name, noise_id),
            Self::ShiftedNoise {
                shift_x,
                shift_y,
                shift_z,
                data,
            } => emit_shifted_noise(
                &fn_name,
                shift_x,
                shift_y,
                shift_z,
                data,
                fn_prefix,
                hash_to_index_map,
            ),
            Self::BlendAlpha => emit_blend_alpha(&fn_name),
            Self::BlendOffset => emit_blend_offset(&fn_name),
            Self::BlendDensity { input } => {
                emit_blend_density(&fn_name, input, fn_prefix, hash_to_index_map)
            }
            Self::Beardifier => emit_beardifier(&fn_name),
            Self::EndIslands => emit_end_islands(&fn_name),
            Self::Wrapper { input, wrapper } => emit_wrapper(
                &fn_name,
                input,
                wrapper,
                index,
                fn_prefix,
                hash_to_index_map,
            ),
            Self::IntervalSelect {
                input,
                thresholds,
                functions,
            } => emit_interval_select(
                &fn_name,
                input,
                thresholds,
                functions,
                fn_prefix,
                hash_to_index_map,
            ),
            Self::InterpolatedNoiseSampler { .. } => emit_interpolated_noise_sampler(&fn_name),
            Self::Spline { spline, .. } => {
                emit_spline(&fn_name, spline, index, fn_prefix, hash_to_index_map)
            }
            Self::FindTopSurface {
                density,
                upper_bound,
                data,
            } => emit_find_top_surface(
                &fn_name,
                density,
                upper_bound,
                data,
                fn_prefix,
                hash_to_index_map,
            ),
        }
    }
}

fn emit_constant(fn_name: &syn::Ident, value: &HashableF32) -> TokenStream {
    let val = value.0;
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            let _ = (pos, ctx);
            #val
        }
    }
}

fn emit_clamped_y_gradient(fn_name: &syn::Ident, data: &ClampedYGradientData) -> TokenStream {
    let from_y = data.from_y as f32;
    let to_y = data.to_y as f32;
    let from_val = data.from_value.0;
    let to_val = data.to_value.0;
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            let _ = ctx;
            let y = pos.y as f32;
            let clamped = y.clamp(#from_y, #to_y);
            let delta = (clamped - #from_y) / (#to_y - #from_y);
            #from_val + delta * (#to_val - #from_val)
        }
    }
}

fn emit_gradient(fn_name: &syn::Ident, data: &GradientData) -> TokenStream {
    let from_coord = data.from_coordinate;
    let to_coord = data.to_coordinate;
    let from_val = data.from_value.0;
    let to_val = data.to_value.0;
    let coord = match data.axis {
        Axis::X => quote! { pos.x },
        Axis::Y => quote! { pos.y },
        Axis::Z => quote! { pos.z },
    };
    let range = to_coord - from_coord;
    let factor = (to_val - from_val) / (range as f32);
    let body = match data.tiling {
        Tiling::ClampToEdge => {
            let min_c = from_coord.min(to_coord);
            let max_c = from_coord.max(to_coord);
            quote! {
                let rel = coord.clamp(#min_c, #max_c) - #from_coord;
                #from_val + rel as f32 * #factor
            }
        }
        Tiling::Repeat => quote! {
            let rel = coord - #from_coord;
            #from_val + rel.rem_euclid(#range) as f32 * #factor
        },
        Tiling::MirroredRepeat => quote! {
            let rel = coord - #from_coord;
            let tile = rel.div_euclid(#range);
            let local = rel - tile * #range;
            if (tile & 1) == 0 {
                #from_val + local as f32 * #factor
            } else {
                #from_val + (#range - local) as f32 * #factor
            }
        },
    };
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            let _ = ctx;
            let coord = #coord;
            #body
        }
    }
}

fn emit_distance_to_point(fn_name: &syn::Ident, data: &DistanceToPointData) -> TokenStream {
    let px = data.point[0];
    let py = data.point[1];
    let pz = data.point[2];
    let body = match data.metric {
        DistanceMetric::Euclidean => quote! { (dx * dx + dy * dy + dz * dz).sqrt() },
        DistanceMetric::EuclideanSquared => quote! { dx * dx + dy * dy + dz * dz },
        DistanceMetric::Manhattan => quote! { dx.abs() + dy.abs() + dz.abs() },
        DistanceMetric::Chebyshev => quote! { dx.abs().max(dy.abs()).max(dz.abs()) },
    };
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            let _ = ctx;
            let dx = (pos.x - #px) as f32;
            let dy = (pos.y - #py) as f32;
            let dz = (pos.z - #pz) as f32;
            #body
        }
    }
}

fn emit_lerp(
    fn_name: &syn::Ident,
    alpha: &DensityFunctionRepr,
    first: &DensityFunctionRepr,
    second: &DensityFunctionRepr,
    fn_prefix: &str,
    hash_to_index_map: &BTreeMap<u64, usize>,
) -> TokenStream {
    let a_idx = alpha.get_index_for_component_readonly(hash_to_index_map);
    let f_idx = first.get_index_for_component_readonly(hash_to_index_map);
    let s_idx = second.get_index_for_component_readonly(hash_to_index_map);
    let a_fn = syn::Ident::new(&format!("{}_{}", fn_prefix, a_idx), Span::call_site());
    let f_fn = syn::Ident::new(&format!("{}_{}", fn_prefix, f_idx), Span::call_site());
    let s_fn = syn::Ident::new(&format!("{}_{}", fn_prefix, s_idx), Span::call_site());
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            let a = #a_fn(pos, ctx);
            let f = #f_fn(pos, ctx);
            let s = #s_fn(pos, ctx);
            f + a * (s - f)
        }
    }
}

fn emit_rounding(
    fn_name: &syn::Ident,
    input: &DensityFunctionRepr,
    multiple: &DensityFunctionRepr,
    data: &RoundingData,
    fn_prefix: &str,
    hash_to_index_map: &BTreeMap<u64, usize>,
) -> TokenStream {
    let in_idx = input.get_index_for_component_readonly(hash_to_index_map);
    let mul_idx = multiple.get_index_for_component_readonly(hash_to_index_map);
    let in_fn = syn::Ident::new(&format!("{}_{}", fn_prefix, in_idx), Span::call_site());
    let mul_fn = syn::Ident::new(&format!("{}_{}", fn_prefix, mul_idx), Span::call_site());
    let body = match data.operation {
        RoundingOperation::Floor => {
            quote! { if m == 0.0 { v } else { (v / m).floor() * m } }
        }
        RoundingOperation::Round => {
            quote! { if m == 0.0 { v } else { (v / m + 0.5).floor() * m } }
        }
        RoundingOperation::Ceil => {
            quote! { if m == 0.0 { v } else { (v / m).ceil() * m } }
        }
        RoundingOperation::Truncate => {
            quote! { if m == 0.0 { v } else { let d = v / m; if d > 0.0 { d.floor() * m } else { d.ceil() * m } } }
        }
    };
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            let v = #in_fn(pos, ctx);
            let m = #mul_fn(pos, ctx);
            #body
        }
    }
}

fn emit_slice(
    fn_name: &syn::Ident,
    axis: &Axis,
    coordinate: &i32,
    input: &DensityFunctionRepr,
    fn_prefix: &str,
    hash_to_index_map: &BTreeMap<u64, usize>,
) -> TokenStream {
    let child_idx = input.get_index_for_component_readonly(hash_to_index_map);
    let child_fn = syn::Ident::new(&format!("{}_{}", fn_prefix, child_idx), Span::call_site());
    let slice_pos = match axis {
        Axis::X => {
            quote! { pumpkin_util::math::vector3::Vector3::new(#coordinate, pos.y, pos.z) }
        }
        Axis::Y => {
            quote! { pumpkin_util::math::vector3::Vector3::new(pos.x, #coordinate, pos.z) }
        }
        Axis::Z => {
            quote! { pumpkin_util::math::vector3::Vector3::new(pos.x, pos.y, #coordinate) }
        }
    };
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            let slice_pos = #slice_pos;
            #child_fn(&slice_pos, ctx)
        }
    }
}

fn emit_linear(
    fn_name: &syn::Ident,
    input: &DensityFunctionRepr,
    data: &LinearData,
    fn_prefix: &str,
    hash_to_index_map: &BTreeMap<u64, usize>,
) -> TokenStream {
    let child_idx = input.get_index_for_component_readonly(hash_to_index_map);
    let child_fn = syn::Ident::new(&format!("{}_{}", fn_prefix, child_idx), Span::call_site());
    let arg = data.argument.0;
    let body = match data.operation {
        LinearOperation::Add => quote! { #child_fn(pos, ctx) + #arg },
        LinearOperation::Mul => quote! { #child_fn(pos, ctx) * #arg },
    };
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            #body
        }
    }
}

fn emit_unary(
    fn_name: &syn::Ident,
    input: &DensityFunctionRepr,
    data: &UnaryData,
    fn_prefix: &str,
    hash_to_index_map: &BTreeMap<u64, usize>,
) -> TokenStream {
    let child_idx = input.get_index_for_component_readonly(hash_to_index_map);
    let child_fn = syn::Ident::new(&format!("{}_{}", fn_prefix, child_idx), Span::call_site());
    let body = match data.operation {
        UnaryOperation::Abs => quote! { #child_fn(pos, ctx).abs() },
        UnaryOperation::Square => quote! { let v = #child_fn(pos, ctx); v * v },
        UnaryOperation::Cube => quote! { let v = #child_fn(pos, ctx); v * v * v },
        UnaryOperation::HalfNegative => {
            quote! { let v = #child_fn(pos, ctx); if v > 0.0 { v } else { v * 0.5 } }
        }
        UnaryOperation::QuarterNegative => {
            quote! { let v = #child_fn(pos, ctx); if v > 0.0 { v } else { v * 0.25 } }
        }
        UnaryOperation::Squeeze => {
            quote! { let c = #child_fn(pos, ctx).clamp(-1.0, 1.0); c / 2.0 - c * c * c / 24.0 }
        }
        UnaryOperation::Invert => {
            quote! { let v = #child_fn(pos, ctx); if v == 0.0 { f32::INFINITY } else { 1.0 / v } }
        }
        UnaryOperation::Negate => quote! { -#child_fn(pos, ctx) },
        UnaryOperation::Sqrt => quote! { #child_fn(pos, ctx).sqrt() },
        UnaryOperation::Log => quote! { #child_fn(pos, ctx).ln() },
        UnaryOperation::Sign => {
            quote! { let v = #child_fn(pos, ctx); if v > 0.0 { 1.0 } else if v < 0.0 { -1.0 } else { 0.0 } }
        }
    };
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            #body
        }
    }
}

fn emit_clamp(
    fn_name: &syn::Ident,
    input: &DensityFunctionRepr,
    data: &ClampData,
    fn_prefix: &str,
    hash_to_index_map: &BTreeMap<u64, usize>,
) -> TokenStream {
    let child_idx = input.get_index_for_component_readonly(hash_to_index_map);
    let child_fn = syn::Ident::new(&format!("{}_{}", fn_prefix, child_idx), Span::call_site());
    let min_v = data.min_value.0;
    let max_v = data.max_value.0;
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            #child_fn(pos, ctx).clamp(#min_v, #max_v)
        }
    }
}

fn emit_binary(
    fn_name: &syn::Ident,
    argument1: &DensityFunctionRepr,
    argument2: &DensityFunctionRepr,
    data: &BinaryData,
    fn_prefix: &str,
    hash_to_index_map: &BTreeMap<u64, usize>,
) -> TokenStream {
    let child1_idx = argument1.get_index_for_component_readonly(hash_to_index_map);
    let child2_idx = argument2.get_index_for_component_readonly(hash_to_index_map);
    let child1_fn = syn::Ident::new(&format!("{}_{}", fn_prefix, child1_idx), Span::call_site());
    let child2_fn = syn::Ident::new(&format!("{}_{}", fn_prefix, child2_idx), Span::call_site());
    let body = match data.operation {
        BinaryOperation::Add => quote! { #child1_fn(pos, ctx) + #child2_fn(pos, ctx) },
        BinaryOperation::Mul => quote! { #child1_fn(pos, ctx) * #child2_fn(pos, ctx) },
        BinaryOperation::Min => {
            quote! { #child1_fn(pos, ctx).min(#child2_fn(pos, ctx)) }
        }
        BinaryOperation::Max => {
            quote! { #child1_fn(pos, ctx).max(#child2_fn(pos, ctx)) }
        }
        BinaryOperation::Sub => quote! { #child1_fn(pos, ctx) - #child2_fn(pos, ctx) },
        BinaryOperation::Div => {
            quote! { let b = #child2_fn(pos, ctx); if b == 0.0 { 0.0 } else { #child1_fn(pos, ctx) / b } }
        }
        BinaryOperation::Pow => {
            quote! { #child1_fn(pos, ctx).powf(#child2_fn(pos, ctx)) }
        }
    };
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            #body
        }
    }
}

fn emit_range_choice(
    fn_name: &syn::Ident,
    input: &DensityFunctionRepr,
    when_in_range: &DensityFunctionRepr,
    when_out_range: &DensityFunctionRepr,
    data: &RangeChoiceData,
    fn_prefix: &str,
    hash_to_index_map: &BTreeMap<u64, usize>,
) -> TokenStream {
    let input_idx = input.get_index_for_component_readonly(hash_to_index_map);
    let when_in_idx = when_in_range.get_index_for_component_readonly(hash_to_index_map);
    let when_out_idx = when_out_range.get_index_for_component_readonly(hash_to_index_map);
    let input_fn = syn::Ident::new(&format!("{}_{}", fn_prefix, input_idx), Span::call_site());
    let when_in_fn = syn::Ident::new(&format!("{}_{}", fn_prefix, when_in_idx), Span::call_site());
    let when_out_fn = syn::Ident::new(
        &format!("{}_{}", fn_prefix, when_out_idx),
        Span::call_site(),
    );
    let min_inc = data.min_inclusive.0;
    let max_exc = data.max_exclusive.0;
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            let val = #input_fn(pos, ctx);
            if val >= #min_inc && val < #max_exc {
                #when_in_fn(pos, ctx)
            } else {
                #when_out_fn(pos, ctx)
            }
        }
    }
}

fn emit_noise(fn_name: &syn::Ident, data: &NoiseData) -> TokenStream {
    let noise_id = quote::format_ident!("{}", data.noise_id.to_shouty_snake_case());
    let xz_scale = data.xz_scale.0;
    let y_scale = data.y_scale.0;
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            ctx.sample_noise(DoublePerlinNoiseParameters::#noise_id, f64::from(pos.x) * #xz_scale, f64::from(pos.y) * #y_scale, f64::from(pos.z) * #xz_scale)
        }
    }
}

fn emit_shift_a(fn_name: &syn::Ident, noise_id: &str) -> TokenStream {
    let noise_id = quote::format_ident!("{}", noise_id.to_shouty_snake_case());
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            ctx.sample_shift_a(DoublePerlinNoiseParameters::#noise_id, pos)
        }
    }
}

fn emit_shift_b(fn_name: &syn::Ident, noise_id: &str) -> TokenStream {
    let noise_id = quote::format_ident!("{}", noise_id.to_shouty_snake_case());
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            ctx.sample_shift_b(DoublePerlinNoiseParameters::#noise_id, pos)
        }
    }
}

fn emit_shifted_noise(
    fn_name: &syn::Ident,
    shift_x: &DensityFunctionRepr,
    shift_y: &DensityFunctionRepr,
    shift_z: &DensityFunctionRepr,
    data: &ShiftedNoiseData,
    fn_prefix: &str,
    hash_to_index_map: &BTreeMap<u64, usize>,
) -> TokenStream {
    let sx_idx = shift_x.get_index_for_component_readonly(hash_to_index_map);
    let sy_idx = shift_y.get_index_for_component_readonly(hash_to_index_map);
    let sz_idx = shift_z.get_index_for_component_readonly(hash_to_index_map);
    let sx_fn = syn::Ident::new(&format!("{}_{}", fn_prefix, sx_idx), Span::call_site());
    let sy_fn = syn::Ident::new(&format!("{}_{}", fn_prefix, sy_idx), Span::call_site());
    let sz_fn = syn::Ident::new(&format!("{}_{}", fn_prefix, sz_idx), Span::call_site());
    let noise_id = quote::format_ident!("{}", data.noise_id.to_shouty_snake_case());
    let xz_scale = data.xz_scale.0;
    let y_scale = data.y_scale.0;
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            let sx = #sx_fn(pos, ctx);
            let sy = #sy_fn(pos, ctx);
            let sz = #sz_fn(pos, ctx);
            ctx.sample_shifted_noise(DoublePerlinNoiseParameters::#noise_id, sx, sy, sz, #xz_scale, #y_scale)
        }
    }
}

fn emit_blend_alpha(fn_name: &syn::Ident) -> TokenStream {
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            ctx.sample_blend_alpha(pos)
        }
    }
}

fn emit_blend_offset(fn_name: &syn::Ident) -> TokenStream {
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            ctx.sample_blend_offset(pos)
        }
    }
}

fn emit_blend_density(
    fn_name: &syn::Ident,
    input: &DensityFunctionRepr,
    fn_prefix: &str,
    hash_to_index_map: &BTreeMap<u64, usize>,
) -> TokenStream {
    let child_idx = input.get_index_for_component_readonly(hash_to_index_map);
    let child_fn = syn::Ident::new(&format!("{}_{}", fn_prefix, child_idx), Span::call_site());
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            let val = #child_fn(pos, ctx);
            ctx.sample_blend_density(val, pos)
        }
    }
}

fn emit_beardifier(fn_name: &syn::Ident) -> TokenStream {
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            ctx.sample_beardifier(pos)
        }
    }
}

fn emit_end_islands(fn_name: &syn::Ident) -> TokenStream {
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            ctx.sample_end_islands(pos)
        }
    }
}

fn emit_wrapper(
    fn_name: &syn::Ident,
    input: &DensityFunctionRepr,
    wrapper: &WrapperType,
    index: usize,
    fn_prefix: &str,
    hash_to_index_map: &BTreeMap<u64, usize>,
) -> TokenStream {
    let child_idx = input.get_index_for_component_readonly(hash_to_index_map);
    let child_fn = syn::Ident::new(&format!("{}_{}", fn_prefix, child_idx), Span::call_site());
    let wrapper_repr = wrapper.into_token_stream();
    let comp_idx = index;
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            ctx.sample_wrapper(#comp_idx, #wrapper_repr, pos, &#child_fn)
        }
    }
}

fn emit_interval_select(
    fn_name: &syn::Ident,
    input: &DensityFunctionRepr,
    thresholds: &[HashableF32],
    functions: &[DensityFunctionRepr],
    fn_prefix: &str,
    hash_to_index_map: &BTreeMap<u64, usize>,
) -> TokenStream {
    let input_idx = input.get_index_for_component_readonly(hash_to_index_map);
    let input_fn = syn::Ident::new(&format!("{}_{}", fn_prefix, input_idx), Span::call_site());
    let func_fns = functions
        .iter()
        .map(|f| {
            let idx = f.get_index_for_component_readonly(hash_to_index_map);
            syn::Ident::new(&format!("{}_{}", fn_prefix, idx), Span::call_site())
        })
        .collect::<Vec<_>>();
    let threshold_values = thresholds.iter().map(|t| t.0).collect::<Vec<_>>();
    let th_indices = (0..threshold_values.len()).collect::<Vec<_>>();
    let last_func_fn = func_fns.last().unwrap();
    let initial_func_fns = if func_fns.len() > 1 {
        &func_fns[..func_fns.len() - 1]
    } else {
        &[]
    };
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            let input_val = #input_fn(pos, ctx);
            let thresholds = &[#(#threshold_values),*];
            let mut selected = thresholds.len();
            for (i, &t) in thresholds.iter().enumerate() {
                if input_val < t {
                    selected = i;
                    break;
                }
            }
            match selected {
                #( #th_indices => #initial_func_fns(pos, ctx), )*
                _ => #last_func_fn(pos, ctx),
            }
        }
    }
}

fn emit_interpolated_noise_sampler(fn_name: &syn::Ident) -> TokenStream {
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            ctx.sample_interpolated_noise(pos)
        }
    }
}

fn emit_spline(
    fn_name: &syn::Ident,
    spline: &SplineRepr,
    index: usize,
    fn_prefix: &str,
    hash_to_index_map: &BTreeMap<u64, usize>,
) -> TokenStream {
    let loc_idx = match spline {
        SplineRepr::Fixed { .. } => None,
        SplineRepr::Standard {
            location_function, ..
        } => Some(location_function.get_index_for_component_readonly(hash_to_index_map)),
    };
    if let Some(loc_idx) = loc_idx {
        let loc_fn = syn::Ident::new(&format!("{}_{}", fn_prefix, loc_idx), Span::call_site());
        quote! {
            #[inline(always)]
            pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                let location_val = #loc_fn(pos, ctx);
                ctx.sample_spline(#index, location_val, pos)
            }
        }
    } else {
        let val = match spline {
            SplineRepr::Fixed { value } => value.0 as f32,
            _ => 0.0,
        };
        quote! {
            #[inline(always)]
            pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
                let _ = (pos, ctx);
                #val
            }
        }
    }
}

fn emit_find_top_surface(
    fn_name: &syn::Ident,
    density: &DensityFunctionRepr,
    upper_bound: &DensityFunctionRepr,
    data: &FindTopSurfaceData,
    fn_prefix: &str,
    hash_to_index_map: &BTreeMap<u64, usize>,
) -> TokenStream {
    let d_idx = density.get_index_for_component_readonly(hash_to_index_map);
    let u_idx = upper_bound.get_index_for_component_readonly(hash_to_index_map);
    let d_fn = syn::Ident::new(&format!("{}_{}", fn_prefix, d_idx), Span::call_site());
    let u_fn = syn::Ident::new(&format!("{}_{}", fn_prefix, u_idx), Span::call_site());
    let lower = data.lower_bound;
    let cell_h = data.cell_height;
    quote! {
        #[inline(always)]
        pub fn #fn_name<C: NoiseEvaluationContext>(pos: &pumpkin_util::math::vector3::Vector3<i32>, ctx: &mut C) -> f32 {
            ctx.sample_find_top_surface(&#d_fn, &#u_fn, #lower, #cell_h, pos)
        }
    }
}
