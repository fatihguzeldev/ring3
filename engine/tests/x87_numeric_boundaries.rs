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

#[test]
fn tiny_unbounded_results_report_underflow_even_when_masked_rounding_returns_minimum_normal() {
    // all inputs are exact raw extended80 values; these expectations do not use APFloat status.
    let normal = Ext80::from_parts(false, 1, 1 << 63);
    let below_one = Ext80::from_parts(false, 0x3ffe, u64::MAX);
    let mut state = X87State {
        tag: 0xfff0,
        ..X87State::default()
    };
    state.registers[0] = normal.0;
    state.registers[1] = below_one.0;
    let operation = X87Operation::Binary {
        kind: X87Binary::Multiply,
        destination: 0,
        source: 1,
        pop: false,
    };
    let masked = state.evaluate(operation, &[], 0x402).unwrap();
    assert_eq!(masked.state.stack(0), Some(normal));
    assert_eq!(masked.state.status, 0x0230);
    assert!(!masked.unmasked_exception);
    state.control = 0x036f;
    let unmasked = state.evaluate(operation, &[], 0x402).unwrap();
    assert_eq!(
        unmasked.state.stack(0),
        Some(Ext80::from_parts(false, 0x6000, u64::MAX))
    );
    assert_eq!(unmasked.state.status, 0x8090);
    assert!(unmasked.unmasked_exception);

    for (format, value, wanted) in [
        (
            ring3_engine::cpu::x86::X87Format::Float32,
            Ext80::from_parts(false, 0x3f80, 0xffff_ff00_0000_0000),
            0x0080_0000_u64.to_le_bytes()[..4].to_vec(),
        ),
        (
            ring3_engine::cpu::x86::X87Format::Float64,
            Ext80::from_parts(false, 0x3c00, 0xffff_ffff_ffff_f800),
            0x0010_0000_0000_0000_u64.to_le_bytes().to_vec(),
        ),
    ] {
        let mut state = X87State {
            tag: 0xfffc,
            ..X87State::default()
        };
        state.registers[0] = value.0;
        let operation = X87Operation::Store {
            format,
            pop: true,
            truncate: false,
        };
        let masked = state.evaluate(operation, &[], 0x402).unwrap();
        assert_eq!(masked.memory_len, wanted.len());
        assert_eq!(&masked.memory[..masked.memory_len], wanted.as_slice());
        assert_eq!(masked.state.status, 0x0a30);
        assert!(!masked.unmasked_exception);
        state.control = 0x036f;
        let unmasked = state.evaluate(operation, &[], 0x402).unwrap();
        assert_eq!(unmasked.memory_len, 0);
        assert_eq!(unmasked.state.stack(0), Some(value));
        assert_eq!(unmasked.state.status, 0x8090);
        assert!(unmasked.unmasked_exception);
    }
}

#[test]
fn scale_infinite_exponents_follow_the_architectural_class_table() {
    let infinity = Ext80::from_parts(false, 0x7fff, 1 << 63);
    for negative in [false, true] {
        let signed = |value: Ext80| if negative { value.negated() } else { value };
        for (value, scale, wanted, status) in [
            (signed(Ext80::ONE), infinity, signed(infinity), 0),
            (
                signed(Ext80::ONE),
                infinity.negated(),
                signed(Ext80::ZERO),
                0,
            ),
            (signed(Ext80::ZERO), infinity, Ext80::INDEFINITE, 1),
            (
                signed(Ext80::ZERO),
                infinity.negated(),
                signed(Ext80::ZERO),
                0,
            ),
            (signed(infinity), infinity, signed(infinity), 0),
            (signed(infinity), infinity.negated(), Ext80::INDEFINITE, 1),
        ] {
            let mut state = X87State {
                tag: 0xfff0,
                ..X87State::default()
            };
            state.registers[0] = value.0;
            state.registers[1] = scale.0;
            let outcome = state.evaluate(X87Operation::Scale, &[], 0x402).unwrap();
            assert_eq!(outcome.state.stack(0), Some(wanted));
            assert_eq!(outcome.state.stack(1), Some(scale));
            assert_eq!(outcome.state.status, status);
            assert!(!outcome.unmasked_exception);
            if status == 1 {
                state.control = 0x037e;
                let unmasked = state.evaluate(X87Operation::Scale, &[], 0x402).unwrap();
                assert_eq!(unmasked.state.stack(0), Some(value));
                assert_eq!(unmasked.state.status, 0x8081);
                assert!(unmasked.unmasked_exception);
            }
        }
    }
    let quiet = Ext80::from_parts(false, 0x7fff, 0xc000_0000_0000_0123);
    let mut state = X87State {
        control: 0x037d,
        tag: 0xfff0,
        ..X87State::default()
    };
    state.registers[0] = quiet.0;
    state.registers[1] = Ext80::from_parts(false, 0, 1).0;
    let outcome = state.evaluate(X87Operation::Scale, &[], 0x402).unwrap();
    assert_eq!(outcome.state.stack(0), Some(quiet));
    assert_eq!(outcome.state.status, 0);
    assert!(!outcome.unmasked_exception);
}

#[test]
fn extract_quiets_nan_payloads_and_reports_denormal_before_stack_publication() {
    let quiet = Ext80::from_parts(false, 0x7fff, 0xc000_0000_0000_0123);
    for (value, wanted, status) in [
        (quiet, quiet, 0x3800),
        (
            Ext80::from_parts(false, 0x7fff, 0x8000_0000_0000_0123),
            quiet,
            0x3801,
        ),
        (
            Ext80::from_parts(false, 0x4000, 1),
            Ext80::INDEFINITE,
            0x3801,
        ),
    ] {
        let mut state = X87State {
            tag: 0xfffc,
            ..X87State::default()
        };
        state.registers[0] = value.0;
        let outcome = state.evaluate(X87Operation::Extract, &[], 0x402).unwrap();
        assert_eq!(outcome.state.stack(0), Some(wanted));
        assert_eq!(outcome.state.stack(1), Some(wanted));
        assert_eq!(outcome.state.status, status);
        assert!(!outcome.unmasked_exception);
    }
    let minimum = Ext80::from_parts(false, 0, 1);
    let mut state = X87State {
        tag: 0xfffc,
        ..X87State::default()
    };
    state.registers[0] = minimum.0;
    let masked = state.evaluate(X87Operation::Extract, &[], 0x402).unwrap();
    assert_eq!(masked.state.stack(0), Some(Ext80::ONE));
    assert_eq!(
        masked.state.stack(1),
        Some(Ext80::from_parts(true, 0x400d, 0x807a_0000_0000_0000))
    );
    assert_eq!(masked.state.status, 0x3802);
    state.control = 0x037d;
    let unmasked = state.evaluate(X87Operation::Extract, &[], 0x402).unwrap();
    assert_eq!(unmasked.state.stack(0), Some(minimum));
    assert_eq!(unmasked.state.top(), 0);
    assert_eq!(unmasked.state.status, 0x8082);
    assert!(unmasked.unmasked_exception);
}
