use super::packed::{put, unsigned};
use super::{ConvertKind as C, FloatKind as F, SimdOperation};
use crate::math::{
    Rounding,
    ieee::{self, BinaryFormat, BinaryOperation},
};

fn rounding(mxcsr: u32) -> Rounding {
    match (mxcsr >> 13) & 3 {
        0 => Rounding::Nearest,
        1 => Rounding::Down,
        2 => Rounding::Up,
        _ => Rounding::Zero,
    }
}
fn ieee_format(width: usize) -> BinaryFormat {
    if width == 4 {
        BinaryFormat::Binary32
    } else {
        BinaryFormat::Binary64
    }
}
fn ieee_operation(kind: F) -> BinaryOperation {
    match kind {
        F::Add => BinaryOperation::Add,
        F::Subtract => BinaryOperation::Subtract,
        F::Multiply => BinaryOperation::Multiply,
        F::Divide => BinaryOperation::Divide,
        _ => unreachable!(),
    }
}
fn lane_exceptions(mut flags: u32, mxcsr: u32) -> u32 {
    if flags & !(mxcsr >> 7) & 0x18 != 0 {
        flags &= !0x20;
    }
    flags
}
fn instruction_exceptions(flags: u32, mxcsr: u32) -> u32 {
    if flags & !(mxcsr >> 7) & 7 != 0 {
        flags & 7
    } else {
        flags
    }
}
fn format(width: usize) -> (u64, u64, u64, u32, i32) {
    ieee_format(width).parameters()
}
fn nan(bits: u64, width: usize) -> bool {
    let (_, e, f, _, _) = format(width);
    bits & e == e && bits & f != 0
}
fn signaling(bits: u64, width: usize) -> bool {
    let (_, _, _, p, _) = format(width);
    nan(bits, width) && bits & (1 << (p - 1)) == 0
}
fn denormal(bits: u64, width: usize) -> bool {
    let (_, e, f, _, _) = format(width);
    bits & e == 0 && bits & f != 0
}
fn quiet(bits: u64, width: usize) -> u64 {
    bits | (1 << (format(width).3 - 1))
}
fn indefinite(width: usize) -> u64 {
    let (s, e, _, p, _) = format(width);
    s | e | (1 << (p - 1))
}
fn input(bits: u64, width: usize, mxcsr: u32) -> (u64, u32) {
    if denormal(bits, width) {
        if mxcsr & 0x40 != 0 {
            (bits & format(width).0, 0)
        } else {
            (bits, 2)
        }
    } else {
        (bits, 0)
    }
}
fn less(a: u64, b: u64, width: usize) -> bool {
    let sign = format(width).0;
    if a & !sign == 0 && b & !sign == 0 {
        return false;
    }
    if (a ^ b) & sign != 0 {
        return a & sign != 0;
    }
    if a & sign != 0 { a > b } else { a < b }
}
fn equal(a: u64, b: u64, width: usize) -> bool {
    a == b || (a | b) & !format(width).0 == 0
}
fn unbounded_tiny(kind: F, a: u64, b: u64, width: usize, round: Rounding) -> bool {
    matches!(kind, F::Add | F::Subtract | F::Multiply | F::Divide)
        && ieee::unbounded_tiny(ieee_operation(kind), ieee_format(width), a, b, round)
}

fn square_root(bits: u64, width: usize, round: Rounding) -> (u64, u32) {
    let result = ieee::square_root(bits, ieee_format(width), round);
    (result.bits, u32::from(result.exceptions))
}

fn binary(
    kind: F,
    mut a: u64,
    mut b: u64,
    width: usize,
    mxcsr: u32,
    predicate: u8,
) -> (u64, u32, Option<u32>) {
    let unary = matches!(
        kind,
        F::SquareRoot | F::Reciprocal | F::ReciprocalSquareRoot
    );
    let (aa, ad) = input(a, width, mxcsr);
    a = aa;
    let (bb, bd) = input(b, width, mxcsr);
    b = bb;
    let mut flags = if unary { bd } else { ad | bd };
    let an = nan(a, width);
    let bn = nan(b, width);
    let sn = signaling(a, width) || signaling(b, width);
    if matches!(kind, F::Compare | F::CompareFlags | F::CompareFlagsQuiet) {
        let unordered = an || bn;
        let invalid = sn
            || (unordered
                && (kind == F::CompareFlags
                    || (kind == F::Compare && matches!(predicate & 7, 1 | 2 | 5 | 6))));
        if invalid {
            flags = 1;
        }
        let eq = !unordered && equal(a, b, width);
        let lt = !unordered && less(a, b, width);
        if kind != F::Compare {
            return (
                0,
                flags,
                Some(if unordered {
                    0x45
                } else if lt {
                    1
                } else if eq {
                    0x40
                } else {
                    0
                }),
            );
        }
        let result = match predicate & 7 {
            0 => eq,
            1 => lt,
            2 => lt || eq,
            3 => unordered,
            4 => !eq,
            5 => !lt,
            6 => !(lt || eq),
            _ => !unordered,
        };
        return (if result { u64::MAX } else { 0 }, flags, None);
    }
    if matches!(kind, F::Minimum | F::Maximum) {
        if an || bn {
            flags = 1;
        }
        let take_a = !an
            && !bn
            && !equal(a, b, width)
            && if kind == F::Minimum {
                less(a, b, width)
            } else {
                less(b, a, width)
            };
        return (if take_a { a } else { b }, flags, None);
    }
    if unary {
        if bn {
            return (
                quiet(b, width),
                if matches!(kind, F::Reciprocal | F::ReciprocalSquareRoot) {
                    0
                } else {
                    u32::from(signaling(b, width))
                },
                None,
            );
        }
    } else if an || bn {
        return (quiet(if an { a } else { b }, width), u32::from(sn), None);
    }
    if matches!(kind, F::Add | F::Subtract)
        && let Some(result) = ieee::exact_normal_sum(a, b, ieee_format(width), kind == F::Subtract)
    {
        return (result, 0, None);
    }
    if kind == F::Multiply
        && let Some(result) = ieee::exact_normal_product(a, b, ieee_format(width))
    {
        return (result, 0, None);
    }
    let round = rounding(mxcsr);
    let (mut result, mut status) = if kind == F::SquareRoot {
        square_root(b, width, round)
    } else if matches!(kind, F::Reciprocal | F::ReciprocalSquareRoot) {
        // exact nearest results satisfy the ISA approximation error bound; these instructions ignore RC and do not report SIMD exceptions.
        let b = if denormal(b, width) {
            b & format(width).0
        } else {
            b
        };
        let denominator = if kind == F::ReciprocalSquareRoot {
            square_root(b, width, Rounding::Nearest).0
        } else {
            b
        };
        let value = if width == 4 {
            ieee::binary(
                BinaryOperation::Divide,
                BinaryFormat::Binary32,
                0x3f80_0000,
                denominator,
                Rounding::Nearest,
            )
            .bits
        } else {
            unreachable!()
        };
        return (
            if denormal(value, width) {
                value & format(width).0
            } else {
                value
            },
            0,
            None,
        );
    } else {
        let result = ieee::binary(ieee_operation(kind), ieee_format(width), a, b, round);
        (result.bits, u32::from(result.exceptions))
    };
    if status & 1 != 0 {
        result = indefinite(width);
    }
    if status & 5 != 0 {
        flags = 0;
    }
    let minimum_normal = if width == 4 {
        0x0080_0000
    } else {
        0x0010_0000_0000_0000
    };
    let tiny =
        result & !format(width).0 <= minimum_normal && unbounded_tiny(kind, a, b, width, round);
    if tiny {
        if mxcsr & 0x800 == 0 || status & 0x20 != 0 {
            status |= 0x10;
        }
        if mxcsr & 0x800 != 0 && mxcsr & 0x8000 != 0 {
            result &= format(width).0;
            status |= 0x30;
        }
    }
    (result, status | flags, None)
}

fn round_integral(op: SimdOperation, a: [u8; 16], b: [u8; 16], mxcsr: u32) -> ([u8; 16], u32) {
    let immediate = op.immediate.unwrap();
    let mode = if immediate & 4 != 0 {
        rounding(mxcsr)
    } else {
        rounding(u32::from(immediate & 3) << 13)
    };
    let width = usize::from(op.lane_bytes);
    let count = if op.scalar { 1 } else { 16 / width };
    let mut out = a;
    let mut exceptions = 0;
    for lane in 0..count {
        // round honors DAZ, but does not signal the denormal operand exception.
        let (bits, _) = input(unsigned(&b, lane * width, width), width, mxcsr);
        let result = ieee::round_to_integral(bits, ieee_format(width), mode);
        let status =
            u32::from(result.exceptions) & if immediate & 8 != 0 { !0x20 } else { u32::MAX };
        exceptions |= status;
        put(&mut out, lane * width, width, result.bits);
    }
    (out, instruction_exceptions(exceptions, mxcsr))
}

fn dot_stage(status: u32, mxcsr: u32, exceptions: &mut u32) -> bool {
    let status = instruction_exceptions(status, mxcsr);
    *exceptions |= status;
    status & !(mxcsr >> 7) & 63 != 0
}

fn dot_product(op: SimdOperation, a: [u8; 16], b: [u8; 16], mxcsr: u32) -> ([u8; 16], u32) {
    let width = usize::from(op.lane_bytes);
    let count = 16 / width;
    let immediate = op.immediate.unwrap();
    let mut products = [0; 4];
    let mut product_exceptions = 0;
    for (lane, product) in products[..count].iter_mut().enumerate() {
        if immediate & (1 << (lane + 4)) != 0 {
            let (bits, status, _) = binary(
                F::Multiply,
                unsigned(&a, lane * width, width),
                unsigned(&b, lane * width, width),
                width,
                mxcsr,
                0,
            );
            *product = bits;
            product_exceptions |= lane_exceptions(status, mxcsr);
        }
    }
    // dot products deliver exceptions between the packed multiplication and
    // each sequential addition. earlier completed stages retain sticky flags.
    let mut exceptions = 0;
    if dot_stage(product_exceptions, mxcsr, &mut exceptions) {
        return (a, exceptions);
    }
    let (left, status, _) = binary(F::Add, products[0], products[1], width, mxcsr, 0);
    if dot_stage(lane_exceptions(status, mxcsr), mxcsr, &mut exceptions) {
        return (a, exceptions);
    }
    let sum = if count == 4 {
        let (right, status, _) = binary(F::Add, products[2], products[3], width, mxcsr, 0);
        if dot_stage(lane_exceptions(status, mxcsr), mxcsr, &mut exceptions) {
            return (a, exceptions);
        }
        let (sum, status, _) = binary(F::Add, left, right, width, mxcsr, 0);
        if dot_stage(lane_exceptions(status, mxcsr), mxcsr, &mut exceptions) {
            return (a, exceptions);
        }
        sum
    } else {
        left
    };
    let mut out = [0; 16];
    for lane in 0..count {
        if immediate & (1 << lane) != 0 {
            put(&mut out, lane * width, width, sum);
        }
    }
    (out, exceptions)
}

pub(super) fn floating(
    op: SimdOperation,
    a: [u8; 16],
    b: [u8; 16],
    mxcsr: u32,
    incoming_flags: u32,
) -> ([u8; 16], u32, u32) {
    let super::SimdKind::Float(kind) = op.kind else {
        unreachable!()
    };
    if matches!(kind, F::Round | F::DotProduct) {
        let (out, exceptions) = if kind == F::Round {
            round_integral(op, a, b, mxcsr)
        } else {
            dot_product(op, a, b, mxcsr)
        };
        return (out, exceptions, incoming_flags);
    }
    let width = usize::from(op.lane_bytes);
    let count = if op.scalar {
        1
    } else {
        usize::from(op.vector_bytes) / width
    };
    let mut out = a;
    let mut exceptions = 0;
    let mut flags = incoming_flags;
    for lane in 0..count {
        let offset = lane * width;
        let (kind, left, right) = match kind {
            F::AddSubtract => (
                if lane % 2 == 0 { F::Subtract } else { F::Add },
                unsigned(&a, offset, width),
                unsigned(&b, offset, width),
            ),
            F::HorizontalAdd | F::HorizontalSubtract => {
                let input = if lane < count / 2 { &a } else { &b };
                let offset = lane % (count / 2) * width * 2;
                (
                    if kind == F::HorizontalAdd {
                        F::Add
                    } else {
                        F::Subtract
                    },
                    unsigned(input, offset, width),
                    unsigned(input, offset + width, width),
                )
            }
            _ => (
                kind,
                unsigned(&a, offset, width),
                unsigned(&b, offset, width),
            ),
        };
        let (value, status, compare) =
            binary(kind, left, right, width, mxcsr, op.immediate.unwrap_or(0));
        exceptions |= lane_exceptions(status, mxcsr);
        if let Some(compare) = compare {
            flags = (incoming_flags & !0x8d5) | compare;
        } else {
            put(&mut out, offset, width, value);
        }
    }
    (out, instruction_exceptions(exceptions, mxcsr), flags)
}

pub(super) fn convert(
    op: SimdOperation,
    a: [u8; 16],
    b: [u8; 16],
    mxcsr: u32,
) -> ([u8; 16], u32, Option<u32>) {
    let super::SimdKind::Convert { kind, truncate } = op.kind else {
        unreachable!()
    };
    let round = if truncate {
        Rounding::Zero
    } else {
        rounding(mxcsr)
    };
    let mut out = if op.scalar { a } else { [0; 16] };
    let mut flags = 0;
    let (source_width, destination_width, count) = match kind {
        C::SignedDwordsToSingles => (
            4,
            4,
            if matches!(
                op.source,
                super::SimdOperand::Register(super::SimdRegister::Mmx(_))
            ) || op.memory_bytes == 8
            {
                2
            } else {
                4
            },
        ),
        C::SignedDwordsToDoubles => (4, 8, 2),
        C::SinglesToSignedDwords => (
            4,
            4,
            if matches!(
                op.destination,
                super::SimdOperand::Register(super::SimdRegister::Mmx(_))
            ) {
                2
            } else {
                4
            },
        ),
        C::DoublesToSignedDwords => (8, 4, 2),
        C::SinglesToDoubles => (4, 8, 2),
        C::DoublesToSingles => (8, 4, 2),
        C::SignedDwordToSingle => (4, 4, 1),
        C::SignedDwordToDouble => (4, 8, 1),
        C::SingleToSignedDword => (4, 4, 1),
        C::DoubleToSignedDword => (8, 4, 1),
        C::SingleToDouble => (4, 8, 1),
        C::DoubleToSingle => (8, 4, 1),
    };
    if kind == C::SignedDwordsToSingles && count == 2 {
        out = a;
    }
    for lane in 0..count {
        let raw = unsigned(&b, lane * source_width, source_width);
        let to_integer = matches!(
            kind,
            C::SinglesToSignedDwords
                | C::DoublesToSignedDwords
                | C::SingleToSignedDword
                | C::DoubleToSignedDword
        );
        let from_integer = matches!(
            kind,
            C::SignedDwordsToSingles
                | C::SignedDwordsToDoubles
                | C::SignedDwordToSingle
                | C::SignedDwordToDouble
        );
        let (value, status) = if from_integer {
            let result =
                ieee::from_signed32(raw as u32 as i32, ieee_format(destination_width), round);
            (result.bits, u32::from(result.exceptions))
        } else {
            let (raw, denormal_flag) = input(raw, source_width, mxcsr);
            if to_integer {
                let result = ieee::to_signed32(raw, ieee_format(source_width), round);
                (result.bits, u32::from(result.exceptions))
            } else if source_width == 4 {
                let result = ieee::convert(raw, BinaryFormat::Binary32, round);
                (
                    result.bits,
                    u32::from(result.exceptions) | if nan(raw, 4) { 0 } else { denormal_flag },
                )
            } else {
                let result = ieee::convert(raw, BinaryFormat::Binary64, round);
                let mut value = result.bits;
                let mut status =
                    u32::from(result.exceptions) | if nan(raw, 8) { 0 } else { denormal_flag };
                let tiny = ieee::narrowing_tiny(raw, round);
                if tiny {
                    if mxcsr & 0x800 == 0 || status & 0x20 != 0 {
                        status |= 0x10;
                    }
                    if mxcsr & 0x800 != 0 && mxcsr & 0x8000 != 0 {
                        value &= 0x8000_0000;
                        status |= 0x30;
                    }
                }
                (value, status)
            }
        };
        flags |= lane_exceptions(status, mxcsr);
        put(&mut out, lane * destination_width, destination_width, value);
    }
    let gpr = matches!(kind, C::SingleToSignedDword | C::DoubleToSignedDword)
        .then(|| unsigned(&out, 0, 4) as u32);
    (out, instruction_exceptions(flags, mxcsr), gpr)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn radical_has_exact_rounding_and_signed_zero() {
        assert_eq!(
            square_root(0x4080_0000, 4, Rounding::Nearest),
            (0x4000_0000, 0)
        );
        assert_eq!(
            square_root(0x4000_0000, 4, Rounding::Nearest),
            (0x3fb5_04f3, 32)
        );
        assert_eq!(square_root(0x8000_0000, 4, Rounding::Up), (0x8000_0000, 0));
        assert_eq!(
            square_root(0x3ff0_0000_0000_0000, 8, Rounding::Zero),
            (0x3ff0_0000_0000_0000, 0)
        );
    }
    #[test]
    fn nan_priority_and_compare_predicates_are_x86() {
        assert_eq!(
            binary(F::Add, 0x7f80_1234, 0x7fc0_5678, 4, 0x1f80, 0).0,
            0x7fc0_1234
        );
        assert_eq!(
            binary(F::Minimum, 0x7fc0_1234, 0x3f80_0000, 4, 0x1f80, 0).0,
            0x3f80_0000
        );
        assert_eq!(binary(F::Compare, 0x7fc0_1234, 0, 4, 0x1f80, 0).1, 0);
        assert_eq!(binary(F::Compare, 0x7fc0_1234, 0, 4, 0x1f80, 1).1, 1);
    }
}
