use super::*;
use crate::cpu::x86::Register32;
fn register(index: u8) -> VectorRegister {
    VectorRegister::new(index).unwrap()
}
fn state() -> (VectorState, State32) {
    let mut vector = VectorState {
        simd: crate::cpu::x86::simd::SimdState {
            mxcsr: 0xe5af,
            ..Default::default()
        },
        ..Default::default()
    };
    for r in 0..8 {
        for lane in 0..8 {
            let value = (r as u32 * 100 + lane as u32 + 1).to_le_bytes();
            if lane < 4 {
                vector.simd.xmm[r][lane * 4..lane * 4 + 4].copy_from_slice(&value);
            } else {
                vector.upper[r][(lane - 4) * 4..(lane - 4) * 4 + 4].copy_from_slice(&value);
            }
        }
    }
    (
        vector,
        State32 {
            registers: [0xfeed_face; 8],
            eip: 0x1234_5678,
            eflags: 0x244cd7,
        },
    )
}
fn expected(mut before: VectorState, destination: VectorRegister, value: &[u8]) -> VectorState {
    let mut all = [0; 32];
    all[..value.len()].copy_from_slice(value);
    before.simd.xmm[destination.index()].copy_from_slice(&all[..16]);
    before.upper[destination.index()].copy_from_slice(&all[16..]);
    before
}
fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

#[test]
fn literal_permutations_keep_128_bit_boundaries_and_distinct_upper_controls() {
    let d = register(0);
    let first = register(1);
    let second = MoveSource::Register(register(2));
    let cases = [
        (
            PermuteInstruction::Interleave {
                high: true,
                element: Element::Single,
                width: VectorWidth::Ymm,
                destination: d,
                first,
                second,
            },
            vec![103, 203, 104, 204, 107, 207, 108, 208],
        ),
        (
            PermuteInstruction::Interleave {
                high: false,
                element: Element::Double,
                width: VectorWidth::Ymm,
                destination: d,
                first,
                second,
            },
            vec![101, 102, 201, 202, 105, 106, 205, 206],
        ),
        (
            PermuteInstruction::Shuffle {
                element: Element::Single,
                width: VectorWidth::Ymm,
                destination: d,
                first,
                second,
                immediate: 0x1b,
            },
            vec![104, 103, 202, 201, 108, 107, 206, 205],
        ),
        (
            PermuteInstruction::Shuffle {
                element: Element::Double,
                width: VectorWidth::Ymm,
                destination: d,
                first,
                second,
                immediate: 0x0c,
            },
            vec![101, 102, 201, 202, 107, 108, 207, 208],
        ),
        (
            PermuteInstruction::Immediate {
                element: Element::Double,
                width: VectorWidth::Ymm,
                destination: d,
                source: MoveSource::Register(first),
                immediate: 0x0c,
            },
            vec![101, 102, 101, 102, 107, 108, 107, 108],
        ),
        (
            PermuteInstruction::Blend {
                element: Element::Single,
                width: VectorWidth::Ymm,
                destination: d,
                first,
                second,
                immediate: 0x96,
            },
            vec![101, 202, 203, 104, 205, 106, 107, 208],
        ),
    ];
    for (operation, literal) in cases {
        let (mut vector, mut general) = state();
        let before = (vector, general);
        vector
            .execute_permute(operation, &mut general, &mut [])
            .unwrap();
        assert_eq!(vector, expected(before.0, d, &words(&literal)));
        assert_eq!(general, before.1);
    }
}

#[test]
fn all_immediates_and_alias_partitions_select_original_lanes() {
    for immediate in 0..=255_u8 {
        for (d, a, b) in [(0, 1, 2), (0, 0, 2), (0, 1, 0), (0, 1, 1), (0, 0, 0)] {
            let (destination, first, second) =
                (register(d), register(a), MoveSource::Register(register(b)));
            for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
                for element in [Element::Single, Element::Double] {
                    for kind in 0..3 {
                        let (mut vector, mut general) = state();
                        let before = (vector, general);
                        let av = vector.read(first);
                        let bv = vector.read(register(b));
                        let mut value = vec![0; width.bytes()];
                        let bytes = element.bytes();
                        for lane in 0..width.bytes() / bytes {
                            let local = lane % (16 / bytes);
                            let half = lane / (16 / bytes);
                            let selection = if element == Element::Single {
                                usize::from((immediate >> (local * 2)) & 3)
                            } else {
                                usize::from((immediate >> lane) & 1)
                            };
                            let (source, index) = match kind {
                                0 => (
                                    if local < 8 / bytes { &av } else { &bv },
                                    half * 16 + selection * bytes,
                                ),
                                1 => (&bv, half * 16 + selection * bytes),
                                _ => (
                                    if immediate & (1 << lane) == 0 {
                                        &av
                                    } else {
                                        &bv
                                    },
                                    lane * bytes,
                                ),
                            };
                            value[lane * bytes..lane * bytes + bytes]
                                .copy_from_slice(&source[index..index + bytes]);
                        }
                        let operation = match kind {
                            0 => PermuteInstruction::Shuffle {
                                element,
                                width,
                                destination,
                                first,
                                second,
                                immediate,
                            },
                            1 => PermuteInstruction::Immediate {
                                element,
                                width,
                                destination,
                                source: second,
                                immediate,
                            },
                            _ => PermuteInstruction::Blend {
                                element,
                                width,
                                destination,
                                first,
                                second,
                                immediate,
                            },
                        };
                        vector
                            .execute_permute(operation, &mut general, &mut [])
                            .unwrap();
                        assert_eq!(
                            vector,
                            expected(before.0, destination, &value),
                            "imm{immediate:02x} kind{kind}"
                        );
                        assert_eq!(general, before.1);
                    }
                }
            }
            let (mut vector, mut general) = state();
            let before = (vector, general);
            let mut value = before.0.read(first)[..16].to_vec();
            let source = before.0.read(register(b));
            let target = usize::from(immediate >> 4 & 3) * 4;
            let at = usize::from(immediate >> 6) * 4;
            value[target..target + 4].copy_from_slice(&source[at..at + 4]);
            for lane in 0..4 {
                if immediate & (1 << lane) != 0 {
                    value[lane * 4..lane * 4 + 4].fill(0);
                }
            }
            vector
                .execute_permute(
                    PermuteInstruction::InsertSingle {
                        destination,
                        first,
                        second,
                        immediate,
                    },
                    &mut general,
                    &mut [],
                )
                .unwrap();
            assert_eq!(vector, expected(before.0, destination, &value));
            assert_eq!(general, before.1);
        }
    }
}

#[test]
fn variable_controls_use_bit1_for_double_and_only_sign_for_blends() {
    for element in [Element::Single, Element::Double] {
        for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
            for (d, a, c) in [
                (0_u8, 1_u8, 2_u8),
                (0, 0, 2),
                (0, 1, 0),
                (0, 1, 1),
                (0, 0, 0),
            ] {
                let (mut vector, mut general) = state();
                let bytes = element.bytes();
                let mut control = [0xff; 32];
                for lane in 0..width.bytes() / bytes {
                    control[lane * bytes] = [1, 2, 3, 0][lane % 4];
                }
                vector.simd.xmm[usize::from(c)][..].copy_from_slice(&control[..16]);
                vector.upper[usize::from(c)][..].copy_from_slice(&control[16..]);
                let before = (vector, general);
                let source = before.0.read(register(a));
                let control = before.0.read(register(c));
                let mut value = vec![0; width.bytes()];
                for lane in 0..width.bytes() / bytes {
                    let selected = if element == Element::Single {
                        usize::from(control[lane * bytes] & 3)
                    } else {
                        usize::from(control[lane * bytes] & 2) / 2
                    };
                    let at = lane * bytes / 16 * 16 + selected * bytes;
                    value[lane * bytes..lane * bytes + bytes]
                        .copy_from_slice(&source[at..at + bytes]);
                }
                vector
                    .execute_permute(
                        PermuteInstruction::Variable {
                            element,
                            width,
                            destination: register(d),
                            source: register(a),
                            control: MoveSource::Register(register(c)),
                        },
                        &mut general,
                        &mut [],
                    )
                    .unwrap();
                assert_eq!(vector, expected(before.0, register(d), &value));
                assert_eq!(general, before.1);
            }
        }
    }
    for element in [Element::Single, Element::Double] {
        for mask_register in [0, 1, 2, 3] {
            let (mut vector, mut general) = state();
            let mut mask = [0xff; 32];
            for lane in 0..32 / element.bytes() {
                mask[(lane + 1) * element.bytes() - 1] = if lane % 2 == 0 { 0x7f } else { 0x80 };
            }
            vector.simd.xmm[mask_register].copy_from_slice(&mask[..16]);
            vector.upper[mask_register].copy_from_slice(&mask[16..]);
            let before = (vector, general);
            let a = before.0.read(register(1));
            let b = before.0.read(register(2));
            let mask = before.0.read(register(mask_register as u8));
            let value: Vec<u8> = (0..32)
                .map(|i| {
                    if mask[(i / element.bytes() + 1) * element.bytes() - 1] >= 128 {
                        b[i]
                    } else {
                        a[i]
                    }
                })
                .collect();
            vector
                .execute_permute(
                    PermuteInstruction::BlendVariable {
                        element,
                        width: VectorWidth::Ymm,
                        destination: register(0),
                        first: register(1),
                        second: MoveSource::Register(register(2)),
                        mask: register(mask_register as u8),
                    },
                    &mut general,
                    &mut [],
                )
                .unwrap();
            assert_eq!(vector, expected(before.0, register(0), &value));
            assert_eq!(general, before.1);
        }
    }
}

#[test]
fn memory_width_and_host_validation_precede_any_publication() {
    let operations = [
        PermuteInstruction::Interleave {
            high: false,
            element: Element::Single,
            width: VectorWidth::Xmm,
            destination: register(0),
            first: register(0),
            second: MoveSource::Memory,
        },
        PermuteInstruction::Variable {
            element: Element::Double,
            width: VectorWidth::Ymm,
            destination: register(0),
            source: register(1),
            control: MoveSource::Memory,
        },
        PermuteInstruction::InsertSingle {
            destination: register(0),
            first: register(0),
            second: MoveSource::Memory,
            immediate: 0xc0,
        },
        PermuteInstruction::ExtractSingle {
            source: register(1),
            destination: WordTarget::Memory,
            immediate: 0xff,
        },
    ];
    for operation in operations {
        let requirement = operation.memory().unwrap();
        assert_eq!(requirement.alignment, 1);
        assert_eq!(
            requirement.access,
            if matches!(operation, PermuteInstruction::ExtractSingle { .. }) {
                Access::Write
            } else {
                Access::Read
            }
        );
        for length in [
            usize::from(requirement.bytes) - 1,
            usize::from(requirement.bytes) + 1,
        ] {
            let (mut vector, mut general) = state();
            let before = (vector, general);
            let mut operand = vec![0xa5; length];
            let original = operand.clone();
            assert_eq!(
                vector.execute_permute(operation, &mut general, &mut operand),
                Err(PermuteError::OperandLength)
            );
            assert_eq!((vector, general), before);
            assert_eq!(operand, original);
        }
        let (mut vector, mut general) = state();
        vector.simd.mxcsr = 0x10000;
        let before = (vector, general);
        let mut operand = vec![0xa5; usize::from(requirement.bytes)];
        let original = operand.clone();
        assert_eq!(
            vector.execute_permute(operation, &mut general, &mut operand),
            Err(PermuteError::InvalidState)
        );
        assert_eq!((vector, general), before);
        assert_eq!(operand, original);
    }
    for immediate in 0..=255_u8 {
        let (mut vector, mut general) = state();
        let before = (vector, general);
        let mut operand = 0x7f80_0001_u32.to_le_bytes();
        let original = operand;
        let mut value = before.0.read(register(0))[..16].to_vec();
        let at = usize::from(immediate >> 4 & 3) * 4;
        value[at..at + 4].copy_from_slice(&operand);
        for lane in 0..4 {
            if immediate & (1 << lane) != 0 {
                value[lane * 4..lane * 4 + 4].fill(0);
            }
        }
        vector
            .execute_permute(
                PermuteInstruction::InsertSingle {
                    destination: register(0),
                    first: register(0),
                    second: MoveSource::Memory,
                    immediate,
                },
                &mut general,
                &mut operand,
            )
            .unwrap();
        assert_eq!(vector, expected(before.0, register(0), &value));
        assert_eq!(general, before.1);
        assert_eq!(operand, original);
    }
}

#[test]
fn extraction_masks_immediate_and_preserves_complete_vector_state() {
    for immediate in 0..=255_u8 {
        for destination in [
            Register32::Eax,
            Register32::Ecx,
            Register32::Edx,
            Register32::Ebx,
            Register32::Esp,
            Register32::Ebp,
            Register32::Esi,
            Register32::Edi,
        ] {
            let (mut vector, mut general) = state();
            let before = (vector, general);
            let operation = PermuteInstruction::ExtractSingle {
                source: register(1),
                destination: WordTarget::Register(destination),
                immediate,
            };
            vector
                .execute_permute(operation, &mut general, &mut [])
                .unwrap();
            let mut expected = before.1;
            expected.registers[destination.index()] = 101 + u32::from(immediate & 3);
            assert_eq!(general, expected);
            assert_eq!(vector, before.0);
        }
    }
    let (mut vector, mut general) = state();
    let before = (vector, general);
    let mut guarded = [0xa5; 12];
    vector
        .execute_permute(
            PermuteInstruction::ExtractSingle {
                source: register(1),
                destination: WordTarget::Memory,
                immediate: 0xfe,
            },
            &mut general,
            &mut guarded[4..8],
        )
        .unwrap();
    assert_eq!(&guarded[4..8], &103_u32.to_le_bytes());
    assert_eq!(&guarded[..4], &[0xa5; 4]);
    assert_eq!(&guarded[8..], &[0xa5; 4]);
    assert_eq!((vector, general), before);
}
