use super::*;

fn register(index: u8) -> VectorRegister {
    VectorRegister::new(index).unwrap()
}

fn state() -> (VectorState, State32) {
    let mut vector = VectorState::default();
    for r in 0..8 {
        vector.simd.xmm[r] = std::array::from_fn(|i| (r * 23 + i * 11 + 1) as u8);
        vector.upper[r] = std::array::from_fn(|i| (r * 17 + i * 13 + 7) as u8);
    }
    (
        vector,
        State32 {
            registers: std::array::from_fn(|r| 0x7654_0000 + r as u32),
            eip: 0x1234_5678,
            eflags: 0xa5f7_ffff,
        },
    )
}

fn lanes(values: &[u64], bytes: usize) -> [u8; 32] {
    std::array::from_fn(|i| values[(i / bytes) % values.len()].to_le_bytes()[i % bytes])
}

#[test]
fn all_packed_forms_use_literal_results_exact_memory_width_and_zero_destination_tail() {
    let cases = [
        (
            PackedConversion::SignedToSingle,
            4,
            4,
            0xffff_fffe,
            0xc000_0000,
        ),
        (
            PackedConversion::SignedToDouble,
            4,
            8,
            0xffff_fffe,
            0xc000_0000_0000_0000,
        ),
        (
            PackedConversion::SingleToSigned { truncate: false },
            4,
            4,
            0xbfc0_0000,
            0xffff_fffe,
        ),
        (
            PackedConversion::SingleToSigned { truncate: true },
            4,
            4,
            0xbfc0_0000,
            0xffff_ffff,
        ),
        (
            PackedConversion::DoubleToSigned { truncate: false },
            8,
            4,
            0xbff8_0000_0000_0000,
            0xffff_fffe,
        ),
        (
            PackedConversion::DoubleToSigned { truncate: true },
            8,
            4,
            0xbff8_0000_0000_0000,
            0xffff_ffff,
        ),
        (
            PackedConversion::SingleToDouble,
            4,
            8,
            0xbfc0_0000,
            0xbff8_0000_0000_0000,
        ),
        (
            PackedConversion::DoubleToSingle,
            8,
            4,
            0xbff8_0000_0000_0000,
            0xbfc0_0000,
        ),
    ];
    for (kind, source_bytes, destination_bytes, raw, literal) in cases {
        for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
            let count = width.bytes() / source_bytes.max(destination_bytes);
            let memory_bytes = count * source_bytes;
            for memory in [false, true] {
                for destination in [0, 2] {
                    let (mut vector, mut general) = state();
                    let input = lanes(&[raw], source_bytes);
                    vector.publish(register(2), input);
                    let operation = ConvertInstruction::Packed {
                        kind,
                        width,
                        destination: register(destination),
                        source: if memory {
                            MoveSource::Memory
                        } else {
                            MoveSource::Register(register(2))
                        },
                    };
                    assert_eq!(
                        operation.memory(),
                        memory.then_some(MemoryRequirement {
                            access: Access::Read,
                            bytes: memory_bytes as u8,
                            alignment: 1
                        })
                    );
                    let mut expected = vector;
                    let mut result = [0; 32];
                    result[..count * destination_bytes].copy_from_slice(
                        &lanes(&[literal], destination_bytes)[..count * destination_bytes],
                    );
                    expected.publish(register(destination), result);
                    if matches!(
                        kind,
                        PackedConversion::SingleToSigned { .. }
                            | PackedConversion::DoubleToSigned { .. }
                    ) {
                        expected.simd.mxcsr |= 0x20;
                    }
                    let saved = general;
                    vector
                        .execute_convert(
                            operation,
                            &mut general,
                            if memory { &input[..memory_bytes] } else { &[] },
                        )
                        .unwrap();
                    assert_eq!(
                        vector, expected,
                        "{kind:?}/{width:?}/memory{memory}/destination{destination}"
                    );
                    assert_eq!(general, saved);
                }
            }
        }
    }
}

#[test]
fn signed_conversion_rounding_truncation_and_indefinite_are_literal() {
    for (raw, expected) in [
        (0x3fc0_0000, [2, 1, 2, 1]),
        (
            0xbfc0_0000,
            [0xffff_fffe, 0xffff_fffe, 0xffff_ffff, 0xffff_ffff],
        ),
    ] {
        for rounding in 0..4 {
            for truncate in [false, true] {
                let (mut vector, mut general) = state();
                vector.simd.mxcsr = 0x1f80 | (rounding << 13);
                vector.publish(register(2), lanes(&[raw], 4));
                let before = vector;
                vector
                    .execute_convert(
                        ConvertInstruction::ToInteger {
                            element: Element::Single,
                            truncate,
                            destination: Register32::Eax,
                            source: MoveSource::Register(register(2)),
                        },
                        &mut general,
                        &[],
                    )
                    .unwrap();
                assert_eq!(
                    general.registers[0],
                    expected[if truncate { 3 } else { rounding as usize }]
                );
                let mut expected_vector = before;
                expected_vector.simd.mxcsr |= 0x20;
                assert_eq!(vector, expected_vector);
            }
        }
    }
    for (raw, literal, exception) in [
        (0x4f00_0000, 0x8000_0000, 1),
        (0xcf00_0000, 0x8000_0000, 0),
        (0x7fc0_1234, 0x8000_0000, 1),
        (0x7f80_0000, 0x8000_0000, 1),
        (1, 0, 0x20),
    ] {
        let (mut vector, mut general) = state();
        vector.publish(register(2), lanes(&[raw], 4));
        vector
            .execute_convert(
                ConvertInstruction::ToInteger {
                    element: Element::Single,
                    truncate: true,
                    destination: Register32::Eax,
                    source: MoveSource::Register(register(2)),
                },
                &mut general,
                &[],
            )
            .unwrap();
        assert_eq!(general.registers[0], literal);
        assert_eq!(vector.simd.mxcsr, 0x1f80 | exception);
    }
    for (integer, nearest, up_or_down) in [
        (0x0100_0001, 0x4b80_0000, 0x4b80_0001),
        (0xfeff_ffff, 0xcb80_0000, 0xcb80_0001),
    ] {
        for rounding in 0..4 {
            let (mut vector, mut general) = state();
            vector.simd.mxcsr = 0x1f80 | (rounding << 13);
            general.registers[0] = integer;
            vector
                .execute_convert(
                    ConvertInstruction::FromInteger {
                        element: Element::Single,
                        destination: register(0),
                        merge: register(1),
                        source: WordSource::Register(Register32::Eax),
                    },
                    &mut general,
                    &[],
                )
                .unwrap();
            let directed = if integer == 0x0100_0001 { 2 } else { 1 };
            assert_eq!(
                u32::from_le_bytes(vector.simd.xmm[0][..4].try_into().unwrap()),
                if rounding == directed {
                    up_or_down
                } else {
                    nearest
                }
            );
            assert_eq!(vector.simd.mxcsr, (0x1f80 | (rounding << 13)) | 0x20);
        }
    }
}

#[test]
fn scalar_forms_capture_merge_sources_under_aliases_and_preserve_other_cpu_state() {
    for (source_element, raw, literal, destination_bytes) in [
        (Element::Single, 0xbfc0_0000, 0xbff8_0000_0000_0000_u64, 8),
        (Element::Double, 0xbff8_0000_0000_0000, 0xbfc0_0000, 4),
    ] {
        for (d, a, b) in [(0, 1, 2), (1, 1, 2), (2, 1, 2), (1, 1, 1)] {
            let (mut vector, mut general) = state();
            vector.publish(register(b), lanes(&[raw], source_element.bytes()));
            let mut expected = vector;
            expected.simd.xmm[usize::from(d)] = vector.simd.xmm[usize::from(a)];
            expected.simd.xmm[usize::from(d)][..destination_bytes]
                .copy_from_slice(&literal.to_le_bytes()[..destination_bytes]);
            expected.upper[usize::from(d)] = [0; 16];
            let saved = general;
            vector
                .execute_convert(
                    ConvertInstruction::Scalar {
                        source_element,
                        destination: register(d),
                        merge: register(a),
                        source: MoveSource::Register(register(b)),
                    },
                    &mut general,
                    &[],
                )
                .unwrap();
            assert_eq!(vector, expected);
            assert_eq!(general, saved);
        }
    }
    for element in [Element::Single, Element::Double] {
        let (mut vector, mut general) = state();
        general.registers[1] = 0xffff_fffe;
        let mut expected = vector;
        expected.simd.xmm[0] = vector.simd.xmm[1];
        let literal: u64 = if element == Element::Single {
            0xc000_0000
        } else {
            0xc000_0000_0000_0000
        };
        expected.simd.xmm[0][..element.bytes()]
            .copy_from_slice(&literal.to_le_bytes()[..element.bytes()]);
        expected.upper[0] = [0; 16];
        let saved = general;
        vector
            .execute_convert(
                ConvertInstruction::FromInteger {
                    element,
                    destination: register(0),
                    merge: register(1),
                    source: WordSource::Register(Register32::Ecx),
                },
                &mut general,
                &[],
            )
            .unwrap();
        assert_eq!(vector, expected);
        assert_eq!(general, saved);
    }
}

#[test]
fn narrowing_nan_tiny_ftz_and_unmasked_aggregated_exceptions_follow_shared_policy() {
    let cases = [
        (0x3ff0_0000_0000_0000, 0x1f80, 0x3f80_0000, 0, false),
        (0x36a0_0000_0000_0000, 0x1f80, 1, 0, false),
        (0x36a0_0000_0000_0000, 0x9f80, 0, 0x30, false),
        (0x3690_0000_0000_0000, 0x1f80, 0, 0x30, false),
        (0x3690_0000_0000_0000, 0x5f80, 1, 0x30, false),
        (0x36a0_0000_0000_0000, 0x1780, 1, 0x10, true),
        (0x47f0_0000_0000_0000, 0x1f80, 0x7f80_0000, 0x28, false),
        (0x47f0_0000_0000_0000, 0x7f80, 0x7f7f_ffff, 0x28, false),
        (0x47f0_0000_0000_0000, 0x1b80, 0x7f80_0000, 8, true),
        (0x7ff0_0000_2000_0000, 0x1f80, 0x7fc0_0001, 1, false),
    ];
    for (raw, mxcsr, literal, exceptions, fault) in cases {
        let (mut vector, mut general) = state();
        vector.simd.mxcsr = mxcsr;
        vector.publish(register(2), lanes(&[raw], 8));
        let mut expected = vector;
        expected.simd.mxcsr |= exceptions;
        if !fault {
            let mut result = [0; 32];
            result[..16].copy_from_slice(&lanes(&[literal], 4)[..16]);
            expected.publish(register(0), result);
        }
        let saved = general;
        let result = vector.execute_convert(
            ConvertInstruction::Packed {
                kind: PackedConversion::DoubleToSingle,
                width: VectorWidth::Ymm,
                destination: register(0),
                source: MoveSource::Register(register(2)),
            },
            &mut general,
            &[],
        );
        assert_eq!(
            result,
            if fault {
                Err(ArithmeticError::FloatingPoint(mxcsr | exceptions))
            } else {
                Ok(())
            },
            "raw{raw:x}/mxcsr{mxcsr:x}"
        );
        assert_eq!(vector, expected);
        assert_eq!(general, saved);
    }
    let (mut vector, mut general) = state();
    vector.simd.mxcsr = 0x1f00;
    vector.publish(
        register(2),
        lanes(&[0x3fc0_0000, 0, 0, 0, 0, 0, 0, 0x7fc0_0001], 4),
    );
    let mut expected = vector;
    expected.simd.mxcsr = 0x1f01;
    let saved = general;
    assert_eq!(
        vector.execute_convert(
            ConvertInstruction::Packed {
                kind: PackedConversion::SingleToSigned { truncate: false },
                width: VectorWidth::Ymm,
                destination: register(0),
                source: MoveSource::Register(register(2))
            },
            &mut general,
            &[]
        ),
        Err(ArithmeticError::FloatingPoint(0x1f01))
    );
    assert_eq!(vector, expected);
    assert_eq!(general, saved);
}

#[test]
fn denormal_input_daz_exact_operand_lengths_and_invalid_state_refusals_are_explicit() {
    for (mxcsr, literal, exception) in [(0x1f80, 0x36a0_0000_0000_0000_u64, 2), (0x1fc0, 0, 0)] {
        let (mut vector, mut general) = state();
        vector.simd.mxcsr = mxcsr;
        vector.publish(register(2), lanes(&[1], 4));
        vector
            .execute_convert(
                ConvertInstruction::Packed {
                    kind: PackedConversion::SingleToDouble,
                    width: VectorWidth::Ymm,
                    destination: register(0),
                    source: MoveSource::Register(register(2)),
                },
                &mut general,
                &[],
            )
            .unwrap();
        assert_eq!(vector.read(register(0)), lanes(&[literal], 8));
        assert_eq!(vector.simd.mxcsr, mxcsr | exception);
    }
    let operations = [
        ConvertInstruction::FromInteger {
            element: Element::Double,
            destination: register(0),
            merge: register(1),
            source: WordSource::Memory,
        },
        ConvertInstruction::ToInteger {
            element: Element::Double,
            truncate: false,
            destination: Register32::Eax,
            source: MoveSource::Memory,
        },
        ConvertInstruction::Scalar {
            source_element: Element::Single,
            destination: register(0),
            merge: register(1),
            source: MoveSource::Memory,
        },
        ConvertInstruction::Packed {
            kind: PackedConversion::SingleToDouble,
            width: VectorWidth::Ymm,
            destination: register(0),
            source: MoveSource::Memory,
        },
    ];
    for operation in operations {
        let bytes = usize::from(operation.memory().unwrap().bytes);
        for length in [0, bytes - 1, bytes + 1] {
            let (mut vector, mut general) = state();
            let saved = (vector, general);
            assert_eq!(
                vector.execute_convert(operation, &mut general, &vec![0; length]),
                Err(ArithmeticError::OperandLength)
            );
            assert_eq!((vector, general), saved);
        }
        let (mut vector, mut general) = state();
        vector.simd.mxcsr |= 0x10000;
        let saved = (vector, general);
        assert_eq!(
            vector.execute_convert(operation, &mut general, &vec![0; bytes]),
            Err(ArithmeticError::InvalidState)
        );
        assert_eq!((vector, general), saved);
    }
}
