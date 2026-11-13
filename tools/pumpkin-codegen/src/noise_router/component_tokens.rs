//! Emits the component stack (one token stream per unique component).

use super::{
    BinaryData, ClampData, ClampedYGradientData, DensityFunctionRepr, DistanceToPointData,
    FindTopSurfaceData, GradientData, InterpolatedNoiseSamplerData, LinearData, NoiseData,
    RangeChoiceData, RoundingData, ShiftedNoiseData, UnaryData,
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
    pub(super) fn unique_id(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }

    pub(super) fn get_index_for_component(
        &self,
        stack: &mut Vec<TokenStream>,
        nodes: &mut Vec<DensityFunctionRepr>,
        hash_to_index_map: &mut BTreeMap<u64, usize>,
    ) -> usize {
        if let Some(index) = hash_to_index_map.get(&self.unique_id()) {
            *index
        } else {
            let id = self.unique_id();
            let repr = self.get_token_stream(stack, nodes, hash_to_index_map);
            stack.push(repr);
            nodes.push(self.clone());
            let index = stack.len() - 1;
            hash_to_index_map.insert(id, index);
            index
        }
    }

    pub(super) fn get_token_stream(
        &self,
        stack: &mut Vec<TokenStream>,
        nodes: &mut Vec<DensityFunctionRepr>,
        hash_to_index_map: &mut BTreeMap<u64, usize>,
    ) -> TokenStream {
        match self {
            Self::Spline { spline, data } => {
                let _ = data;
                let spline_repr = spline.get_token_stream(stack, nodes, hash_to_index_map);

                quote! {
                    BaseNoiseFunctionComponent::Spline {
                        spline: &#spline_repr,
                    }
                }
            }
            Self::FindTopSurface {
                density,
                upper_bound,
                data,
            } => {
                let density_index =
                    density.get_index_for_component(stack, nodes, hash_to_index_map);
                let upper_bound_index =
                    upper_bound.get_index_for_component(stack, nodes, hash_to_index_map);
                let lower_bound = data.lower_bound;
                let cell_height = data.cell_height;

                quote! {
                    BaseNoiseFunctionComponent::FindTopSurface {
                        density_index: #density_index,
                        upper_bound_index: #upper_bound_index,
                        data: &FindTopSurfaceData {
                            lower_bound: #lower_bound,
                            cell_height: #cell_height,
                        },
                    }
                }
            }
            Self::EndIslands => quote! {
                BaseNoiseFunctionComponent::EndIslands
            },
            Self::Noise { data } => {
                let noise_id = quote::format_ident!("{}", data.noise_id.to_shouty_snake_case());
                let xz_scale = &data.xz_scale;
                let y_scale = &data.y_scale;

                quote! {
                    BaseNoiseFunctionComponent::Noise {
                        data: &NoiseData {
                            noise_id: DoublePerlinNoiseParameters::#noise_id,
                            xz_scale: #xz_scale,
                            y_scale: #y_scale,
                        }
                    }
                }
            }
            Self::ShiftA { noise_id } => {
                let noise_id = quote::format_ident!("{}", noise_id.to_shouty_snake_case());

                quote! {
                    BaseNoiseFunctionComponent::ShiftA {
                        noise_id: DoublePerlinNoiseParameters::#noise_id,
                    }
                }
            }
            Self::ShiftB { noise_id } => {
                let noise_id = quote::format_ident!("{}", noise_id.to_shouty_snake_case());

                quote! {
                    BaseNoiseFunctionComponent::ShiftB {
                        noise_id: DoublePerlinNoiseParameters::#noise_id,
                    }
                }
            }
            Self::BlendDensity { input } => {
                let input_index = input.get_index_for_component(stack, nodes, hash_to_index_map);

                quote! {
                    BaseNoiseFunctionComponent::BlendDensity {
                        input_index: #input_index,
                    }
                }
            }
            Self::BlendAlpha => {
                quote! {
                    BaseNoiseFunctionComponent::BlendAlpha
                }
            }
            Self::BlendOffset => {
                quote! {
                    BaseNoiseFunctionComponent::BlendOffset
                }
            }
            Self::Beardifier => {
                quote! {
                    BaseNoiseFunctionComponent::Beardifier
                }
            }
            Self::ShiftedNoise {
                shift_x,
                shift_y,
                shift_z,
                data,
            } => {
                let shift_x_index =
                    shift_x.get_index_for_component(stack, nodes, hash_to_index_map);
                let shift_y_index =
                    shift_y.get_index_for_component(stack, nodes, hash_to_index_map);
                let shift_z_index =
                    shift_z.get_index_for_component(stack, nodes, hash_to_index_map);

                let xz_scale = &data.xz_scale;
                let y_scale = &data.y_scale;
                let noise_id = quote::format_ident!("{}", data.noise_id.to_shouty_snake_case());

                quote! {
                    BaseNoiseFunctionComponent::ShiftedNoise {
                        shift_x_index: #shift_x_index,
                        shift_y_index: #shift_y_index,
                        shift_z_index: #shift_z_index,
                        data: &ShiftedNoiseData {
                            xz_scale: #xz_scale,
                            y_scale: #y_scale,
                            noise_id: DoublePerlinNoiseParameters::#noise_id,
                        },
                    }
                }
            }
            Self::RangeChoice {
                input,
                when_in_range,
                when_out_range,
                data,
            } => {
                let input_index = input.get_index_for_component(stack, nodes, hash_to_index_map);
                let when_in_index =
                    when_in_range.get_index_for_component(stack, nodes, hash_to_index_map);
                let when_out_index =
                    when_out_range.get_index_for_component(stack, nodes, hash_to_index_map);

                let min_inclusive = &data.min_inclusive;
                let max_exclusive = &data.max_exclusive;

                quote! {
                    BaseNoiseFunctionComponent::RangeChoice {
                        input_index: #input_index,
                        when_in_range_index: #when_in_index,
                        when_out_range_index: #when_out_index,
                        data: &RangeChoiceData {
                            min_inclusive: #min_inclusive,
                            max_exclusive: #max_exclusive,
                        },
                    }
                }
            }
            Self::Binary {
                argument1,
                argument2,
                data,
            } => {
                let argument1_index =
                    argument1.get_index_for_component(stack, nodes, hash_to_index_map);
                let argument2_index =
                    argument2.get_index_for_component(stack, nodes, hash_to_index_map);

                let action = data.operation.get_token_stream();
                quote! {
                    BaseNoiseFunctionComponent::Binary {
                        argument1_index: #argument1_index,
                        argument2_index: #argument2_index,
                        data: &BinaryData {
                            operation: #action,
                        },
                    }
                }
            }
            Self::ClampedYGradient { data } => {
                let from_y = data.from_y as f32;
                let to_y = data.to_y as f32;
                let from_value = &data.from_value;
                let to_value = &data.to_value;

                quote! {
                    BaseNoiseFunctionComponent::ClampedYGradient {
                        data: &ClampedYGradientData {
                            from_y: #from_y,
                            to_y: #to_y,
                            from_value: #from_value,
                            to_value: #to_value,
                        }
                    }
                }
            }
            Self::Gradient { data } => {
                let axis = data.axis.into_token_stream();
                let tiling = data.tiling.into_token_stream();
                let from_coordinate = data.from_coordinate;
                let to_coordinate = data.to_coordinate;
                let from_value = &data.from_value;
                let to_value = &data.to_value;

                quote! {
                    BaseNoiseFunctionComponent::Gradient {
                        data: &GradientData {
                            axis: #axis,
                            tiling: #tiling,
                            from_coordinate: #from_coordinate,
                            to_coordinate: #to_coordinate,
                            from_value: #from_value,
                            to_value: #to_value,
                        }
                    }
                }
            }
            Self::DistanceToPoint { data } => {
                let px = data.point[0];
                let py = data.point[1];
                let pz = data.point[2];
                let metric = data.metric.into_token_stream();

                quote! {
                    BaseNoiseFunctionComponent::DistanceToPoint {
                        data: &DistanceToPointData {
                            point: [#px, #py, #pz],
                            metric: #metric,
                        },
                    }
                }
            }
            Self::Lerp {
                alpha,
                first,
                second,
            } => {
                let alpha_index = alpha.get_index_for_component(stack, nodes, hash_to_index_map);
                let first_index = first.get_index_for_component(stack, nodes, hash_to_index_map);
                let second_index = second.get_index_for_component(stack, nodes, hash_to_index_map);

                quote! {
                    BaseNoiseFunctionComponent::Lerp {
                        alpha_index: #alpha_index,
                        first_index: #first_index,
                        second_index: #second_index,
                    }
                }
            }
            Self::Rounding {
                input,
                multiple,
                data,
            } => {
                let input_index = input.get_index_for_component(stack, nodes, hash_to_index_map);
                let multiple_index =
                    multiple.get_index_for_component(stack, nodes, hash_to_index_map);
                let action = data.operation.into_token_stream();

                quote! {
                    BaseNoiseFunctionComponent::Rounding {
                        input_index: #input_index,
                        multiple_index: #multiple_index,
                        data: &RoundingData {
                            operation: #action,
                        },
                    }
                }
            }
            Self::Slice {
                axis,
                coordinate,
                input,
            } => {
                let input_index = input.get_index_for_component(stack, nodes, hash_to_index_map);
                let axis_ts = axis.into_token_stream();

                quote! {
                    BaseNoiseFunctionComponent::Slice {
                        input_index: #input_index,
                        axis: #axis_ts,
                        coordinate: #coordinate,
                    }
                }
            }
            Self::Constant { value } => {
                quote! {
                    BaseNoiseFunctionComponent::Constant {
                        value: #value
                    }
                }
            }
            Self::Wrapper { input, wrapper } => {
                let input_index = input.get_index_for_component(stack, nodes, hash_to_index_map);
                let wrapper_repr = wrapper.into_token_stream();

                quote! {
                    BaseNoiseFunctionComponent::Wrapper {
                        input_index: #input_index,
                        wrapper: #wrapper_repr,
                    }
                }
            }
            Self::Linear { input, data } => {
                let input_index = input.get_index_for_component(stack, nodes, hash_to_index_map);

                let action = data.operation.into_token_stream();
                let argument = &data.argument;
                quote! {
                    BaseNoiseFunctionComponent::Linear {
                        input_index: #input_index,
                        data: &LinearData {
                            operation: #action,
                            argument: #argument,
                        },
                    }
                }
            }
            Self::Clamp { input, data } => {
                let input_index = input.get_index_for_component(stack, nodes, hash_to_index_map);

                let min_value = &data.min_value;
                let max_value = &data.max_value;

                quote! {
                    BaseNoiseFunctionComponent::Clamp {
                        input_index: #input_index,
                        data: &ClampData {
                            min_value: #min_value,
                            max_value: #max_value,
                        },
                    }
                }
            }
            Self::Unary { input, data } => {
                let input_index = input.get_index_for_component(stack, nodes, hash_to_index_map);

                let action = data.operation.into_token_stream();

                quote! {
                    BaseNoiseFunctionComponent::Unary {
                        input_index: #input_index,
                        data: &UnaryData {
                            operation: #action,
                        },
                    }
                }
            }
            Self::IntervalSelect {
                input,
                thresholds,
                functions,
            } => {
                let input_index = input.get_index_for_component(stack, nodes, hash_to_index_map);
                let functions_indices = functions
                    .iter()
                    .map(|f| f.get_index_for_component(stack, nodes, hash_to_index_map))
                    .collect::<Vec<_>>();
                let thresholds = thresholds.iter().map(|t| t.0).collect::<Vec<_>>();

                quote! {
                    BaseNoiseFunctionComponent::IntervalSelect {
                        input_index: #input_index,
                        thresholds: &[#(#thresholds),*],
                        functions_indices: &[#(#functions_indices),*],
                    }
                }
            }
            Self::InterpolatedNoiseSampler { data } => {
                let xz_scale = &data.xz_scale;
                let y_scale = &data.y_scale;
                let xz_factor = &data.xz_factor;
                let y_factor = &data.y_factor;
                let smear_scale_multiplier = &data.smear_scale_multiplier;

                quote! {
                    BaseNoiseFunctionComponent::InterpolatedNoiseSampler {
                        data: &InterpolatedNoiseSamplerData {
                            xz_scale: #xz_scale,
                            y_scale: #y_scale,
                            xz_factor: #xz_factor,
                            y_factor: #y_factor,
                            smear_scale_multiplier: #smear_scale_multiplier,
                        }
                    }
                }
            }
        }
    }
}
