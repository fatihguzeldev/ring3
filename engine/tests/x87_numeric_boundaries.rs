use ring3_engine::cpu::x86::{Ext80, X87Binary, X87Operation, X87State};
use rustc_apfloat::{Float, Round, ieee::X87DoubleExtended};

fn bits(value: Ext80) -> u128 {
    u128::from(value.significand())
        | (u128::from(u16::from_le_bytes([value.0[8], value.0[9]])) << 64)
}

#[test]
fn extended_range_and_subnormal_rounding_match_independent_apfloat() {
    let minimum = Ext80::from_parts(false, 0, 1);
    let normal = Ext80::from_parts(false, 1, 1 << 63);
    let maximum = Ext80::from_parts(false, 0x7ffe, u64::MAX);
    let half = Ext80::from_parts(false, 0x3ffe, 1 << 63);
    let two = Ext80::from_parts(false, 0x4000, 1 << 63);
    let pairs = [
        (minimum, half),
        (normal, half),
        (maximum, two),
        (maximum, maximum),
        (normal, minimum.negated()),
        (minimum.negated(), half),
        (Ext80::from_parts(false, 1, (1 << 63) + 1), half),
        (Ext80::from_parts(false, 0, (1 << 63) - 1), two),
    ];
    for (a, b) in pairs {
        for (control, round) in [
            (0x037f, Round::NearestTiesToEven),
            (0x077f, Round::TowardNegative),
            (0x0b7f, Round::TowardPositive),
            (0x0f7f, Round::TowardZero),
        ] {
            let av = X87DoubleExtended::from_bits(bits(a));
            let bv = X87DoubleExtended::from_bits(bits(b));
            for (kind, expected) in [
                (X87Binary::Add, av.add_r(bv, round)),
                (X87Binary::Multiply, av.mul_r(bv, round)),
                (X87Binary::Divide, av.div_r(bv, round)),
            ] {
                let mut state = X87State {
                    control,
                    tag: 0xfff0,
                    ..X87State::default()
                };
                state.registers[0] = a.0;
                state.registers[1] = b.0;
                let outcome = state
                    .evaluate(
                        X87Operation::Binary {
                            kind,
                            destination: 0,
                            source: 1,
                            pop: false,
                        },
                        &[],
                        0x402,
                    )
                    .unwrap();
                assert_eq!(
                    bits(outcome.state.stack(0).unwrap()),
                    expected.value.to_bits(),
                    "{kind:?} {a:?} {b:?} CW={control:x}"
                );
                assert!(!outcome.unmasked_exception);
            }
        }
    }
}

#[test]
fn quiet_nan_precedes_other_operand_denormal_for_compare_and_transcendental() {
    let qnan = Ext80::from_parts(false, 0x7fff, 0xc000_0000_0000_0123);
    let denormal = Ext80::from_parts(false, 0, 1);
    let mut state = X87State {
        control: 0x037d,
        tag: 0xfff8,
        ..X87State::default()
    };
    state.registers[0] = qnan.0;
    state.registers[1] = denormal.0;
    let compared = state
        .evaluate(
            X87Operation::Compare {
                index: 1,
                quiet: true,
                pop: 0,
                integer_flags: false,
            },
            &[],
            0x402,
        )
        .unwrap();
    assert!(!compared.unmasked_exception);
    assert_eq!(compared.state.status, 0x4500);
    let atan = state
        .evaluate(X87Operation::PartialAtan, &[], 0x402)
        .unwrap();
    assert!(!atan.unmasked_exception);
    assert_eq!(atan.state.status, 0x0800);
    assert_eq!(atan.state.stack(0), Some(qnan));
}
