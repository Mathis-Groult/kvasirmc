//! Loads the vanilla density functions and noise routers from the JSON data.

use std::{
    collections::BTreeMap,
    fs,
    hash::{DefaultHasher, Hash, Hasher},
};

use heck::ToShoutySnakeCase;

use proc_macro2::{Punct, Spacing, Span, TokenStream};
use quote::{ToTokens, TokenStreamExt, quote};
use serde::Deserialize;
use syn::Ident;

use super::{
    Axis, BinaryData, BinaryOperation, ClampData, DensityFunctionRepr, DistanceMetric,
    DistanceToPointData, FindTopSurfaceData, GradientData, HashableF32, HashableF64,
    InterpolatedNoiseSamplerData, LinearData, LinearOperation, NoiseData, NoiseRouterRepr,
    NoiseRouterReprs, RangeChoiceData, RoundingData, RoundingOperation, ShiftedNoiseData,
    SplineData, SplineRepr, Tiling, UnaryData, UnaryOperation, WrapperType,
};

fn load_df_json(base_df_dir: &std::path::Path, name: &str) -> serde_json::Value {
    let clean = name.strip_prefix("minecraft:").unwrap_or(name);
    let path = base_df_dir.join(format!("{clean}.json"));
    let content = fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "Failed to read density function at {}: {err}",
            path.display()
        )
    });
    serde_json::from_str(&content).unwrap_or_else(|err| {
        panic!(
            "Failed to parse density function at {}: {err}",
            path.display()
        )
    })
}

fn clean_noise_name(n: &str) -> String {
    n.strip_prefix("minecraft:").unwrap_or(n).to_string()
}

fn parse_axis(s: &str) -> Axis {
    match s {
        "x" | "X" => Axis::X,
        "y" | "Y" => Axis::Y,
        "z" | "Z" => Axis::Z,
        other => panic!("Unknown axis: {other}"),
    }
}

fn parse_tiling(s: &str) -> Tiling {
    match s {
        "clamp_to_edge" => Tiling::ClampToEdge,
        "repeat" => Tiling::Repeat,
        "mirrored_repeat" => Tiling::MirroredRepeat,
        _ => Tiling::ClampToEdge,
    }
}

fn parse_metric(s: &str) -> DistanceMetric {
    match s {
        "euclidean" => DistanceMetric::Euclidean,
        "euclidean_squared" => DistanceMetric::EuclideanSquared,
        "manhattan" => DistanceMetric::Manhattan,
        "chebyshev" => DistanceMetric::Chebyshev,
        other => panic!("Unknown distance metric: {other}"),
    }
}

fn parse_vanilla_df(base_df_dir: &std::path::Path, val: &serde_json::Value) -> DensityFunctionRepr {
    match val {
        serde_json::Value::Number(n) => DensityFunctionRepr::Constant {
            value: HashableF32(n.as_f64().unwrap_or(0.0) as f32),
        },
        serde_json::Value::String(s) => {
            if s == "minecraft:y" {
                DensityFunctionRepr::Gradient {
                    data: GradientData {
                        axis: Axis::Y,
                        tiling: Tiling::ClampToEdge,
                        from_coordinate: -4064,
                        to_coordinate: 4062,
                        from_value: HashableF32((-4064.0) as f32),
                        to_value: HashableF32((4062.0) as f32),
                    },
                }
            } else if s == "minecraft:zero" {
                DensityFunctionRepr::Constant {
                    value: HashableF32((0.0) as f32),
                }
            } else {
                let loaded = load_df_json(base_df_dir, s);
                parse_vanilla_df(base_df_dir, &loaded)
            }
        }
        serde_json::Value::Object(obj) => parse_vanilla_df_object(base_df_dir, val, obj),
        _ => panic!("Unsupported JSON value in density function: {val:?}"),
    }
}

fn parse_vanilla_df_object(
    base_df_dir: &std::path::Path,
    val: &serde_json::Value,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> DensityFunctionRepr {
    let type_str = obj
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or_else(|| panic!("Density function object missing type: {val:?}"));
    let clean_type = type_str.strip_prefix("minecraft:").unwrap_or(type_str);

    match clean_type {
        "constant" => parse_constant_df(obj),
        "y_clamped_gradient" => parse_y_clamped_gradient_df(obj),
        "gradient" => parse_gradient_df(obj),
        "distance_to_point" => parse_distance_to_point_df(obj),
        "lerp" => parse_lerp_df(base_df_dir, obj),
        "floor" | "round" | "ceil" | "truncate" => parse_rounding_df(base_df_dir, obj, clean_type),
        "slice" => parse_slice_df(base_df_dir, obj),
        "old_blended_noise" => parse_old_blended_noise_df(obj),
        "add" | "mul" | "min" | "max" | "sub" | "div" | "pow" => {
            parse_binary_df(base_df_dir, obj, clean_type)
        }
        "abs" | "square" | "cube" | "half_negative" | "quarter_negative" | "squeeze" | "invert"
        | "reciprocal" | "negate" | "sqrt" | "log" | "sign" => {
            parse_unary_df(base_df_dir, obj, clean_type)
        }
        "clamp" => parse_clamp_df(base_df_dir, obj),
        "range_choice" => parse_range_choice_df(base_df_dir, obj),
        "interval_select" => parse_interval_select_df(base_df_dir, obj),
        "spline" => parse_spline_df(base_df_dir, obj),
        "noise" => parse_noise_df(base_df_dir, obj),
        "shifted_noise" => parse_shifted_noise_df(base_df_dir, obj),
        "shift_a" => parse_shift_a_df(obj),
        "shift_b" => parse_shift_b_df(obj),
        "shift" => parse_shift_df(obj),
        "blend_alpha" => DensityFunctionRepr::BlendAlpha,
        "blend_offset" => DensityFunctionRepr::BlendOffset,
        "blend_density" => parse_blend_density_df(base_df_dir, obj),
        "end_islands" | "end_outer_islands" => DensityFunctionRepr::EndIslands,
        "beardifier" => DensityFunctionRepr::Beardifier,
        "cache" | "interpolated" | "flat_cache" | "cache_flat" | "cache_2d" | "cache_once"
        | "cache_all_in_cell" => parse_wrapper_df(base_df_dir, obj, clean_type),
        "find_top_surface" | "weird_utility_density" => parse_find_top_surface_df(base_df_dir, obj),
        other => panic!("Unknown density function type in datapack: {other}"),
    }
}

fn parse_constant_df(obj: &serde_json::Map<String, serde_json::Value>) -> DensityFunctionRepr {
    let num = obj
        .get("value")
        .or_else(|| obj.get("argument"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    DensityFunctionRepr::Constant {
        value: HashableF32((num) as f32),
    }
}

fn parse_y_clamped_gradient_df(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> DensityFunctionRepr {
    let from_y = obj.get("from_y").and_then(|v| v.as_i64()).unwrap_or(-64) as i32;
    let to_y = obj.get("to_y").and_then(|v| v.as_i64()).unwrap_or(320) as i32;
    let from_value = obj
        .get("from_value")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let to_value = obj.get("to_value").and_then(|v| v.as_f64()).unwrap_or(0.0);
    DensityFunctionRepr::Gradient {
        data: GradientData {
            axis: Axis::Y,
            tiling: Tiling::ClampToEdge,
            from_coordinate: from_y,
            to_coordinate: to_y,
            from_value: HashableF32((from_value) as f32),
            to_value: HashableF32((to_value) as f32),
        },
    }
}

fn parse_gradient_df(obj: &serde_json::Map<String, serde_json::Value>) -> DensityFunctionRepr {
    let axis_str = obj.get("axis").and_then(|v| v.as_str()).unwrap_or("y");
    let axis = parse_axis(axis_str);
    let tiling_str = obj
        .get("tiling")
        .and_then(|v| v.as_str())
        .unwrap_or("clamp_to_edge");
    let tiling = parse_tiling(tiling_str);
    let from_coordinate = obj
        .get("from_coordinate")
        .or_else(|| obj.get("from_y"))
        .and_then(|v| v.as_i64())
        .expect("Missing from_coordinate") as i32;
    let to_coordinate = obj
        .get("to_coordinate")
        .or_else(|| obj.get("to_y"))
        .and_then(|v| v.as_i64())
        .expect("Missing to_coordinate") as i32;
    let from_value = obj
        .get("from_value")
        .and_then(|v| v.as_f64())
        .expect("Missing from_value");
    let to_value = obj
        .get("to_value")
        .and_then(|v| v.as_f64())
        .expect("Missing to_value");
    DensityFunctionRepr::Gradient {
        data: GradientData {
            axis,
            tiling,
            from_coordinate,
            to_coordinate,
            from_value: HashableF32((from_value) as f32),
            to_value: HashableF32((to_value) as f32),
        },
    }
}

fn parse_distance_to_point_df(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> DensityFunctionRepr {
    let point_arr = obj
        .get("point")
        .and_then(|v| v.as_array())
        .expect("Missing point");
    let point = [
        point_arr[0].as_i64().unwrap_or(0) as i32,
        point_arr[1].as_i64().unwrap_or(0) as i32,
        point_arr[2].as_i64().unwrap_or(0) as i32,
    ];
    let metric_str = obj
        .get("metric")
        .and_then(|v| v.as_str())
        .unwrap_or("euclidean");
    let metric = parse_metric(metric_str);
    DensityFunctionRepr::DistanceToPoint {
        data: DistanceToPointData { point, metric },
    }
}

fn parse_lerp_df(
    base_df_dir: &std::path::Path,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> DensityFunctionRepr {
    let alpha = parse_vanilla_df(base_df_dir, obj.get("alpha").expect("Missing alpha"));
    let first = parse_vanilla_df(base_df_dir, obj.get("first").expect("Missing first"));
    let second = parse_vanilla_df(base_df_dir, obj.get("second").expect("Missing second"));
    DensityFunctionRepr::Lerp {
        alpha: Box::new(alpha),
        first: Box::new(first),
        second: Box::new(second),
    }
}

fn parse_rounding_df(
    base_df_dir: &std::path::Path,
    obj: &serde_json::Map<String, serde_json::Value>,
    clean_type: &str,
) -> DensityFunctionRepr {
    let op = match clean_type {
        "floor" => RoundingOperation::Floor,
        "round" => RoundingOperation::Round,
        "ceil" => RoundingOperation::Ceil,
        "truncate" => RoundingOperation::Truncate,
        _ => unreachable!(),
    };
    let input_node = obj
        .get("input")
        .or_else(|| obj.get("argument"))
        .expect("Missing input");
    let input = parse_vanilla_df(base_df_dir, input_node);
    let multiple = if let Some(m) = obj.get("multiple") {
        parse_vanilla_df(base_df_dir, m)
    } else {
        DensityFunctionRepr::Constant {
            value: HashableF32((1.0) as f32),
        }
    };
    DensityFunctionRepr::Rounding {
        input: Box::new(input),
        multiple: Box::new(multiple),
        data: RoundingData { operation: op },
    }
}

fn parse_slice_df(
    base_df_dir: &std::path::Path,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> DensityFunctionRepr {
    let axis_str = obj.get("axis").and_then(|v| v.as_str()).unwrap_or("y");
    let axis = parse_axis(axis_str);
    let coordinate = obj
        .get("coordinate")
        .and_then(|v| v.as_i64())
        .expect("Missing coordinate") as i32;
    let input = parse_vanilla_df(base_df_dir, obj.get("input").expect("Missing input"));
    DensityFunctionRepr::Slice {
        axis,
        coordinate,
        input: Box::new(input),
    }
}

fn parse_old_blended_noise_df(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> DensityFunctionRepr {
    let xz_scale = obj.get("xz_scale").and_then(|v| v.as_f64()).unwrap_or(0.25);
    let y_scale = obj.get("y_scale").and_then(|v| v.as_f64()).unwrap_or(0.125);
    let xz_factor = obj
        .get("xz_factor")
        .and_then(|v| v.as_f64())
        .unwrap_or(80.0);
    let y_factor = obj
        .get("y_factor")
        .and_then(|v| v.as_f64())
        .unwrap_or(160.0);
    let smear_scale_multiplier = obj
        .get("smear_scale_multiplier")
        .and_then(|v| v.as_f64())
        .unwrap_or(8.0);
    DensityFunctionRepr::InterpolatedNoiseSampler {
        data: InterpolatedNoiseSamplerData {
            xz_scale: HashableF64(xz_scale),
            y_scale: HashableF64(y_scale),
            xz_factor: HashableF64(xz_factor),
            y_factor: HashableF64(y_factor),
            smear_scale_multiplier: HashableF64(smear_scale_multiplier),
        },
    }
}

fn parse_binary_df(
    base_df_dir: &std::path::Path,
    obj: &serde_json::Map<String, serde_json::Value>,
    clean_type: &str,
) -> DensityFunctionRepr {
    let arg1_node = obj
        .get("left")
        .or_else(|| obj.get("argument1"))
        .or_else(|| obj.get("base"))
        .expect("Missing left/argument1/base");
    let arg2_node = obj
        .get("right")
        .or_else(|| obj.get("argument2"))
        .or_else(|| obj.get("exponent"))
        .expect("Missing right/argument2/exponent");
    let arg1 = parse_vanilla_df(base_df_dir, arg1_node);
    let arg2 = parse_vanilla_df(base_df_dir, arg2_node);
    let min_value = obj
        .get("min_value")
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::NEG_INFINITY);
    let max_value = obj
        .get("max_value")
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::INFINITY);

    if clean_type == "add" || clean_type == "mul" {
        let linear_op = if clean_type == "add" {
            LinearOperation::Add
        } else {
            LinearOperation::Mul
        };

        if let DensityFunctionRepr::Constant { value: c } = arg1 {
            return DensityFunctionRepr::Linear {
                input: Box::new(arg2),
                data: LinearData {
                    operation: linear_op,
                    argument: c,
                    min_value: HashableF32((min_value) as f32),
                    max_value: HashableF32((max_value) as f32),
                },
            };
        } else if let DensityFunctionRepr::Constant { value: c } = arg2 {
            return DensityFunctionRepr::Linear {
                input: Box::new(arg1),
                data: LinearData {
                    operation: linear_op,
                    argument: c,
                    min_value: HashableF32((min_value) as f32),
                    max_value: HashableF32((max_value) as f32),
                },
            };
        }
    }

    let op = match clean_type {
        "add" => BinaryOperation::Add,
        "mul" => BinaryOperation::Mul,
        "min" => BinaryOperation::Min,
        "max" => BinaryOperation::Max,
        "sub" => BinaryOperation::Sub,
        "div" => BinaryOperation::Div,
        "pow" => BinaryOperation::Pow,
        _ => unreachable!(),
    };

    DensityFunctionRepr::Binary {
        argument1: Box::new(arg1),
        argument2: Box::new(arg2),
        data: BinaryData {
            operation: op,
            min_value: HashableF32((min_value) as f32),
            max_value: HashableF32((max_value) as f32),
        },
    }
}

fn parse_unary_df(
    base_df_dir: &std::path::Path,
    obj: &serde_json::Map<String, serde_json::Value>,
    clean_type: &str,
) -> DensityFunctionRepr {
    let op = match clean_type {
        "abs" => UnaryOperation::Abs,
        "square" => UnaryOperation::Square,
        "cube" => UnaryOperation::Cube,
        "half_negative" => UnaryOperation::HalfNegative,
        "quarter_negative" => UnaryOperation::QuarterNegative,
        "squeeze" => UnaryOperation::Squeeze,
        "invert" | "reciprocal" => UnaryOperation::Invert,
        "negate" => UnaryOperation::Negate,
        "sqrt" => UnaryOperation::Sqrt,
        "log" => UnaryOperation::Log,
        "sign" => UnaryOperation::Sign,
        _ => unreachable!(),
    };
    let input_node = obj
        .get("argument")
        .or_else(|| obj.get("input"))
        .expect("Missing argument/input");
    let input = parse_vanilla_df(base_df_dir, input_node);
    let min_value = obj
        .get("min_value")
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::NEG_INFINITY);
    let max_value = obj
        .get("max_value")
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::INFINITY);
    DensityFunctionRepr::Unary {
        input: Box::new(input),
        data: UnaryData {
            operation: op,
            min_value: HashableF32((min_value) as f32),
            max_value: HashableF32((max_value) as f32),
        },
    }
}

fn parse_clamp_df(
    base_df_dir: &std::path::Path,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> DensityFunctionRepr {
    let input_node = obj
        .get("input")
        .or_else(|| obj.get("argument"))
        .expect("Missing input");
    let input = parse_vanilla_df(base_df_dir, input_node);
    let min_val = obj
        .get("min")
        .or_else(|| obj.get("min_value"))
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::NEG_INFINITY);
    let max_val = obj
        .get("max")
        .or_else(|| obj.get("max_value"))
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::INFINITY);
    DensityFunctionRepr::Clamp {
        input: Box::new(input),
        data: ClampData {
            min_value: HashableF32((min_val) as f32),
            max_value: HashableF32((max_val) as f32),
        },
    }
}

fn parse_range_choice_df(
    base_df_dir: &std::path::Path,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> DensityFunctionRepr {
    let input = parse_vanilla_df(base_df_dir, obj.get("input").expect("Missing input"));
    let min_inc = obj
        .get("min_inclusive")
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::NEG_INFINITY);
    let max_exc = obj
        .get("max_exclusive")
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::INFINITY);
    let when_in = parse_vanilla_df(
        base_df_dir,
        obj.get("when_in_range").expect("Missing when_in_range"),
    );
    let when_out = parse_vanilla_df(
        base_df_dir,
        obj.get("when_out_of_range")
            .expect("Missing when_out_of_range"),
    );
    DensityFunctionRepr::RangeChoice {
        input: Box::new(input),
        when_in_range: Box::new(when_in),
        when_out_range: Box::new(when_out),
        data: RangeChoiceData {
            min_inclusive: HashableF32((min_inc) as f32),
            max_exclusive: HashableF32((max_exc) as f32),
        },
    }
}

fn parse_interval_select_df(
    base_df_dir: &std::path::Path,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> DensityFunctionRepr {
    let input = parse_vanilla_df(base_df_dir, obj.get("input").expect("Missing input"));
    let thresholds: Vec<HashableF32> = obj
        .get("thresholds")
        .and_then(|v| v.as_array())
        .expect("Missing thresholds array")
        .iter()
        .map(|v| HashableF32(v.as_f64().unwrap_or(0.0) as f32))
        .collect();
    let funcs: Vec<DensityFunctionRepr> = obj
        .get("functions")
        .and_then(|v| v.as_array())
        .expect("Missing functions array")
        .iter()
        .map(|v| parse_vanilla_df(base_df_dir, v))
        .collect();

    DensityFunctionRepr::IntervalSelect {
        input: Box::new(input),
        thresholds: thresholds.into_boxed_slice(),
        functions: funcs.into_boxed_slice(),
    }
}

fn parse_spline_df(
    base_df_dir: &std::path::Path,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> DensityFunctionRepr {
    let spline_node = obj.get("spline").expect("Missing spline");
    let spline = parse_vanilla_spline(base_df_dir, spline_node);
    let min_value = obj
        .get("min_value")
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::NEG_INFINITY);
    let max_value = obj
        .get("max_value")
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::INFINITY);
    DensityFunctionRepr::Spline {
        spline,
        data: SplineData {
            min_value: HashableF32((min_value) as f32),
            max_value: HashableF32((max_value) as f32),
        },
    }
}

fn parse_noise_df(
    base_df_dir: &std::path::Path,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> DensityFunctionRepr {
    let noise_name = obj
        .get("noise")
        .and_then(|v| v.as_str())
        .expect("Missing noise name");
    let xz_scale = obj.get("xz_scale").and_then(|v| v.as_f64()).unwrap_or(1.0);
    let y_scale = obj.get("y_scale").and_then(|v| v.as_f64()).unwrap_or(1.0);
    let zero = serde_json::Value::Number(serde_json::Number::from_f64(0.0).unwrap());

    if obj.contains_key("shift_x") || obj.contains_key("shift_y") || obj.contains_key("shift_z") {
        let shift_x = parse_vanilla_df(base_df_dir, obj.get("shift_x").unwrap_or(&zero));
        let shift_y = parse_vanilla_df(base_df_dir, obj.get("shift_y").unwrap_or(&zero));
        let shift_z = parse_vanilla_df(base_df_dir, obj.get("shift_z").unwrap_or(&zero));
        DensityFunctionRepr::ShiftedNoise {
            shift_x: Box::new(shift_x),
            shift_y: Box::new(shift_y),
            shift_z: Box::new(shift_z),
            data: ShiftedNoiseData {
                noise_id: clean_noise_name(noise_name),
                xz_scale: HashableF64(xz_scale),
                y_scale: HashableF64(y_scale),
            },
        }
    } else {
        DensityFunctionRepr::Noise {
            data: NoiseData {
                noise_id: clean_noise_name(noise_name),
                xz_scale: HashableF64(xz_scale),
                y_scale: HashableF64(y_scale),
            },
        }
    }
}

fn parse_shifted_noise_df(
    base_df_dir: &std::path::Path,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> DensityFunctionRepr {
    let shift_x = parse_vanilla_df(base_df_dir, obj.get("shift_x").expect("Missing shift_x"));
    let shift_y = parse_vanilla_df(base_df_dir, obj.get("shift_y").expect("Missing shift_y"));
    let shift_z = parse_vanilla_df(base_df_dir, obj.get("shift_z").expect("Missing shift_z"));
    let noise_name = obj
        .get("noise")
        .and_then(|v| v.as_str())
        .expect("Missing noise name");
    let xz_scale = obj.get("xz_scale").and_then(|v| v.as_f64()).unwrap_or(1.0);
    let y_scale = obj.get("y_scale").and_then(|v| v.as_f64()).unwrap_or(1.0);
    DensityFunctionRepr::ShiftedNoise {
        shift_x: Box::new(shift_x),
        shift_y: Box::new(shift_y),
        shift_z: Box::new(shift_z),
        data: ShiftedNoiseData {
            noise_id: clean_noise_name(noise_name),
            xz_scale: HashableF64(xz_scale),
            y_scale: HashableF64(y_scale),
        },
    }
}

fn parse_shift_a_df(obj: &serde_json::Map<String, serde_json::Value>) -> DensityFunctionRepr {
    let offset_noise = obj
        .get("noise")
        .or_else(|| obj.get("argument"))
        .and_then(|v| v.as_str())
        .expect("Missing noise");
    DensityFunctionRepr::ShiftA {
        noise_id: clean_noise_name(offset_noise),
    }
}

fn parse_shift_b_df(obj: &serde_json::Map<String, serde_json::Value>) -> DensityFunctionRepr {
    let offset_noise = obj
        .get("noise")
        .or_else(|| obj.get("argument"))
        .and_then(|v| v.as_str())
        .expect("Missing noise");
    DensityFunctionRepr::ShiftB {
        noise_id: clean_noise_name(offset_noise),
    }
}

fn parse_shift_df(obj: &serde_json::Map<String, serde_json::Value>) -> DensityFunctionRepr {
    let offset_noise = obj
        .get("noise")
        .or_else(|| obj.get("argument"))
        .and_then(|v| v.as_str())
        .expect("Missing noise");
    DensityFunctionRepr::ShiftA {
        noise_id: clean_noise_name(offset_noise),
    }
}

fn parse_blend_density_df(
    base_df_dir: &std::path::Path,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> DensityFunctionRepr {
    let input = parse_vanilla_df(
        base_df_dir,
        obj.get("input")
            .or_else(|| obj.get("argument"))
            .expect("Missing input/argument"),
    );
    DensityFunctionRepr::BlendDensity {
        input: Box::new(input),
    }
}

fn parse_wrapper_df(
    base_df_dir: &std::path::Path,
    obj: &serde_json::Map<String, serde_json::Value>,
    clean_type: &str,
) -> DensityFunctionRepr {
    let input = parse_vanilla_df(
        base_df_dir,
        obj.get("input")
            .or_else(|| obj.get("argument"))
            .expect("Missing input/argument"),
    );
    let wrapper = if clean_type == "interpolated" {
        let cell_size_xz = obj
            .get("cell_size_xz")
            .and_then(|v| v.as_i64())
            .expect("Missing cell_size_xz") as i32;
        let cell_size_y = obj
            .get("cell_size_y")
            .and_then(|v| v.as_i64())
            .expect("Missing cell_size_y") as i32;
        WrapperType::Interpolated {
            cell_size_xz,
            cell_size_y,
        }
    } else {
        WrapperType::Cache
    };
    DensityFunctionRepr::Wrapper {
        input: Box::new(input),
        wrapper,
    }
}

fn parse_find_top_surface_df(
    base_df_dir: &std::path::Path,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> DensityFunctionRepr {
    let density = parse_vanilla_df(base_df_dir, obj.get("density").expect("Missing density"));
    let upper_bound = parse_vanilla_df(
        base_df_dir,
        obj.get("upper_bound").expect("Missing upper_bound"),
    );
    let lower_bound = obj
        .get("lower_bound")
        .and_then(|v| v.as_i64())
        .unwrap_or(-64) as i32;
    let cell_height = obj.get("cell_height").and_then(|v| v.as_i64()).unwrap_or(8) as i32;
    DensityFunctionRepr::FindTopSurface {
        density: Box::new(density),
        upper_bound: Box::new(upper_bound),
        data: FindTopSurfaceData {
            lower_bound,
            cell_height,
        },
    }
}

fn parse_vanilla_spline(base_df_dir: &std::path::Path, val: &serde_json::Value) -> SplineRepr {
    if let Some(n) = val.as_f64() {
        return SplineRepr::Fixed {
            value: HashableF32(n as f32),
        };
    }
    if let Some(obj) = val.as_object() {
        if obj.contains_key("value") && !obj.contains_key("points") {
            return parse_vanilla_spline(base_df_dir, &obj["value"]);
        }
        let loc_fn = parse_vanilla_df(
            base_df_dir,
            obj.get("coordinate").expect("Missing coordinate in spline"),
        );
        let points = obj
            .get("points")
            .and_then(|v| v.as_array())
            .expect("Missing points array in spline");

        let mut locations = Vec::new();
        let mut values = Vec::new();
        let mut derivatives = Vec::new();

        for pt in points {
            locations.push(HashableF32(pt["location"].as_f64().unwrap() as f32));
            values.push(parse_vanilla_spline(base_df_dir, &pt["value"]));
            derivatives.push(HashableF32(pt["derivative"].as_f64().unwrap() as f32));
        }

        return SplineRepr::Standard {
            location_function: Box::new(loc_fn),
            locations: locations.into_boxed_slice(),
            values: values.into_boxed_slice(),
            derivatives: derivatives.into_boxed_slice(),
        };
    }
    panic!("Unsupported spline node: {val:?}");
}

fn load_vanilla_noise_router(
    base_ns_dir: &std::path::Path,
    base_df_dir: &std::path::Path,
    dim_name: &str,
) -> NoiseRouterRepr {
    let path = base_ns_dir.join(format!("{dim_name}.json"));
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("Failed to read noise_settings at {}: {err}", path.display()));
    let val: serde_json::Value = serde_json::from_str(&content).unwrap_or_else(|err| {
        panic!(
            "Failed to parse noise_settings at {}: {err}",
            path.display()
        )
    });
    let nr = val
        .get("noise_router")
        .expect("Missing noise_router in noise_settings");
    let aquifers = val.get("aquifers");

    let zero = serde_json::Value::Number(serde_json::Number::from_f64(0.0).unwrap());

    let barrier = nr
        .get("barrier")
        .or_else(|| aquifers.and_then(|a| a.get("barrier")))
        .unwrap_or(&zero);
    let fluid_level_floodedness = nr
        .get("fluid_level_floodedness")
        .or_else(|| aquifers.and_then(|a| a.get("fluid_level_floodedness")))
        .unwrap_or(&zero);
    let fluid_level_spread = nr
        .get("fluid_level_spread")
        .or_else(|| aquifers.and_then(|a| a.get("fluid_level_spread")))
        .unwrap_or(&zero);
    let lava = nr
        .get("lava")
        .or_else(|| aquifers.and_then(|a| a.get("lava")))
        .unwrap_or(&zero);
    let preliminary_surface_level = nr
        .get("preliminary_surface_level")
        .or_else(|| nr.get("chunk_surface_level"))
        .or_else(|| aquifers.and_then(|a| a.get("surface_level")))
        .unwrap_or(&zero);

    let vein_toggle = nr.get("vein_toggle").cloned().unwrap_or_else(|| {
        if dim_name.starts_with("overworld")
            || dim_name == "amplified"
            || dim_name == "large_biomes"
        {
            serde_json::Value::String("minecraft:overworld/ore_vein/toggle".to_string())
        } else {
            zero.clone()
        }
    });
    let vein_ridged = nr.get("vein_ridged").cloned().unwrap_or_else(|| {
        if dim_name.starts_with("overworld")
            || dim_name == "amplified"
            || dim_name == "large_biomes"
        {
            serde_json::Value::String("minecraft:overworld/ore_vein/mask".to_string())
        } else {
            zero.clone()
        }
    });
    let vein_gap = nr.get("vein_gap").cloned().unwrap_or_else(|| {
        if dim_name.starts_with("overworld")
            || dim_name == "amplified"
            || dim_name == "large_biomes"
        {
            serde_json::Value::String("minecraft:overworld/ore_vein/gap".to_string())
        } else {
            zero.clone()
        }
    });

    NoiseRouterRepr {
        barrier_noise: parse_vanilla_df(base_df_dir, barrier),
        fluid_level_floodedness_noise: parse_vanilla_df(base_df_dir, fluid_level_floodedness),
        fluid_level_spread_noise: parse_vanilla_df(base_df_dir, fluid_level_spread),
        lava_noise: parse_vanilla_df(base_df_dir, lava),
        temperature: parse_vanilla_df(base_df_dir, nr.get("temperature").unwrap_or(&zero)),
        vegetation: parse_vanilla_df(base_df_dir, nr.get("vegetation").unwrap_or(&zero)),
        continents: parse_vanilla_df(base_df_dir, nr.get("continents").unwrap_or(&zero)),
        erosion: parse_vanilla_df(base_df_dir, nr.get("erosion").unwrap_or(&zero)),
        depth: parse_vanilla_df(base_df_dir, nr.get("depth").unwrap_or(&zero)),
        ridges: parse_vanilla_df(base_df_dir, nr.get("ridges").unwrap_or(&zero)),
        preliminary_surface_level: parse_vanilla_df(base_df_dir, preliminary_surface_level),
        final_density: parse_vanilla_df(base_df_dir, nr.get("final_density").unwrap_or(&zero)),
        vein_toggle: parse_vanilla_df(base_df_dir, &vein_toggle),
        vein_ridged: parse_vanilla_df(base_df_dir, &vein_ridged),
        vein_gap: parse_vanilla_df(base_df_dir, &vein_gap),
    }
}

pub(super) fn load_vanilla_noise_routers() -> NoiseRouterReprs {
    let base_ns_dir =
        std::path::Path::new("../../assets/datapack/data/minecraft/worldgen/noise_settings");
    let base_df_dir =
        std::path::Path::new("../../assets/datapack/data/minecraft/worldgen/density_function");

    let overworld = load_vanilla_noise_router(base_ns_dir, base_df_dir, "overworld");
    let overworld_large_biomes =
        load_vanilla_noise_router(base_ns_dir, base_df_dir, "large_biomes");
    let overworld_amplified = load_vanilla_noise_router(base_ns_dir, base_df_dir, "amplified");
    let nether = load_vanilla_noise_router(base_ns_dir, base_df_dir, "nether");
    let end = load_vanilla_noise_router(base_ns_dir, base_df_dir, "end");
    let end_islands = load_vanilla_noise_router(base_ns_dir, base_df_dir, "floating_islands");

    NoiseRouterReprs {
        overworld,
        overworld_large_biomes,
        overworld_amplified,
        nether,
        end,
        end_islands,
    }
}
