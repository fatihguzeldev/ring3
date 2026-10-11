use super::packed::{put, unsigned};
use super::{ConvertKind as C, FloatKind as F, SimdOperation};
use rustc_apfloat::{
    Float, FloatConvert, Round, Status,
    ieee::{Double, IeeeFloat, Semantics, Single},
};

fn rounding(mxcsr: u32) -> Round {
    match (mxcsr >> 13) & 3 {
        0 => Round::NearestTiesToEven,
        1 => Round::TowardNegative,
        2 => Round::TowardPositive,
        _ => Round::TowardZero,
    }
}
fn exceptions(status: Status) -> u32 {
    u32::from(status.contains(Status::INVALID_OP))
        | (u32::from(status.contains(Status::DIV_BY_ZERO)) << 2)
        | (u32::from(status.contains(Status::OVERFLOW)) << 3)
        | (u32::from(status.contains(Status::UNDERFLOW)) << 4)
        | (u32::from(status.contains(Status::INEXACT)) << 5)
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
    if width == 4 {
        (0x8000_0000, 0x7f80_0000, 0x007f_ffff, 23, 127)
    } else {
        (
            0x8000_0000_0000_0000,
            0x7ff0_0000_0000_0000,
            0x000f_ffff_ffff_ffff,
            52,
            1023,
        )
    }
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
fn root(mut n: u128) -> (u128, u128) {
    let mut result = 0;
    let mut bit = 1u128 << 126;
    while bit > n {
        bit >>= 2;
    }
    while bit != 0 {
        if n >= result + bit {
            n -= result + bit;
            result = (result >> 1) + bit;
        } else {
            result >>= 1;
        }
        bit >>= 2;
    }
    (result, n)
}

struct WideSingle;
impl Semantics for WideSingle {
    const BITS: usize = 39;
    const EXP_BITS: usize = 15;
}
struct WideDouble;
impl Semantics for WideDouble {
    const BITS: usize = 68;
    const EXP_BITS: usize = 15;
}

fn unbounded_tiny(kind: F, a: u64, b: u64, width: usize, round: Round) -> bool {
    if !matches!(kind, F::Add | F::Subtract | F::Multiply | F::Divide) {
        return false;
    }
    let mut loses = false;
    if width == 4 {
        let x: IeeeFloat<WideSingle> = Single::from_bits(a as u128)
            .convert_r(round, &mut loses)
            .value;
        let y: IeeeFloat<WideSingle> = Single::from_bits(b as u128)
            .convert_r(round, &mut loses)
            .value;
        let value = match kind {
            F::Add => x.add_r(y, round),
            F::Subtract => x.sub_r(y, round),
            F::Multiply => x.mul_r(y, round),
            F::Divide => x.div_r(y, round),
            _ => unreachable!(),
        }
        .value;
        value.is_finite_non_zero() && value.ilogb() < -126
    } else {
        let x: IeeeFloat<WideDouble> = Double::from_bits(a as u128)
            .convert_r(round, &mut loses)
            .value;
        let y: IeeeFloat<WideDouble> = Double::from_bits(b as u128)
            .convert_r(round, &mut loses)
            .value;
        let value = match kind {
            F::Add => x.add_r(y, round),
            F::Subtract => x.sub_r(y, round),
            F::Multiply => x.mul_r(y, round),
            F::Divide => x.div_r(y, round),
            _ => unreachable!(),
        }
        .value;
        value.is_finite_non_zero() && value.ilogb() < -1022
    }
}

// integer radical rounding retains the exact remainder; no host floating point is used.
fn square_root(bits: u64, width: usize, round: Round) -> (u64, u32) {
    let (sign, exponent_mask, fraction_mask, fraction_bits, bias) = format(width);
    if nan(bits, width) {
        return (quiet(bits, width), u32::from(signaling(bits, width)));
    }
    if bits & !sign == 0 || bits == exponent_mask {
        return (bits, 0);
    }
    if bits & sign != 0 {
        return (indefinite(width), 1);
    }
    let mut exponent = ((bits & exponent_mask) >> fraction_bits) as i32 - bias;
    let mut significand = bits & fraction_mask;
    if bits & exponent_mask != 0 {
        significand |= 1 << fraction_bits;
    } else {
        exponent = 1 - bias;
        while significand & (1 << fraction_bits) == 0 {
            significand <<= 1;
            exponent -= 1;
        }
    }
    if exponent & 1 != 0 {
        significand <<= 1;
        exponent -= 1;
    }
    let radicand = u128::from(significand) << fraction_bits;
    let (mut result, remainder) = root(radicand);
    let inexact = remainder != 0;
    let increment = match round {
        Round::NearestTiesToEven | Round::NearestTiesToAway => remainder > result,
        Round::TowardPositive => inexact,
        Round::TowardNegative | Round::TowardZero => false,
    };
    result += u128::from(increment);
    exponent /= 2;
    if result >= 1u128 << (fraction_bits + 1) {
        result >>= 1;
        exponent += 1;
    }
    (
        ((exponent + bias) as u64) << fraction_bits | (result as u64 & fraction_mask),
        u32::from(inexact) << 5,
    )
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
            square_root(b, width, Round::NearestTiesToEven).0
        } else {
            b
        };
        let (value, _) = if width == 4 {
            let r = Single::from_bits(0x3f80_0000).div_r(
                Single::from_bits(denominator as u128),
                Round::NearestTiesToEven,
            );
            (r.value.to_bits() as u64, exceptions(r.status))
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
    } else if width == 4 {
        let x = Single::from_bits(a as u128);
        let y = Single::from_bits(b as u128);
        let r = match kind {
            F::Add => x.add_r(y, round),
            F::Subtract => x.sub_r(y, round),
            F::Multiply => x.mul_r(y, round),
            F::Divide => x.div_r(y, round),
            _ => unreachable!(),
        };
        (r.value.to_bits() as u64, exceptions(r.status))
    } else {
        let x = Double::from_bits(a as u128);
        let y = Double::from_bits(b as u128);
        let r = match kind {
            F::Add => x.add_r(y, round),
            F::Subtract => x.sub_r(y, round),
            F::Multiply => x.mul_r(y, round),
            F::Divide => x.div_r(y, round),
            _ => unreachable!(),
        };
        (r.value.to_bits() as u64, exceptions(r.status))
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
        let (value, status, compare) = binary(
            kind,
            unsigned(&a, offset, width),
            unsigned(&b, offset, width),
            width,
            mxcsr,
            op.immediate.unwrap_or(0),
        );
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
        Round::TowardZero
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
            if destination_width == 4 {
                let r = Single::from_i128_r(raw as u32 as i32 as i128, round);
                (r.value.to_bits() as u64, exceptions(r.status))
            } else {
                let r = Double::from_i128_r(raw as u32 as i32 as i128, round);
                (r.value.to_bits() as u64, exceptions(r.status))
            }
        } else {
            let (raw, denormal_flag) = input(raw, source_width, mxcsr);
            if to_integer {
                let mut exact = false;
                let r = if source_width == 4 {
                    Single::from_bits(raw as u128).to_i128_r(32, round, &mut exact)
                } else {
                    Double::from_bits(raw as u128).to_i128_r(32, round, &mut exact)
                };
                (
                    if r.status.contains(Status::INVALID_OP) {
                        0x8000_0000
                    } else {
                        r.value as u64
                    },
                    exceptions(r.status),
                )
            } else if source_width == 4 {
                let mut loses = false;
                let r: rustc_apfloat::StatusAnd<Double> =
                    Single::from_bits(raw as u128).convert_r(round, &mut loses);
                (
                    r.value.to_bits() as u64,
                    exceptions(r.status) | if nan(raw, 4) { 0 } else { denormal_flag },
                )
            } else {
                let mut loses = false;
                let r: rustc_apfloat::StatusAnd<Single> =
                    Double::from_bits(raw as u128).convert_r(round, &mut loses);
                let mut value = r.value.to_bits() as u64;
                let mut status = exceptions(r.status) | if nan(raw, 8) { 0 } else { denormal_flag };
                let unbounded: IeeeFloat<WideSingle> = Double::from_bits(raw as u128)
                    .convert_r(round, &mut loses)
                    .value;
                let tiny = unbounded.is_finite_non_zero() && unbounded.ilogb() < -126;
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
            square_root(0x4080_0000, 4, Round::NearestTiesToEven),
            (0x4000_0000, 0)
        );
        assert_eq!(
            square_root(0x4000_0000, 4, Round::NearestTiesToEven),
            (0x3fb5_04f3, 32)
        );
        assert_eq!(
            square_root(0x8000_0000, 4, Round::TowardPositive),
            (0x8000_0000, 0)
        );
        assert_eq!(
            square_root(0x3ff0_0000_0000_0000, 8, Round::TowardZero),
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
