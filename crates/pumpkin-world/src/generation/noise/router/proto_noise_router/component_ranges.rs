//! The range (`min`, `max`) of a component, worked out from the ranges of the components it reads.
//!
//! Pure arithmetic on `(min, max)` pairs: no stack, no random source. A range is conservative: the
//! component never leaves it, and an operation whose result cannot be bounded (a division, a
//! power) gets an infinite end.

use pumpkin_data::noise_router::{
    BinaryOperation, LinearData, LinearOperation, UnaryData, UnaryOperation,
};

/// The lowest and highest value a component can take.
pub(super) type Range = (f32, f32);

/// The range of the product of two values with the given ranges.
fn product(first: Range, second: Range) -> Range {
    let (first_min, first_max) = first;
    let (second_min, second_max) = second;
    let min = if first_min > 0.0 && second_min > 0.0 {
        first_min * second_min
    } else if first_max < 0.0 && second_max < 0.0 {
        first_max * second_max
    } else {
        (first_min * second_max).min(first_max * second_min)
    };

    let max = if first_min > 0.0 && second_min > 0.0 {
        first_max * second_max
    } else if first_max < 0.0 && second_max < 0.0 {
        first_min * second_min
    } else {
        (first_min * second_min).max(first_max * second_max)
    };

    (min, max)
}

/// The range of a binary operation on two values with the given ranges.
pub(super) fn binary(operation: BinaryOperation, first: Range, second: Range) -> Range {
    let (first_min, first_max) = first;
    let (second_min, second_max) = second;
    match operation {
        BinaryOperation::Add => (first_min + second_min, first_max + second_max),
        BinaryOperation::Mul => product(first, second),
        BinaryOperation::Min => (first_min.min(second_min), first_max.min(second_max)),
        BinaryOperation::Max => (first_min.max(second_min), first_max.max(second_max)),
        BinaryOperation::Sub => (first_min - second_max, first_max - second_min),
        BinaryOperation::Div => (f32::NEG_INFINITY, f32::INFINITY),
        BinaryOperation::Pow => (0.0, f32::INFINITY),
    }
}

/// The range of a value with the given range, added to or multiplied by a constant.
pub(super) fn linear(data: &LinearData, input: Range) -> Range {
    let (input_min, input_max) = input;
    match data.operation {
        LinearOperation::Add => (input_min + data.argument, input_max + data.argument),
        LinearOperation::Mul => product(input, (data.argument, data.argument)),
    }
}

/// The range of a unary operation applied to a value with the given range.
pub(super) fn unary(data: &UnaryData, input: Range) -> Range {
    let (input_min, input_max) = input;
    let applied_min = data.apply_density(input_min);
    let applied_max = data.apply_density(input_max);

    match data.operation {
        UnaryOperation::Abs | UnaryOperation::Square => {
            (input_min.max(0.0), applied_min.max(applied_max))
        }
        UnaryOperation::Squeeze
        | UnaryOperation::Cube
        | UnaryOperation::QuarterNegative
        | UnaryOperation::HalfNegative => (applied_min, applied_max),
        UnaryOperation::Invert => {
            if input_min < 0.0 && input_max > 0.0 {
                (f32::NEG_INFINITY, f32::INFINITY)
            } else {
                (applied_max, applied_min)
            }
        }
        UnaryOperation::Negate => (-input_max, -input_min),
        UnaryOperation::Sqrt => (0.0, applied_max.max(0.0)),
        UnaryOperation::Log => (f32::NEG_INFINITY, applied_max),
        UnaryOperation::Sign => (-1.0, 1.0),
    }
}

/// The range that covers two ranges.
pub(super) const fn union(first: Range, second: Range) -> Range {
    (first.0.min(second.0), first.1.max(second.1))
}

/// The range that covers every range in `ranges` (empty: `(INFINITY, NEG_INFINITY)`).
pub(super) fn covering(ranges: impl IntoIterator<Item = Range>) -> Range {
    let mut min_value = f32::INFINITY;
    let mut max_value = f32::NEG_INFINITY;
    for (min, max) in ranges {
        if min < min_value {
            min_value = min;
        }
        if max > max_value {
            max_value = max;
        }
    }
    (min_value, max_value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_of_two_positive_ranges_multiplies_the_ends() {
        assert_eq!(product((1.0, 2.0), (3.0, 4.0)), (3.0, 8.0));
    }

    #[test]
    fn product_of_two_negative_ranges_swaps_the_ends() {
        assert_eq!(product((-3.0, -1.0), (-4.0, -2.0)), (2.0, 12.0));
    }

    #[test]
    fn product_of_ranges_with_mixed_signs_takes_the_extreme_corners() {
        assert_eq!(product((-2.0, 5.0), (-3.0, 4.0)), (-15.0, 20.0));
    }

    #[test]
    fn union_spans_both_ranges() {
        assert_eq!(union((1.0, 2.0), (-3.0, 1.5)), (-3.0, 2.0));
    }

    #[test]
    fn covering_range_spans_every_input() {
        assert_eq!(
            covering([(1.0, 2.0), (-3.0, -1.0), (0.0, 7.0)]),
            (-3.0, 7.0)
        );
    }

    #[test]
    fn covering_nothing_gives_an_inverted_range() {
        assert_eq!(covering([]), (f32::INFINITY, f32::NEG_INFINITY));
    }
}
