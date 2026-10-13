use super::*;

fn register(index: u8) -> VectorRegister {
    VectorRegister::new(index).unwrap()
}

fn state() -> (VectorState, State32) {
    let mut vector = VectorState::default();
    for r in 0..8 {
        vector.simd.xmm[r] = std::array::from_fn(|i| (r * 19 + i * 3 + 1) as u8);
        vector.upper[r] = std::array::from_fn(|i| (r * 31 + i * 7 + 3) as u8);
    }
    (
        vector,
        State32 {
            registers: std::array::from_fn(|r| 0x1234_0000 + r as u32),
            eip: 0x1234_5678,
            eflags: 0xa5f7_ffff,
        },
    )
}

fn lanes(values: &[u64], bytes: usize) -> [u8; 32] {
    std::array::from_fn(|i| values[(i / bytes) % values.len()].to_le_bytes()[i % bytes])
}

fn operation(
    shape: CompareShape,
    d: u8,
    a: u8,
    source: MoveSource,
    immediate: u8,
) -> CompareInstruction {
    CompareInstruction::Vector {
        shape,
        destination: register(d),
        first: register(a),
        second: source,
        immediate,
    }
}

#[test]
fn all_predicates_have_literal_truth_tables_and_signaling_classes() {
    let truth = [2_u8, 1, 3, 8, 13, 14, 12, 7, 10, 9, 11, 0, 5, 6, 4, 15];
    let signaling = 0x9999_6666_u32;
    for (element, one, two, three, qnan) in [
        (
            Element::Single,
            0x3f80_0000,
            0x4000_0000,
            0x4040_0000,
            0x7fc0_1234,
        ),
        (
            Element::Double,
            0x3ff0_0000_0000_0000,
            0x4000_0000_0000_0000,
            0x4008_0000_0000_0000,
            0x7ff8_0000_0000_1234,
        ),
    ] {
        let bytes = element.bytes();
        for immediate in 0..=255_u8 {
            let (mut vector, mut general) = state();
            vector.publish(register(1), lanes(&[one, two, three, qnan], bytes));
            vector.publish(register(2), lanes(&[two], bytes));
            let mut expected = vector;
            let values: Vec<u64> = (0..32 / bytes)
                .map(|lane| {
                    if truth[usize::from(immediate & 15)] & (1 << (lane % 4)) != 0 {
                        u64::MAX
                    } else {
                        0
                    }
                })
                .collect();
            expected.publish(register(0), lanes(&values, bytes));
            expected.simd.mxcsr |= (signaling >> (immediate & 31)) & 1;
            let original_general = general;
            vector
                .execute_compare(
                    operation(
                        CompareShape::Packed {
                            width: VectorWidth::Ymm,
                            element,
                        },
                        0,
                        1,
                        MoveSource::Register(register(2)),
                        immediate,
                    ),
                    &mut general,
                    &[],
                )
                .unwrap();
            assert_eq!(vector, expected, "{element:?}/{immediate}");
            assert_eq!(general, original_general);
        }
    }
}

#[test]
fn scalar_merge_aliases_preserve_first_high_lanes_and_zero_only_destination_upper() {
    for element in [Element::Single, Element::Double] {
        let one: u64 = if element == Element::Single {
            0x3f80_0000
        } else {
            0x3ff0_0000_0000_0000
        };
        for (d, a, b) in [(0, 1, 2), (1, 1, 2), (2, 1, 2), (1, 1, 1)] {
            let (mut vector, mut general) = state();
            vector.simd.xmm[usize::from(a)][..element.bytes()]
                .copy_from_slice(&one.to_le_bytes()[..element.bytes()]);
            vector.simd.xmm[usize::from(b)][..element.bytes()]
                .copy_from_slice(&one.to_le_bytes()[..element.bytes()]);
            let mut expected = vector;
            expected.simd.xmm[usize::from(d)] = vector.simd.xmm[usize::from(a)];
            expected.simd.xmm[usize::from(d)][..element.bytes()].fill(0xff);
            expected.upper[usize::from(d)] = [0; 16];
            let saved = general;
            vector
                .execute_compare(
                    operation(
                        CompareShape::Scalar(element),
                        d,
                        a,
                        MoveSource::Register(register(b)),
                        0,
                    ),
                    &mut general,
                    &[],
                )
                .unwrap();
            assert_eq!(vector, expected);
            assert_eq!(general, saved);
        }
    }
}

#[test]
fn flags_quiet_nan_unmasked_priority_and_daz_use_staged_publication() {
    for element in [Element::Single, Element::Double] {
        let bytes = element.bytes();
        let (one, two, qnan, snan) = if bytes == 4 {
            (0x3f80_0000, 0x4000_0000, 0x7fc0_0001, 0x7f80_0001)
        } else {
            (
                0x3ff0_0000_0000_0000,
                0x4000_0000_0000_0000,
                0x7ff8_0000_0000_0001,
                0x7ff0_0000_0000_0001,
            )
        };
        for (a, b, flags, unordered) in [
            (one, two, 1, false),
            (two, one, 0, false),
            (one, one, 0x40, false),
            (qnan, one, 0x45, true),
            (snan, one, 0x45, true),
        ] {
            for quiet in [false, true] {
                let (mut vector, mut general) = state();
                vector.publish(register(1), lanes(&[a], bytes));
                vector.publish(register(2), lanes(&[b], bytes));
                let mut expected = vector;
                expected.simd.mxcsr |= u32::from(a == snan || unordered && !quiet);
                let expected_general = State32 {
                    eflags: (general.eflags & !0x8d5) | flags,
                    ..general
                };
                vector
                    .execute_compare(
                        CompareInstruction::Flags {
                            element,
                            first: register(1),
                            second: MoveSource::Register(register(2)),
                            quiet,
                        },
                        &mut general,
                        &[],
                    )
                    .unwrap();
                assert_eq!(vector, expected);
                assert_eq!(general, expected_general);
            }
        }
        let (mut vector, mut general) = state();
        vector.simd.mxcsr = 0x1f00;
        vector.publish(register(1), lanes(&[snan], bytes));
        let mut expected = vector;
        expected.simd.mxcsr = 0x1f01;
        let saved = general;
        assert_eq!(
            vector.execute_compare(
                CompareInstruction::Flags {
                    element,
                    first: register(1),
                    second: MoveSource::Register(register(1)),
                    quiet: true
                },
                &mut general,
                &[]
            ),
            Err(ArithmeticError::FloatingPoint(0x1f01))
        );
        assert_eq!(vector, expected);
        assert_eq!(general, saved);
        for (mxcsr, result, status) in [(0x1f80, 0, 2), (0x1fc0, u64::MAX, 0)] {
            let (mut vector, mut general) = state();
            vector.simd.mxcsr = mxcsr;
            vector.publish(register(1), lanes(&[1], bytes));
            vector.publish(register(2), [0; 32]);
            vector
                .execute_compare(
                    operation(
                        CompareShape::Scalar(element),
                        0,
                        1,
                        MoveSource::Register(register(2)),
                        0,
                    ),
                    &mut general,
                    &[],
                )
                .unwrap();
            assert_eq!(&vector.simd.xmm[0][..bytes], &result.to_le_bytes()[..bytes]);
            assert_eq!(vector.simd.mxcsr, mxcsr | status);
        }
    }
}

#[test]
fn exact_memory_widths_and_upper_lane_exceptions_do_not_publish_partial_results() {
    for (shape, bytes) in [
        (CompareShape::Scalar(Element::Single), 4),
        (CompareShape::Scalar(Element::Double), 8),
        (
            CompareShape::Packed {
                width: VectorWidth::Xmm,
                element: Element::Single,
            },
            16,
        ),
        (
            CompareShape::Packed {
                width: VectorWidth::Ymm,
                element: Element::Double,
            },
            32,
        ),
    ] {
        let instruction = operation(shape, 0, 1, MoveSource::Memory, 0);
        assert_eq!(
            instruction.memory(),
            Some(MemoryRequirement {
                access: Access::Read,
                bytes,
                alignment: 1
            })
        );
        for length in [0, usize::from(bytes) - 1, usize::from(bytes) + 1] {
            let (mut vector, mut general) = state();
            let before = (vector, general);
            assert_eq!(
                vector.execute_compare(instruction, &mut general, &vec![0; length]),
                Err(ArithmeticError::OperandLength)
            );
            assert_eq!((vector, general), before);
        }
    }
    let (mut vector, mut general) = state();
    vector.simd.mxcsr = 0x1f00;
    vector.publish(register(1), [0; 32]);
    vector.publish(register(2), lanes(&[0, 0, 0, 0, 0, 0, 0, 0x7f80_0001], 4));
    let mut expected = vector;
    expected.simd.mxcsr = 0x1f01;
    let saved = general;
    assert_eq!(
        vector.execute_compare(
            operation(
                CompareShape::Packed {
                    width: VectorWidth::Ymm,
                    element: Element::Single
                },
                0,
                1,
                MoveSource::Register(register(2)),
                0
            ),
            &mut general,
            &[]
        ),
        Err(ArithmeticError::FloatingPoint(0x1f01))
    );
    assert_eq!(vector, expected);
    assert_eq!(general, saved);
}
