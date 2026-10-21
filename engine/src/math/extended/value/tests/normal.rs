use super::*;
use rustc_apfloat::{
    Float, Round, Status,
    ieee::{IeeeFloat, Semantics},
};

struct Pc24;
impl Semantics for Pc24 {
    const BITS: usize = 39;
    const EXP_BITS: usize = 15;
}

type Oracle = IeeeFloat<Pc24>;

#[derive(Clone, Copy, Debug)]
enum Operation {
    Add,
    Multiply,
    Divide,
}

const MODES: [(u16, Round); 4] = [
    (0, Round::NearestTiesToEven),
    (0x0400, Round::TowardNegative),
    (0x0800, Round::TowardPositive),
    (0x0c00, Round::TowardZero),
];

fn oracle_bits(value: Ext80) -> u128 {
    (u128::from(value.negative()) << 38)
        | (u128::from(value.exponent_bits()) << 23)
        | u128::from((value.significand() >> 40) & 0x7f_ffff)
}

fn extended(bits: u128) -> Ext80 {
    let exponent = ((bits >> 23) & 0x7fff) as u16;
    let significand = ((bits as u64 & 0x7f_ffff) | if exponent != 0 { 0x80_0000 } else { 0 }) << 40;
    Ext80::from_parts(bits >> 38 != 0, exponent, significand)
}

fn oracle(operation: Operation, a: Ext80, b: Ext80, rounding: Round) -> (Rounded, Status) {
    let a = Oracle::from_bits(oracle_bits(a));
    let b = Oracle::from_bits(oracle_bits(b));
    let calculate = |rounding| match operation {
        Operation::Add => a.add_r(b, rounding),
        Operation::Multiply => a.mul_r(b, rounding),
        Operation::Divide => a.div_r(b, rounding),
    };
    let result = calculate(rounding);
    let truncated = calculate(Round::TowardZero);
    let magnitude = (1_u128 << 38) - 1;
    (
        Rounded {
            value: extended(result.value.to_bits()),
            exceptions: if result.status.contains(Status::INEXACT) {
                PRECISION
            } else {
                0
            },
            incremented: result.value.to_bits() & magnitude > truncated.value.to_bits() & magnitude,
        },
        result.status,
    )
}

fn check(a: Ext80, b: Ext80) {
    for (rounding_bits, rounding) in MODES {
        for operation in [Operation::Add, Operation::Multiply, Operation::Divide] {
            let (expected, status) = oracle(operation, a, b, rounding);
            if status.intersects(Status::OVERFLOW | Status::UNDERFLOW)
                || expected.value.exponent_bits() >= 0x7ffe
            {
                continue;
            }
            for masks in [0x007f, 0x005f] {
                let control = masks | rounding_bits;
                let actual = match operation {
                    Operation::Add => normal::add(a, b, control),
                    Operation::Multiply => normal::multiply(a, b, control),
                    Operation::Divide => normal::divide(a, b, control),
                }
                .unwrap_or_else(|| panic!("declined {operation:?} a={a:?} b={b:?} CW={control:x}"));
                assert_eq!(
                    (actual.value, actual.exceptions, actual.incremented),
                    (expected.value, expected.exceptions, expected.incremented),
                    "{operation:?} a={a:?} b={b:?} CW={control:x}"
                );
            }
        }
    }
}

#[test]
fn normal_pc24_arithmetic_matches_independent_extended_exponent_apfloat() {
    for (a_sig, a_exp, b_sig, b_exp) in [
        (0x80_0000, 0, 0x80_0000, 0),
        (0x80_0000, 0, 0x80_0000, -24),
        (0x80_0001, 0, 0x80_0000, -24),
        (0x80_0000, 0, 0x80_0001, -24),
        (0x80_0000, 0, 0xff_ffff, -25),
        (0xff_ffff, 0, 0x80_0000, -24),
        (0x80_0001, 0, 0x80_0000, 0),
        (0x80_0000, 0, 0x80_0000, -65),
        (0xff_ffff, 0, 0x80_0001, -65),
        (0x80_0000, 0, 0xc0_0000, 0),
        (0xff_ffff, 0, 0x80_0000, -1),
        (0x80_0001, 0, 0xff_ffff, 0),
    ] {
        for scale in [-16000, 0, 16000] {
            for negative_a in [false, true] {
                for negative_b in [false, true] {
                    check(
                        Ext80::from_parts(negative_a, (16383 + scale + a_exp) as u16, a_sig << 40),
                        Ext80::from_parts(negative_b, (16383 + scale + b_exp) as u16, b_sig << 40),
                    );
                }
            }
        }
    }
    let mut seed = 0xa076_1d64_78bd_642f_u64;
    for _ in 0..128 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let a = Ext80::from_parts(
            seed & 1 != 0,
            100 + (seed % 32500) as u16,
            ((seed >> 40) | 0x80_0000) << 40,
        );
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let b = Ext80::from_parts(
            seed & 1 != 0,
            100 + (seed % 32500) as u16,
            ((seed >> 40) | 0x80_0000) << 40,
        );
        check(a, b);
    }
}

#[test]
fn normal_pc24_arithmetic_declines_wider_operands_other_precisions_and_specials() {
    for operation in [normal::add, normal::multiply, normal::divide] {
        for control in [0x017f, 0x027f, 0x037f, 0x0f7f] {
            assert!(operation(Ext80::ONE, Ext80::ONE, control).is_none());
        }
        for value in [
            Ext80::from_parts(false, 0x3fff, (1 << 63) | 1),
            Ext80::from_parts(false, 0, 1 << 63),
            Ext80::from_parts(false, 0, 1),
            Ext80::from_parts(false, 0x3fff, 1 << 62),
            Ext80::from_parts(false, 0x7fff, 1 << 63),
            Ext80::from_parts(false, 0x7fff, 0xc000_0000_0000_0123),
            Ext80::ZERO,
        ] {
            for (a, b) in [(value, Ext80::ONE), (Ext80::ONE, value)] {
                assert!(operation(a, b, 0x007f).is_none(), "a={a:?} b={b:?}");
            }
        }
    }
}
