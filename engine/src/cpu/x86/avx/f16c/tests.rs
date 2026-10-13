use super::*;
use crate::cpu::x86::simd::SimdState;

fn register(index: u8) -> VectorRegister {
    VectorRegister::new(index).unwrap()
}
fn state() -> VectorState {
    let mut state = VectorState {
        simd: SimdState::default(),
        upper: [[0xa5; 16]; 8],
    };
    for register in 0..8 {
        state.simd.xmm[register] = [0x81 + register as u8; 16];
    }
    state
}
fn source(state: &mut VectorState, register: VectorRegister, values: &[u32]) {
    let mut bytes = [0; 32];
    for (lane, value) in values.iter().enumerate() {
        bytes[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    state.publish(register, bytes);
}
fn result(
    mut before: VectorState,
    destination: VectorRegister,
    values: &[u16],
    mxcsr: u32,
) -> VectorState {
    let mut bytes = [0; 32];
    for (lane, value) in values.iter().enumerate() {
        bytes[lane * 2..lane * 2 + 2].copy_from_slice(&value.to_le_bytes());
    }
    before.publish(destination, bytes);
    before.simd.mxcsr = mxcsr;
    before
}
fn narrow(
    width: VectorWidth,
    source: u8,
    destination: F16cTarget,
    immediate: u8,
) -> F16cInstruction {
    F16cInstruction::Narrow {
        width,
        source: register(source),
        destination,
        immediate,
    }
}

#[test]
fn literal_hardware_counterexamples_keep_generic_masked_underflow_and_rounding() {
    // retained single-model oracle: hardware-oracle/response.json, intel signature606a6.
    for (bits, immediate, before_csr, half, after_csr) in [
        (0x0000_0000, 0, 0x1f80, 0x0000, 0x1f80),
        (0x3380_0000, 0, 0x1f80, 0x0001, 0x1f80),
        (0x3380_0000, 0, 0x9f80, 0x0001, 0x9f80),
        (0xb380_0000, 0, 0x1f80, 0x8001, 0x1f80),
        (0x3300_0000, 0, 0x1f80, 0x0000, 0x1fb0),
        (0x3380_0001, 0, 0x1f80, 0x0001, 0x1fb0),
        (0x4780_0000, 0, 0x1f80, 0x7c00, 0x1fa8),
        (0x4780_0000, 3, 0x1f80, 0x7bff, 0x1fa8),
        (0x4780_0000, 4, 0x7f80, 0x7bff, 0x7fa8),
    ] {
        let mut vector = state();
        source(&mut vector, register(1), &[bits, 0, 0, 0]);
        vector.simd.mxcsr = before_csr;
        let before = vector;
        vector
            .execute_f16c(
                narrow(
                    VectorWidth::Xmm,
                    1,
                    F16cTarget::Register(register(0)),
                    immediate,
                ),
                &mut [],
            )
            .unwrap();
        assert_eq!(
            vector,
            result(before, register(0), &[half, 0, 0, 0], after_csr),
            "{bits:08x}"
        );
    }
}

#[test]
fn measured_unmasked_exceptions_preserve_every_destination_and_operand_byte() {
    for (bits, before_csr, after_csr) in [
        (0x3380_0000, 0x1780, 0x1790),
        (0x3300_0000, 0x1780, 0x1790),
        (0x3300_0000, 0x0f80, 0x0fb0),
        (0x0000_0001, 0x1780, 0x17b2),
        (0x0000_0001, 0x1e80, 0x1e82),
        (0x4780_0000, 0x1b80, 0x1b88),
        (0x7f80_0001, 0x1f00, 0x1f01),
    ] {
        for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
            for destination in [
                F16cTarget::Register(register(0)),
                F16cTarget::Register(register(1)),
                F16cTarget::Memory,
            ] {
                let mut vector = state();
                source(&mut vector, register(1), &[bits, 0, 0, 0, 0, 0, 0, 0]);
                vector.simd.mxcsr = before_csr;
                let mut expected = vector;
                expected.simd.mxcsr = after_csr;
                let mut operand = vec![
                    0x6a;
                    if destination == F16cTarget::Memory {
                        width.bytes() / 2
                    } else {
                        0
                    }
                ];
                let before_operand = operand.clone();
                assert_eq!(
                    vector.execute_f16c(narrow(width, 1, destination, 0), &mut operand),
                    Err(F16cError::FloatingPoint(after_csr))
                );
                assert_eq!(vector, expected, "{bits:08x}");
                assert_eq!(operand, before_operand);
            }
        }
    }
}

#[test]
fn all_immediates_select_mxcsr_or_literal_rounding_and_ignore_high_bits() {
    for immediate in 0..=255_u8 {
        for mode in 0..4 {
            let control = if immediate & 4 != 0 {
                mode
            } else {
                u32::from(immediate & 3)
            };
            let positive = [0x3c00, 0x3c00, 0x3c01, 0x3c00][control as usize];
            let negative = [0xbc00, 0xbc01, 0xbc00, 0xbc00][control as usize];
            let mut vector = state();
            source(&mut vector, register(1), &[0x3f80_1000, 0xbf80_1000, 0, 0]);
            vector.simd.mxcsr = 0x1f80 | mode << 13;
            let before = vector;
            vector
                .execute_f16c(
                    narrow(
                        VectorWidth::Xmm,
                        1,
                        F16cTarget::Register(register(0)),
                        immediate,
                    ),
                    &mut [],
                )
                .unwrap();
            assert_eq!(
                vector,
                result(
                    before,
                    register(0),
                    &[positive, negative, 0, 0],
                    (0x1f80 | mode << 13) | 0x20
                )
            );
        }
    }
}

#[test]
fn every_register_alias_and_vector_width_uses_original_source_and_clears_unused_lanes() {
    let halves: [u16; 8] = [
        0x3c00, 0xc000, 0x0001, 0x0400, 0x7bff, 0x8000, 0x7c00, 0xfc00,
    ];
    let singles = [
        0x3f80_0000,
        0xc000_0000,
        0x3380_0000,
        0x3880_0000,
        0x477f_e000,
        0x8000_0000,
        0x7f80_0000,
        0xff80_0000,
    ];
    for d in 0..8 {
        for a in 0..8 {
            for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
                let mut vector = state();
                source(&mut vector, register(a), &singles);
                let before = vector;
                vector
                    .execute_f16c(
                        narrow(width, a, F16cTarget::Register(register(d)), 0),
                        &mut [],
                    )
                    .unwrap();
                assert_eq!(
                    vector,
                    result(before, register(d), &halves[..width.bytes() / 4], 0x1f80)
                );
                let mut input = [0; 32];
                for (lane, value) in halves.iter().enumerate() {
                    input[lane * 2..lane * 2 + 2].copy_from_slice(&value.to_le_bytes());
                }
                vector = state();
                vector.publish(register(a), input);
                let mut expected = vector;
                let mut output = [0; 32];
                for (lane, value) in singles.iter().enumerate().take(width.bytes() / 4) {
                    output[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
                }
                expected.publish(register(d), output);
                vector
                    .execute_f16c(
                        F16cInstruction::Widen {
                            width,
                            destination: register(d),
                            source: MoveSource::Register(register(a)),
                        },
                        &mut [],
                    )
                    .unwrap();
                assert_eq!(vector, expected);
            }
        }
    }
}

#[test]
fn daz_is_honored_only_for_single_inputs_and_ftz_never_discards_half_results() {
    for csr in [0x1f80, 0x1fc0, 0x9f80, 0x9fc0] {
        let mut vector = state();
        source(
            &mut vector,
            register(1),
            &[1, 0x8000_0001, 0x3380_0000, 0xb380_0000],
        );
        vector.simd.mxcsr = csr;
        let before = vector;
        vector
            .execute_f16c(
                narrow(VectorWidth::Xmm, 1, F16cTarget::Register(register(0)), 0),
                &mut [],
            )
            .unwrap();
        assert_eq!(
            vector,
            result(
                before,
                register(0),
                &[0, 0x8000, 1, 0x8001],
                csr | if csr & 0x40 != 0 { 0 } else { 0x32 }
            )
        );
        let mut operand = [1, 0, 1, 0x80, 1, 0x7c, 0x55, 0xfe];
        vector = state();
        vector.simd.mxcsr = csr;
        let mut expected = vector;
        let singles: [u32; 4] = [0x3380_0000, 0xb380_0000, 0x7fc0_2000, 0xffca_a000];
        let mut output = [0; 32];
        for (lane, value) in singles.iter().enumerate() {
            output[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        expected.publish(register(0), output);
        expected.simd.mxcsr |= 1;
        vector
            .execute_f16c(
                F16cInstruction::Widen {
                    width: VectorWidth::Xmm,
                    destination: register(0),
                    source: MoveSource::Memory,
                },
                &mut operand,
            )
            .unwrap();
        assert_eq!(vector, expected);
        assert_eq!(operand, [1, 0, 1, 0x80, 1, 0x7c, 0x55, 0xfe]);
    }
}

#[test]
fn packed_precomputation_exceptions_suppress_other_lanes_postcomputation() {
    for (csr, expected) in [(0x1f00, 0x1f03), (0x1e80, 0x1e83)] {
        let mut vector = state();
        source(
            &mut vector,
            register(1),
            &[0x7f80_0001, 0x3300_0000, 1, 0x4780_0000],
        );
        vector.simd.mxcsr = csr;
        let mut before = vector;
        before.simd.mxcsr = expected;
        assert_eq!(
            vector.execute_f16c(
                narrow(VectorWidth::Xmm, 1, F16cTarget::Register(register(0)), 0),
                &mut []
            ),
            Err(F16cError::FloatingPoint(expected))
        );
        assert_eq!(vector, before);
    }
}

#[test]
fn exact_memory_widths_neutral_host_refusals_and_memory_destination_preserve_vectors() {
    for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
        let operation = narrow(width, 1, F16cTarget::Memory, 0);
        assert_eq!(
            operation.memory(),
            Some(MemoryRequirement {
                access: Access::Write,
                bytes: (width.bytes() / 2) as u8,
                alignment: 1
            })
        );
        let mut vector = state();
        source(&mut vector, register(1), &[0x3f80_0000; 8]);
        let before = vector;
        let mut bytes = vec![0x77; width.bytes() / 2];
        vector.execute_f16c(operation, &mut bytes).unwrap();
        assert_eq!(vector, before);
        assert_eq!(bytes, [0, 0x3c].repeat(width.bytes() / 4));
        for length in [0, width.bytes() / 2 - 1, width.bytes() / 2 + 1, 32] {
            let mut vector = before;
            let mut operand = vec![0x66; length];
            let prior = operand.clone();
            assert_eq!(
                vector.execute_f16c(operation, &mut operand),
                Err(F16cError::OperandLength)
            );
            assert_eq!(vector, before);
            assert_eq!(operand, prior);
        }
        vector = before;
        vector.simd.mxcsr = 0x1_1f80;
        let prior = vector;
        let mut operand = vec![0x55; width.bytes() / 2];
        let bytes = operand.clone();
        assert_eq!(
            vector.execute_f16c(operation, &mut operand),
            Err(F16cError::InvalidState)
        );
        assert_eq!(vector, prior);
        assert_eq!(operand, bytes);
    }
}

#[test]
fn directed_finite_overflow_with_om_unmasked_traps_before_register_or_memory_publication() {
    for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
        for destination in [F16cTarget::Register(register(0)), F16cTarget::Memory] {
            for immediate in [1, 3] {
                let mut vector = state();
                source(&mut vector, register(1), &[0x4780_0000; 8]);
                vector.simd.mxcsr = 0x1b80;
                let mut expected = vector;
                expected.simd.mxcsr = 0x1b88;
                let mut operand = vec![
                    0x5a;
                    if destination == F16cTarget::Memory {
                        width.bytes() / 2
                    } else {
                        0
                    }
                ];
                let before_operand = operand.clone();
                assert_eq!(
                    vector.execute_f16c(narrow(width, 1, destination, immediate), &mut operand),
                    Err(F16cError::FloatingPoint(0x1b88))
                );
                assert_eq!(vector, expected);
                assert_eq!(operand, before_operand);
            }
        }
    }
}
