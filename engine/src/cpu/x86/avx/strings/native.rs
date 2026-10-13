use super::*;
use crate::cpu::x86::{ir::EffectiveAddress, packed_strings::PackedStringLength};

fn operation(
    left: u8,
    right: PackedStringSource,
    length: PackedStringLength,
    output: PackedStringOutput,
    control: u8,
) -> StringInstruction {
    StringInstruction::new(PackedStringCompare {
        left,
        right,
        length,
        output,
        control,
    })
    .unwrap()
}

fn seeded() -> (VectorState, State32) {
    let mut vector = VectorState::default();
    for register in 0..8 {
        vector.simd.xmm[register] = std::array::from_fn(|byte| (register * 17 + byte + 1) as u8);
        vector.upper[register] = [0xa0 + register as u8; 16];
    }
    vector.simd.mxcsr = 0xe5af;
    let state = State32 {
        registers: std::array::from_fn(|index| 0x1234_0000 + index as u32),
        eip: 0x7654_3210,
        eflags: 0x244fd7,
    };
    (vector, state)
}

fn words(values: [u16; 8]) -> [u8; 16] {
    let mut bytes = [0; 16];
    for (index, value) in values.into_iter().enumerate() {
        bytes[index * 2..index * 2 + 2].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn check(
    instruction: StringInstruction,
    mut vector: VectorState,
    mut state: State32,
    operand: &[u8],
    bits: u16,
    left_length: usize,
    right_length: usize,
) {
    let control = instruction.0.control;
    let count = if control & 1 == 0 { 16 } else { 8 };
    let mut expected_vector = vector;
    let mut expected_state = state;
    expected_state.eflags = (state.eflags & !0x8d5)
        | u32::from(bits != 0)
        | (u32::from(right_length < count) << 6)
        | (u32::from(left_length < count) << 7)
        | (u32::from(bits & 1 != 0) << 11);
    match instruction.0.output {
        PackedStringOutput::Index => {
            expected_state.registers[Register32::Ecx.index()] = if bits == 0 {
                count as u32
            } else if control & 0x40 == 0 {
                bits.trailing_zeros()
            } else {
                15 - bits.leading_zeros()
            };
        }
        PackedStringOutput::Mask => {
            let mut mask = [0; 16];
            if control & 0x40 == 0 {
                mask[..2].copy_from_slice(&bits.to_le_bytes());
            } else {
                let width = 16 / count;
                for index in 0..count {
                    if bits & (1 << index) != 0 {
                        mask[index * width..(index + 1) * width].fill(0xff);
                    }
                }
            }
            expected_vector.simd.xmm[0] = mask;
            expected_vector.upper[0] = [0; 16];
        }
    }
    vector
        .execute_string(instruction, &mut state, operand)
        .unwrap();
    assert_eq!(vector, expected_vector, "{instruction:?}");
    assert_eq!(state, expected_state, "{instruction:?}");
}

#[test]
fn closed_register_shapes_keep_exact_unaligned_read_sixteen_contract() {
    for left in 0..8 {
        for right in 0..8 {
            let instruction = operation(
                left,
                PackedStringSource::Xmm(right),
                PackedStringLength::Explicit,
                PackedStringOutput::Index,
                0xff,
            );
            assert_eq!(instruction.memory(), None);
        }
        let address = EffectiveAddress {
            displacement: 0xffff_ffff,
            ..EffectiveAddress::default()
        };
        let instruction = operation(
            left,
            PackedStringSource::Memory(address),
            PackedStringLength::NullTerminated,
            PackedStringOutput::Mask,
            0xff,
        );
        assert_eq!(instruction.0.right, PackedStringSource::Memory(address));
        assert_eq!(
            instruction.memory(),
            Some(MemoryRequirement {
                access: Access::Read,
                bytes: 16,
                alignment: 1,
            })
        );
    }
    for (left, right) in [
        (8, PackedStringSource::Xmm(0)),
        (0, PackedStringSource::Xmm(8)),
    ] {
        assert_eq!(
            StringInstruction::new(PackedStringCompare {
                left,
                right,
                length: PackedStringLength::Explicit,
                output: PackedStringOutput::Index,
                control: 0,
            }),
            Err(ArithmeticError::InvalidRegister)
        );
    }
}

#[test]
fn every_control_has_literal_aggregation_polarity_and_result_format() {
    for control in 0..=u8::MAX {
        let format = control & 3;
        let (left, right, full) = if format & 1 == 0 {
            (
                [0x80, 0x7f, 0x20, 0x30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                [
                    0x7f, 0x80, 0x20, 0x30, 0x25, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                ],
                0xffff,
            )
        } else {
            (
                words([0x8000, 0x7fff, 0x20, 0x30, 0, 0, 0, 0]),
                words([0x7fff, 0x8000, 0x20, 0x30, 0x25, 0xffff, 0, 0]),
                0xff,
            )
        };
        let positive = match control >> 2 & 3 {
            0 => 0xf,
            1 if format & 2 == 0 => 0x1c,
            1 => 0x3f,
            2 => (full & !0x3f) | 0xc,
            _ => 0,
        };
        let bits = match control >> 4 & 3 {
            1 => positive ^ full,
            3 => positive ^ 0x3f,
            _ => positive,
        };
        for length in [
            PackedStringLength::Explicit,
            PackedStringLength::NullTerminated,
        ] {
            for output in [PackedStringOutput::Index, PackedStringOutput::Mask] {
                for memory in [false, true] {
                    let (mut vector, mut state) = seeded();
                    vector.simd.xmm[0] = left;
                    vector.simd.xmm[1] = right;
                    state.registers[Register32::Eax.index()] = 4;
                    state.registers[Register32::Edx.index()] = 6;
                    let instruction = operation(
                        0,
                        if memory {
                            PackedStringSource::Memory(EffectiveAddress::default())
                        } else {
                            PackedStringSource::Xmm(1)
                        },
                        length,
                        output,
                        control,
                    );
                    check(
                        instruction,
                        vector,
                        state,
                        if memory { &right } else { &[] },
                        bits,
                        4,
                        6,
                    );
                }
            }
        }
    }
}

#[test]
fn explicit_signed_absolute_lengths_saturate_and_implicit_lengths_ignore_gprs() {
    for format in 0..4 {
        let count = if format & 1 == 0 { 16 } else { 8 };
        let data = if format & 1 == 0 {
            [7; 16]
        } else {
            words([7; 8])
        };
        for raw in [
            0,
            1,
            u32::MAX,
            8,
            16,
            17,
            0x7fff_ffff,
            0x8000_0000,
            0xffff_fff0,
        ] {
            let length = (raw as i32).unsigned_abs().min(count) as usize;
            for output in [PackedStringOutput::Index, PackedStringOutput::Mask] {
                let (mut vector, mut state) = seeded();
                vector.simd.xmm[1] = data;
                vector.simd.xmm[2] = data;
                state.registers[Register32::Eax.index()] = raw;
                state.registers[Register32::Edx.index()] = raw;
                check(
                    operation(
                        1,
                        PackedStringSource::Xmm(2),
                        PackedStringLength::Explicit,
                        output,
                        format,
                    ),
                    vector,
                    state,
                    &[],
                    ((1u32 << length) - 1) as u16,
                    length,
                    length,
                );
                check(
                    operation(
                        1,
                        PackedStringSource::Xmm(2),
                        PackedStringLength::NullTerminated,
                        output,
                        format,
                    ),
                    vector,
                    state,
                    &[],
                    ((1u32 << count) - 1) as u16,
                    count as usize,
                    count as usize,
                );
            }
        }
    }
}

#[test]
fn ordered_physical_end_empty_patterns_and_odd_ranges_have_literal_bits() {
    let mut left = [9; 16];
    left[..4].copy_from_slice(&[1, 2, 3, 4]);
    let mut right = [9; 16];
    right[14..].copy_from_slice(&[1, 2]);
    for output in [PackedStringOutput::Index, PackedStringOutput::Mask] {
        let (mut vector, mut state) = seeded();
        vector.simd.xmm[0] = left;
        vector.simd.xmm[1] = right;
        state.registers[Register32::Eax.index()] = 4;
        state.registers[Register32::Edx.index()] = 16;
        check(
            operation(
                0,
                PackedStringSource::Xmm(1),
                PackedStringLength::Explicit,
                output,
                0xc,
            ),
            vector,
            state,
            &[],
            0x4000,
            4,
            16,
        );
        state.registers[Register32::Eax.index()] = 0;
        state.registers[Register32::Edx.index()] = 1;
        for (control, bits) in [(0xc, 0xffff), (0x1c, 0), (0x2c, 0xffff), (0x3c, 0xfffe)] {
            check(
                operation(
                    0,
                    PackedStringSource::Xmm(1),
                    PackedStringLength::Explicit,
                    output,
                    control,
                ),
                vector,
                state,
                &[],
                bits,
                0,
                1,
            );
        }
        vector.simd.xmm[0] = [10, 20, 30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        vector.simd.xmm[1] = [10, 20, 30, 40, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        state.registers[Register32::Eax.index()] = 3;
        state.registers[Register32::Edx.index()] = 4;
        check(
            operation(
                0,
                PackedStringSource::Xmm(1),
                PackedStringLength::Explicit,
                output,
                4,
            ),
            vector,
            state,
            &[],
            3,
            3,
            4,
        );
    }
}

#[test]
fn all_input_aliases_are_captured_before_mask_or_index_publication() {
    for left in 0..8 {
        for right in 0..8 {
            for length in [
                PackedStringLength::Explicit,
                PackedStringLength::NullTerminated,
            ] {
                for output in [PackedStringOutput::Index, PackedStringOutput::Mask] {
                    let (mut vector, mut state) = seeded();
                    vector.simd.xmm[usize::from(left)] = [7; 16];
                    vector.simd.xmm[usize::from(right)] = [7; 16];
                    state.registers[Register32::Eax.index()] = 16;
                    state.registers[Register32::Edx.index()] = 16;
                    check(
                        operation(left, PackedStringSource::Xmm(right), length, output, 0),
                        vector,
                        state,
                        &[],
                        0xffff,
                        16,
                        16,
                    );
                }
            }
        }
    }
}

#[test]
fn zero_lengths_still_require_sixteen_bytes_and_failures_are_state_neutral() {
    for length in [
        PackedStringLength::Explicit,
        PackedStringLength::NullTerminated,
    ] {
        for output in [PackedStringOutput::Index, PackedStringOutput::Mask] {
            let memory = operation(
                0,
                PackedStringSource::Memory(EffectiveAddress::default()),
                length,
                output,
                0,
            );
            let register = operation(0, PackedStringSource::Xmm(1), length, output, 0);
            let (mut vector, mut state) = seeded();
            vector.simd.xmm[0] = [0; 16];
            state.registers[Register32::Eax.index()] = 0;
            state.registers[Register32::Edx.index()] = 0;
            let saved_vector = vector;
            let saved_state = state;
            for size in [0, 1, 15, 17, 32] {
                assert_eq!(
                    vector.execute_string(memory, &mut state, &vec![0; size]),
                    Err(ArithmeticError::OperandLength)
                );
                assert_eq!(vector, saved_vector);
                assert_eq!(state, saved_state);
            }
            assert_eq!(
                vector.execute_string(register, &mut state, &[0; 16]),
                Err(ArithmeticError::OperandLength)
            );
            assert_eq!(vector, saved_vector);
            assert_eq!(state, saved_state);
            check(memory, vector, state, &[0; 16], 0, 0, 0);
            vector.simd.mxcsr |= 1 << 16;
            let invalid = vector;
            assert_eq!(
                vector.execute_string(memory, &mut state, &[0; 16]),
                Err(ArithmeticError::InvalidState)
            );
            assert_eq!(vector, invalid);
            assert_eq!(state, saved_state);
        }
    }
}
