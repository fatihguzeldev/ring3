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

#[test]
fn selected_fs_gs_offsets_and_current_code_selector_survive_environment_and_fxsave() {
    use ring3_engine::{
        abi::{
            arena::{TRANSFER_OFFSET, X87_OFFSET},
            x86::{X87_SIZE, decode_x87},
        },
        cpu::x86::{
            SegmentDescriptor, SegmentDescriptorKind, SegmentRegister, X87Format,
            ir::EffectiveAddress,
            simd::{SimdKind, SimdOperand, SimdOperation},
        },
        memory::GuestAddress,
        process::{EngineInstance, HostError},
    };
    let mut engine = EngineInstance::new(3, 0x1234_5678_9abc_def0).unwrap();
    for (base, permission) in [(0x1000, 7), (0x5000, 3), (0x7000, 3)] {
        engine.map(base, 1, permission).unwrap();
    }
    engine
        .set_segment_base(SegmentRegister::Fs, 0x5000)
        .unwrap();
    engine
        .configure_segment_descriptor(SegmentDescriptor {
            selector: 0x43,
            base: 0x7000,
            dpl: 3,
            present: true,
            kind: SegmentDescriptorKind::Data { writable: true },
        })
        .unwrap();
    for address in [0x5014, 0x7028] {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
            .copy_from_slice(&7_u32.to_le_bytes());
        engine.upload(address, 4).unwrap();
    }
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    request[4..].copy_from_slice(&2_u32.to_le_bytes());
    engine.compile(1).unwrap();
    engine.segment_load(5, 0x43).unwrap();
    let save = SimdOperation {
        kind: SimdKind::SaveExtended,
        destination: SimdOperand::Memory(EffectiveAddress::default()),
        source: SimdOperand::None,
        vector_bytes: 4,
        lane_bytes: 0,
        memory_bytes: 512,
        immediate: None,
        aligned: true,
        scalar: false,
        implicit_address: None,
    };
    for (address, control, expected_offset, expected_selector) in [
        (0x5014, 0x501, 0x14_u32, 0x3b_u16),
        (0x7028, 0x601, 0x28, 0x43),
        // old callers with only the waiting bit continue to mean DS.
        (0x5014, 1, 0x5014, 0x23),
    ] {
        engine
            .x87_op(X87Operation::Initialize.encode(), 0, 0x402, 0, 0, 0)
            .unwrap();
        engine
            .x87_op(
                X87Operation::Load(X87Format::Integer32).encode(),
                address,
                0x402,
                0x1234_5678,
                0x503,
                control,
            )
            .unwrap();
        let state = decode_x87(&engine.arena()[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap();
        assert_eq!(state.instruction_pointer, 0x1234_5678);
        assert_eq!(state.code_selector, 0x1b);
        assert_eq!(state.data_pointer, expected_offset);
        assert_eq!(state.data_selector, expected_selector);
        engine
            .x87_op(
                X87Operation::StoreEnvironment { operand16: false }.encode(),
                0x5200,
                0x402,
                0x8765_4321,
                0x137,
                0x401,
            )
            .unwrap();
        let mut environment = [0; 28];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x5200), &mut environment)
            .unwrap();
        assert_eq!(&environment[12..16], &0x1234_5678_u32.to_le_bytes());
        assert_eq!(&environment[16..18], &0x1b_u16.to_le_bytes());
        assert_eq!(&environment[18..20], &0x503_u16.to_le_bytes());
        assert_eq!(&environment[20..24], &expected_offset.to_le_bytes());
        assert_eq!(&environment[24..26], &expected_selector.to_le_bytes());
        let (descriptor, operands, immediate) = save.descriptor();
        engine
            .simd_execute(descriptor, operands, immediate, 0, 0, 0x402)
            .unwrap();
        engine.cpu_fxsave(0x5500).unwrap();
        let mut image = [0; 512];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x5500), &mut image)
            .unwrap();
        assert_eq!(&image[8..12], &0x1234_5678_u32.to_le_bytes());
        assert_eq!(&image[12..14], &0x1b_u16.to_le_bytes());
        assert_eq!(&image[16..20], &expected_offset.to_le_bytes());
        assert_eq!(&image[20..22], &expected_selector.to_le_bytes());
    }
    for control in [2, 0x80, 0x700, 0x800, 0x8000_0000] {
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.x87_op(X87Operation::Wait.encode(), 0, 0x402, 0, 0, control),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), before);
    }
}

#[test]
fn packed_bcd_zero_retains_its_sign_across_load_and_store_pop() {
    use ring3_engine::cpu::x86::X87Format;
    for negative in [false, true] {
        let mut input = [0; 10];
        input[9] = if negative { 0x80 } else { 0 };
        let loaded = X87State::default()
            .evaluate(X87Operation::Load(X87Format::PackedBcd), &input, 0x402)
            .unwrap();
        assert_eq!(
            loaded.state.stack(0),
            Some(if negative {
                Ext80::ZERO.negated()
            } else {
                Ext80::ZERO
            })
        );
        assert_eq!(loaded.state.status, 0x3800);
        assert_eq!(loaded.state.tag, 0x7fff);
        assert!(!loaded.unmasked_exception);
        let stored = loaded
            .state
            .evaluate(
                X87Operation::Store {
                    format: X87Format::PackedBcd,
                    pop: true,
                    truncate: false,
                },
                &[],
                0x402,
            )
            .unwrap();
        assert_eq!(stored.memory_len, 10);
        assert_eq!(&stored.memory[..10], &input);
        assert_eq!(stored.state.status, 0);
        assert_eq!(stored.state.tag, 0xffff);
        assert!(!stored.unmasked_exception);
    }
}
