//! Axis analysis and the optimisation passes over a density function tree.

use super::{
    AXES_ALL, AXES_XZ, AXIS_X, AXIS_Y, AXIS_Z, Axis, BinaryOperation, DensityFunctionRepr,
    HashableF32, LinearOperation, UnaryOperation, WrapperType, noise_domain_axes,
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
    pub(super) fn domain_axes(&self) -> u8 {
        match self {
            Self::Constant { .. } => 0,
            Self::BlendAlpha
            | Self::BlendOffset
            | Self::EndIslands
            | Self::ShiftA { .. }
            | Self::ShiftB { .. } => AXES_XZ,
            Self::Beardifier
            | Self::InterpolatedNoiseSampler { .. }
            | Self::DistanceToPoint { .. } => AXES_ALL,
            Self::ClampedYGradient { .. } => AXIS_Y,
            Self::Gradient { data } => data.axis.as_axes(),
            Self::Noise { data } => noise_domain_axes(data.xz_scale.0, data.y_scale.0),
            Self::ShiftedNoise {
                shift_x,
                shift_y,
                shift_z,
                data,
            } => {
                noise_domain_axes(data.xz_scale.0, data.y_scale.0)
                    | shift_x.domain_axes()
                    | shift_y.domain_axes()
                    | shift_z.domain_axes()
            }
            Self::BlendDensity { input }
            | Self::Wrapper { input, .. }
            | Self::Linear { input, .. }
            | Self::Unary { input, .. }
            | Self::Clamp { input, .. } => input.domain_axes(),
            Self::Slice { axis, input, .. } => input.domain_axes() & !axis.as_axes(),
            Self::FindTopSurface {
                density,
                upper_bound,
                ..
            } => (density.domain_axes() | upper_bound.domain_axes()) & !AXIS_Y,
            Self::IntervalSelect {
                input, functions, ..
            } => functions
                .iter()
                .fold(input.domain_axes(), |axes, f| axes | f.domain_axes()),
            Self::Lerp {
                alpha,
                first,
                second,
            } => alpha.domain_axes() | first.domain_axes() | second.domain_axes(),
            Self::Rounding {
                input, multiple, ..
            } => input.domain_axes() | multiple.domain_axes(),
            Self::Binary {
                argument1,
                argument2,
                ..
            } => argument1.domain_axes() | argument2.domain_axes(),
            Self::RangeChoice {
                input,
                when_in_range,
                when_out_range,
                ..
            } => input.domain_axes() | when_in_range.domain_axes() | when_out_range.domain_axes(),
            Self::Spline { spline, .. } => spline.domain_axes(),
        }
    }

    pub(super) fn is_cache(&self) -> bool {
        matches!(
            self,
            Self::Wrapper {
                wrapper: WrapperType::Cache,
                ..
            }
        )
    }

    pub(super) fn for_each_child(&mut self, f: &mut dyn FnMut(&mut Self)) {
        match self {
            Self::Beardifier
            | Self::BlendAlpha
            | Self::BlendOffset
            | Self::EndIslands
            | Self::Noise { .. }
            | Self::ShiftA { .. }
            | Self::ShiftB { .. }
            | Self::InterpolatedNoiseSampler { .. }
            | Self::Constant { .. }
            | Self::ClampedYGradient { .. }
            | Self::Gradient { .. }
            | Self::DistanceToPoint { .. } => {}
            Self::BlendDensity { input }
            | Self::Wrapper { input, .. }
            | Self::Linear { input, .. }
            | Self::Unary { input, .. }
            | Self::Clamp { input, .. }
            | Self::Slice { input, .. } => f(input),
            Self::FindTopSurface {
                density,
                upper_bound,
                ..
            } => {
                f(density);
                f(upper_bound);
            }
            Self::ShiftedNoise {
                shift_x,
                shift_y,
                shift_z,
                ..
            } => {
                f(shift_x);
                f(shift_y);
                f(shift_z);
            }
            Self::IntervalSelect {
                input, functions, ..
            } => {
                f(input);
                for function in functions.iter_mut() {
                    f(function);
                }
            }
            Self::Lerp {
                alpha,
                first,
                second,
            } => {
                f(alpha);
                f(first);
                f(second);
            }
            Self::Rounding {
                input, multiple, ..
            } => {
                f(input);
                f(multiple);
            }
            Self::Binary {
                argument1,
                argument2,
                ..
            } => {
                f(argument1);
                f(argument2);
            }
            Self::RangeChoice {
                input,
                when_in_range,
                when_out_range,
                ..
            } => {
                f(input);
                f(when_in_range);
                f(when_out_range);
            }
            Self::Spline { spline, .. } => spline.for_each_function(f),
        }
    }

    pub(super) fn existing_removed_axes(&self) -> u8 {
        let mut axes = 0;
        let mut function = self;
        while let Self::Slice { axis, input, .. } = function {
            axes |= axis.as_axes();
            function = input;
        }
        axes
    }

    pub(super) fn remove_axes(&mut self, axes: u8) {
        let filtered = axes & !self.existing_removed_axes();
        for (bit, axis) in [(AXIS_X, Axis::X), (AXIS_Z, Axis::Z), (AXIS_Y, Axis::Y)] {
            if filtered & bit != 0 {
                let input = std::mem::replace(
                    self,
                    Self::Constant {
                        value: HashableF32(0.0),
                    },
                );
                *self = Self::Slice {
                    axis,
                    coordinate: 0,
                    input: Box::new(input),
                };
            }
        }
    }

    pub(super) fn slice_uniform_axes(&mut self, parent_axes: u8) {
        if matches!(
            self,
            Self::Constant { .. } | Self::Gradient { .. } | Self::ClampedYGradient { .. }
        ) {
            return;
        }
        let axes = self.domain_axes();
        let child_parent_axes = if self.is_cache() { AXES_ALL } else { axes };
        self.for_each_child(&mut |child| child.slice_uniform_axes(child_parent_axes));
        if parent_axes != axes {
            self.remove_axes(parent_axes & !axes);
        }
    }

    pub(super) fn optimize(&mut self) {
        match self {
            Self::BlendDensity { input } => input.optimize(),
            Self::Slice { input, .. } => input.optimize(),
            Self::FindTopSurface {
                density,
                upper_bound,
                ..
            } => {
                density.optimize();
                upper_bound.optimize();
            }
            Self::ShiftedNoise {
                shift_x,
                shift_y,
                shift_z,
                ..
            } => {
                shift_x.optimize();
                shift_y.optimize();
                shift_z.optimize();
            }
            Self::IntervalSelect {
                input, functions, ..
            } => {
                input.optimize();
                for f in functions.iter_mut() {
                    f.optimize();
                }
            }
            Self::Wrapper { input, .. } => input.optimize(),
            Self::Lerp {
                alpha,
                first,
                second,
            } => {
                alpha.optimize();
                first.optimize();
                second.optimize();
            }
            Self::Rounding {
                input, multiple, ..
            } => {
                input.optimize();
                multiple.optimize();
            }
            Self::RangeChoice {
                input,
                when_in_range,
                when_out_range,
                ..
            } => {
                input.optimize();
                when_in_range.optimize();
                when_out_range.optimize();
            }
            Self::Linear { input, data } => {
                input.optimize();
                if let Self::Constant { value } = &**input {
                    let val = match data.operation {
                        LinearOperation::Add => value.0 + data.argument.0,
                        LinearOperation::Mul => value.0 * data.argument.0,
                    };
                    *self = Self::Constant {
                        value: HashableF32((val) as f32),
                    };
                    return;
                }
            }
            Self::Binary {
                argument1,
                argument2,
                data,
            } => {
                argument1.optimize();
                argument2.optimize();
                if let (Self::Constant { value: v1 }, Self::Constant { value: v2 }) =
                    (&**argument1, &**argument2)
                {
                    let res = match data.operation {
                        BinaryOperation::Add => v1.0 + v2.0,
                        BinaryOperation::Mul => v1.0 * v2.0,
                        BinaryOperation::Min => v1.0.min(v2.0),
                        BinaryOperation::Max => v1.0.max(v2.0),
                        BinaryOperation::Sub => v1.0 - v2.0,
                        BinaryOperation::Div => {
                            if v2.0 == 0.0 {
                                0.0
                            } else {
                                v1.0 / v2.0
                            }
                        }
                        BinaryOperation::Pow => v1.0.powf(v2.0),
                    };
                    *self = Self::Constant {
                        value: HashableF32((res) as f32),
                    };
                    return;
                }
            }
            Self::Unary { input, data } => {
                input.optimize();
                if let Self::Constant { value } = &**input {
                    let val = match data.operation {
                        UnaryOperation::Abs => value.0.abs(),
                        UnaryOperation::Square => value.0 * value.0,
                        UnaryOperation::Cube => value.0 * value.0 * value.0,
                        UnaryOperation::HalfNegative => {
                            if value.0 > 0.0 {
                                value.0
                            } else {
                                value.0 * 0.5
                            }
                        }
                        UnaryOperation::QuarterNegative => {
                            if value.0 > 0.0 {
                                value.0
                            } else {
                                value.0 * 0.25
                            }
                        }
                        UnaryOperation::Squeeze => {
                            let c = value.0.clamp(-1.0, 1.0);
                            c / 2.0 - c * c * c / 24.0
                        }
                        UnaryOperation::Invert => {
                            if value.0 == 0.0 {
                                f32::INFINITY
                            } else {
                                1.0 / value.0
                            }
                        }
                        UnaryOperation::Negate => -value.0,
                        UnaryOperation::Sqrt => value.0.sqrt(),
                        UnaryOperation::Log => value.0.ln(),
                        UnaryOperation::Sign => {
                            if value.0 > 0.0 {
                                1.0
                            } else if value.0 < 0.0 {
                                -1.0
                            } else {
                                0.0
                            }
                        }
                    };
                    *self = Self::Constant {
                        value: HashableF32((val) as f32),
                    };
                }
            }
            Self::Clamp { input, data } => {
                input.optimize();
                if let Self::Constant { value } = &**input {
                    *self = Self::Constant {
                        value: HashableF32(
                            (value.0.clamp(data.min_value.0, data.max_value.0) as f32),
                        ),
                    };
                }
            }
            _ => {}
        }
    }
}
