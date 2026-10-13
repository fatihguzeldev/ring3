use super::*;

fn register(index: u8) -> VectorRegister {
    VectorRegister::new(index).unwrap()
}
fn state() -> VectorState {
    let mut state = VectorState::default();
    for r in 0..8 {
        state.simd.xmm[r] = std::array::from_fn(|i| (r * 17 + i * 7 + 1) as u8);
        state.upper[r] = std::array::from_fn(|i| (r * 19 + i * 11 + 3) as u8);
    }
    state
}
fn lanes(values: &[u64], width: usize) -> [u8; 32] {
    std::array::from_fn(|i| values[(i / width) % values.len()].to_le_bytes()[i % width])
}

#[test]
fn pairwise_groups_have_literal_lane_order_and_alias_results() {
    let cases = [
        (
            Element::Single,
            PairwiseKind::HorizontalAdd,
            vec![
                0x3f800000, 0x40000000, 0x40800000, 0x41000000, 0x41800000, 0x42000000, 0x42800000,
                0x43000000,
            ],
            vec![
                0x41800000, 0x42000000, 0x42800000, 0x43000000, 0x3f800000, 0x40000000, 0x40800000,
                0x41000000,
            ],
            vec![
                0x40400000, 0x41400000, 0x42400000, 0x43400000, 0x42400000, 0x43400000, 0x40400000,
                0x41400000,
            ],
        ),
        (
            Element::Single,
            PairwiseKind::HorizontalSubtract,
            vec![
                0x3f800000, 0x40000000, 0x40800000, 0x41000000, 0x41800000, 0x42000000, 0x42800000,
                0x43000000,
            ],
            vec![
                0x41800000, 0x42000000, 0x42800000, 0x43000000, 0x3f800000, 0x40000000, 0x40800000,
                0x41000000,
            ],
            vec![
                0xbf800000, 0xc0800000, 0xc1800000, 0xc2800000, 0xc1800000, 0xc2800000, 0xbf800000,
                0xc0800000,
            ],
        ),
        (
            Element::Single,
            PairwiseKind::AddSubtract,
            vec![
                0x3f800000, 0x40000000, 0x40800000, 0x41000000, 0x41800000, 0x42000000, 0x42800000,
                0x43000000,
            ],
            vec![
                0x41800000, 0x42000000, 0x42800000, 0x43000000, 0x3f800000, 0x40000000, 0x40800000,
                0x41000000,
            ],
            vec![
                0xc1700000, 0x42080000, 0xc2700000, 0x43080000, 0x41700000, 0x42080000, 0x42700000,
                0x43080000,
            ],
        ),
        (
            Element::Double,
            PairwiseKind::HorizontalAdd,
            vec![
                0x3ff0000000000000,
                0x4000000000000000,
                0x4030000000000000,
                0x4040000000000000,
            ],
            vec![
                0x4010000000000000,
                0x4020000000000000,
                0x3ff0000000000000,
                0x4000000000000000,
            ],
            vec![
                0x4008000000000000,
                0x4028000000000000,
                0x4048000000000000,
                0x4008000000000000,
            ],
        ),
        (
            Element::Double,
            PairwiseKind::HorizontalSubtract,
            vec![
                0x3ff0000000000000,
                0x4000000000000000,
                0x4030000000000000,
                0x4040000000000000,
            ],
            vec![
                0x4010000000000000,
                0x4020000000000000,
                0x3ff0000000000000,
                0x4000000000000000,
            ],
            vec![
                0xbff0000000000000,
                0xc010000000000000,
                0xc030000000000000,
                0xbff0000000000000,
            ],
        ),
        (
            Element::Double,
            PairwiseKind::AddSubtract,
            vec![
                0x3ff0000000000000,
                0x4000000000000000,
                0x4030000000000000,
                0x4040000000000000,
            ],
            vec![
                0x4010000000000000,
                0x4020000000000000,
                0x3ff0000000000000,
                0x4000000000000000,
            ],
            vec![
                0xc008000000000000,
                0x4024000000000000,
                0x402e000000000000,
                0x4041000000000000,
            ],
        ),
    ];
    for (element, kind, a, b, literal) in cases {
        for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
            for (d, first, second) in [(0, 1, 2), (1, 1, 2), (2, 1, 2)] {
                let mut vector = state();
                vector.publish(register(first), lanes(&a, element.bytes()));
                vector.publish(register(second), lanes(&b, element.bytes()));
                let mut expected = vector;
                let mut result = lanes(&literal, element.bytes());
                result[width.bytes()..].fill(0);
                expected.publish(register(d), result);
                vector
                    .execute_compound(
                        CompoundInstruction::Pairwise {
                            kind,
                            width,
                            element,
                            destination: register(d),
                            first: register(first),
                            second: MoveSource::Register(register(second)),
                        },
                        &[],
                    )
                    .unwrap();
                assert_eq!(vector, expected, "{kind:?}/{element:?}/{width:?}/{d}");
            }
        }
    }
}

#[test]
fn pairwise_nan_order_and_all_active_lanes_precede_publication() {
    let mut vector = state();
    vector.publish(register(1), lanes(&[0x7fc01234, 0x7f805678, 0, 0], 4));
    vector.publish(register(2), [0; 32]);
    vector
        .execute_compound(
            CompoundInstruction::Pairwise {
                kind: PairwiseKind::HorizontalAdd,
                width: VectorWidth::Xmm,
                element: Element::Single,
                destination: register(0),
                first: register(1),
                second: MoveSource::Register(register(2)),
            },
            &[],
        )
        .unwrap();
    assert_eq!(
        u32::from_le_bytes(vector.simd.xmm[0][..4].try_into().unwrap()),
        0x7fc01234
    );
    assert_eq!(vector.simd.mxcsr, 0x1f81);
    let mut vector = state();
    vector.simd.mxcsr = 0x1f00;
    vector.publish(
        register(1),
        lanes(
            &[0x4b800000, 0x3f800000, 0, 0, 0x7f800000, 0xff800000, 1, 0],
            4,
        ),
    );
    vector.publish(register(2), [0; 32]);
    let mut expected = vector;
    expected.simd.mxcsr = 0x1f03;
    assert_eq!(
        vector.execute_compound(
            CompoundInstruction::Pairwise {
                kind: PairwiseKind::HorizontalAdd,
                width: VectorWidth::Ymm,
                element: Element::Single,
                destination: register(0),
                first: register(1),
                second: MoveSource::Register(register(2))
            },
            &[]
        ),
        Err(ArithmeticError::FloatingPoint(0x1f03))
    );
    assert_eq!(vector, expected);
    let mut vector = state();
    vector.simd.mxcsr = 0x7f80;
    vector.publish(register(1), lanes(&[0x7f7fffff, 0], 4));
    vector.publish(register(2), lanes(&[0xff7fffff, 0], 4));
    let mut expected = vector;
    expected.simd.mxcsr = 0x7fa8;
    let mut result = lanes(&[0x7f7fffff, 0], 4);
    result[16..].fill(0);
    expected.publish(register(0), result);
    vector
        .execute_compound(
            CompoundInstruction::Pairwise {
                kind: PairwiseKind::AddSubtract,
                width: VectorWidth::Xmm,
                element: Element::Single,
                destination: register(0),
                first: register(1),
                second: MoveSource::Register(register(2)),
            },
            &[],
        )
        .unwrap();
    assert_eq!(vector, expected);
}

#[test]
fn round_all_immediates_respect_mxcsr_selection_precision_suppression_and_scalar_merge() {
    let input = [0x3fc00000, 0x40200000, 0xbfc00000, 0xc0200000];
    let result = [
        [0x40000000, 0x40000000, 0xc0000000, 0xc0000000],
        [0x3f800000, 0x40000000, 0xc0000000, 0xc0400000],
        [0x40000000, 0x40400000, 0xbf800000, 0xc0000000],
        [0x3f800000, 0x40000000, 0xbf800000, 0xc0000000],
    ];
    for immediate in 0..=255_u8 {
        for mode in 0..4 {
            let mut vector = state();
            vector.simd.mxcsr = 0x1f80 | (mode << 13);
            vector.publish(register(2), lanes(&input, 4));
            let mut expected = vector;
            let selected = if immediate & 4 != 0 {
                mode as usize
            } else {
                usize::from(immediate & 3)
            };
            expected.publish(register(0), lanes(&result[selected], 4));
            if immediate & 8 == 0 {
                expected.simd.mxcsr |= 0x20;
            }
            vector
                .execute_compound(
                    CompoundInstruction::RoundPacked {
                        width: VectorWidth::Ymm,
                        element: Element::Single,
                        destination: register(0),
                        source: MoveSource::Register(register(2)),
                        immediate,
                    },
                    &[],
                )
                .unwrap();
            assert_eq!(vector, expected, "imm{immediate}/mode{mode}");
        }
    }
    for element in [Element::Single, Element::Double] {
        let (raw, literal): (u64, u64) = if element == Element::Single {
            (0x3fc00000, 0x40000000)
        } else {
            (0x3ff8000000000000, 0x4000000000000000)
        };
        for (d, a, b) in [(0, 1, 2), (1, 1, 2), (2, 1, 2), (1, 1, 1)] {
            let mut vector = state();
            vector.publish(register(b), lanes(&[raw], element.bytes()));
            let mut expected = vector;
            expected.simd.xmm[usize::from(d)] = vector.simd.xmm[usize::from(a)];
            expected.simd.xmm[usize::from(d)][..element.bytes()]
                .copy_from_slice(&literal.to_le_bytes()[..element.bytes()]);
            expected.upper[usize::from(d)] = [0; 16];
            vector
                .execute_compound(
                    CompoundInstruction::RoundScalar {
                        element,
                        destination: register(d),
                        merge: register(a),
                        source: MoveSource::Register(register(b)),
                        immediate: 8,
                    },
                    &[],
                )
                .unwrap();
            assert_eq!(vector, expected);
        }
        for (mxcsr, raw, flags, fault) in [
            (0x1f80, 1, 0x20, false),
            (0x1fc0, 1, 0, false),
            (0x0f80, 1, 0x20, true),
            (
                0x1f00,
                if element == Element::Single {
                    0x7f800001
                } else {
                    0x7ff0000000000001
                },
                1,
                true,
            ),
        ] {
            let mut vector = state();
            vector.simd.mxcsr = mxcsr;
            vector.publish(register(2), lanes(&[raw], element.bytes()));
            let mut expected = vector;
            expected.simd.mxcsr |= flags;
            if !fault {
                expected.simd.xmm[0] = vector.simd.xmm[1];
                expected.simd.xmm[0][..element.bytes()].fill(0);
                expected.upper[0] = [0; 16];
            }
            let result = vector.execute_compound(
                CompoundInstruction::RoundScalar {
                    element,
                    destination: register(0),
                    merge: register(1),
                    source: MoveSource::Register(register(2)),
                    immediate: 0,
                },
                &[],
            );
            assert_eq!(
                result,
                if fault {
                    Err(ArithmeticError::FloatingPoint(mxcsr | flags))
                } else {
                    Ok(())
                }
            );
            assert_eq!(vector, expected);
        }
    }
}

#[test]
fn dot_masks_reduce_separately_per_128_bits_and_do_not_fuse_products() {
    let sums: [u32; 16] = [
        0, 0x3f800000, 0x40000000, 0x40400000, 0x40800000, 0x40a00000, 0x40c00000, 0x40e00000,
        0x41000000, 0x41100000, 0x41200000, 0x41300000, 0x41400000, 0x41500000, 0x41600000,
        0x41700000,
    ];
    for immediate in 0..=255_u8 {
        for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
            let mut vector = state();
            vector.publish(
                register(1),
                lanes(
                    &[
                        0x3f800000, 0x40000000, 0x40800000, 0x41000000, 0x41800000, 0x42000000,
                        0x42800000, 0x43000000,
                    ],
                    4,
                ),
            );
            vector.publish(register(2), lanes(&[0x3f800000], 4));
            let mut expected = vector;
            let mut result = [0; 32];
            let mask = usize::from(immediate >> 4);
            for lane in 0..width.bytes() / 4 {
                if immediate & (1 << (lane % 4)) != 0 {
                    let raw = sums[mask]
                        + if lane >= 4 && mask != 0 {
                            0x02000000
                        } else {
                            0
                        };
                    result[lane * 4..lane * 4 + 4].copy_from_slice(&raw.to_le_bytes());
                }
            }
            expected.publish(register(0), result);
            vector
                .execute_compound(
                    CompoundInstruction::DotSingle {
                        width,
                        destination: register(0),
                        first: register(1),
                        second: MoveSource::Register(register(2)),
                        immediate,
                    },
                    &[],
                )
                .unwrap();
            assert_eq!(vector, expected, "{width:?}/imm{immediate}");
        }
        let mut vector = state();
        vector.publish(
            register(1),
            lanes(&[0x3ff0000000000000, 0x4000000000000000], 8),
        );
        vector.publish(register(2), lanes(&[0x3ff0000000000000], 8));
        let mut expected = vector;
        let mut result = [0; 32];
        let sum = [
            0_u64,
            0x3ff0000000000000,
            0x4000000000000000,
            0x4008000000000000,
        ][usize::from((immediate >> 4) & 3)];
        for lane in 0..2 {
            if immediate & (1 << lane) != 0 {
                result[lane * 8..lane * 8 + 8].copy_from_slice(&sum.to_le_bytes());
            }
        }
        expected.publish(register(0), result);
        vector
            .execute_compound(
                CompoundInstruction::DotDouble {
                    destination: register(0),
                    first: register(1),
                    second: MoveSource::Register(register(2)),
                    immediate,
                },
                &[],
            )
            .unwrap();
        assert_eq!(vector, expected);
    }
    let mut vector = state();
    vector.publish(register(1), lanes(&[0x3f800001, 0xbf800000, 0, 0], 4));
    vector.publish(
        register(2),
        lanes(&[0x3f7ffffe, 0x3f800000, 0x3f800000, 0x3f800000], 4),
    );
    let mut expected = vector;
    expected.publish(register(0), [0; 32]);
    expected.simd.mxcsr |= 0x20;
    vector
        .execute_compound(
            CompoundInstruction::DotSingle {
                width: VectorWidth::Xmm,
                destination: register(0),
                first: register(1),
                second: MoveSource::Register(register(2)),
                immediate: 0xff,
            },
            &[],
        )
        .unwrap();
    assert_eq!(vector, expected);
}

#[test]
fn dot_product_stage_covers_both_halves_and_never_publishes_a_partial_vector() {
    for (mxcsr, expected_mxcsr) in [(0x0f00, 0x0f01), (0x1f00, 0x1f01)] {
        let mut vector = state();
        vector.simd.mxcsr = mxcsr;
        vector.publish(
            register(1),
            lanes(&[0x4b800000, 0x3f800000, 0, 0, 0x7f800001, 0, 0, 0], 4),
        );
        vector.publish(register(2), lanes(&[0x3f800000], 4));
        let mut expected = vector;
        expected.simd.mxcsr = expected_mxcsr;
        assert_eq!(
            vector.execute_compound(
                CompoundInstruction::DotSingle {
                    width: VectorWidth::Ymm,
                    destination: register(0),
                    first: register(1),
                    second: MoveSource::Register(register(2)),
                    immediate: 0xff
                },
                &[]
            ),
            Err(ArithmeticError::FloatingPoint(expected_mxcsr))
        );
        assert_eq!(vector, expected);
    }
    let mut vector = state();
    vector.simd.mxcsr = 0;
    vector.publish(
        register(1),
        lanes(&[0x7f800001, 1, 0x7f800000, 0xff800000], 4),
    );
    vector.publish(register(2), lanes(&[0], 4));
    let mut expected = vector;
    expected.publish(register(0), [0; 32]);
    vector
        .execute_compound(
            CompoundInstruction::DotSingle {
                width: VectorWidth::Ymm,
                destination: register(0),
                first: register(1),
                second: MoveSource::Register(register(2)),
                immediate: 0x0f,
            },
            &[],
        )
        .unwrap();
    assert_eq!(vector, expected);
    let mut vector = state();
    vector.simd.mxcsr = 0x1f00;
    vector.publish(register(1), lanes(&[0x7f800001], 4));
    vector.publish(register(2), lanes(&[0x3f800000], 4));
    let mut expected = vector;
    expected.simd.mxcsr = 0x1f01;
    assert_eq!(
        vector.execute_compound(
            CompoundInstruction::DotSingle {
                width: VectorWidth::Xmm,
                destination: register(0),
                first: register(1),
                second: MoveSource::Register(register(2)),
                immediate: 0xf0
            },
            &[]
        ),
        Err(ArithmeticError::FloatingPoint(0x1f01))
    );
    assert_eq!(vector, expected);
}

#[test]
fn dot_pair_add_stage_aggregates_priority_before_final_sum() {
    for (width, input) in [
        (
            VectorWidth::Xmm,
            [0x4b800000, 0x3f800000, 0x7f800000, 0xff800000, 0, 0, 0, 0],
        ),
        (
            VectorWidth::Ymm,
            [0x4b800000, 0x3f800000, 0, 0, 0, 0, 0x7f800000, 0xff800000],
        ),
    ] {
        for destination in 0..3 {
            for (mxcsr, expected_mxcsr) in [(0x0f00, 0x0f01), (0x1f00, 0x1f01)] {
                let mut vector = state();
                vector.simd.mxcsr = mxcsr;
                vector.publish(register(1), lanes(&input, 4));
                vector.publish(register(2), lanes(&[0x3f800000], 4));
                let mut expected = vector;
                expected.simd.mxcsr = expected_mxcsr;
                assert_eq!(
                    vector.execute_compound(
                        CompoundInstruction::DotSingle {
                            width,
                            destination: register(destination),
                            first: register(1),
                            second: MoveSource::Register(register(2)),
                            immediate: 0xff,
                        },
                        &[],
                    ),
                    Err(ArithmeticError::FloatingPoint(expected_mxcsr))
                );
                assert_eq!(vector, expected);
            }
        }
    }
    for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
        let mut vector = state();
        vector.publish(
            register(1),
            lanes(&[0x4b800000, 0x3f800000, 0x7f800000, 0xff800000], 4),
        );
        vector.publish(register(2), lanes(&[0x3f800000], 4));
        let mut expected = vector;
        let mut result = lanes(&[0xffc00000], 4);
        result[width.bytes()..].fill(0);
        expected.publish(register(0), result);
        expected.simd.mxcsr = 0x1fa1;
        vector
            .execute_compound(
                CompoundInstruction::DotSingle {
                    width,
                    destination: register(0),
                    first: register(1),
                    second: MoveSource::Register(register(2)),
                    immediate: 0xff,
                },
                &[],
            )
            .unwrap();
        assert_eq!(vector, expected);
    }
}

#[test]
fn exact_operand_shapes_and_host_refusals_are_effect_neutral() {
    let operations = [
        (
            CompoundInstruction::Pairwise {
                kind: PairwiseKind::AddSubtract,
                width: VectorWidth::Xmm,
                element: Element::Single,
                destination: register(0),
                first: register(1),
                second: MoveSource::Memory,
            },
            16,
        ),
        (
            CompoundInstruction::Pairwise {
                kind: PairwiseKind::HorizontalSubtract,
                width: VectorWidth::Ymm,
                element: Element::Double,
                destination: register(0),
                first: register(1),
                second: MoveSource::Memory,
            },
            32,
        ),
        (
            CompoundInstruction::RoundScalar {
                element: Element::Single,
                destination: register(0),
                merge: register(1),
                source: MoveSource::Memory,
                immediate: 8,
            },
            4,
        ),
        (
            CompoundInstruction::RoundScalar {
                element: Element::Double,
                destination: register(0),
                merge: register(1),
                source: MoveSource::Memory,
                immediate: 8,
            },
            8,
        ),
        (
            CompoundInstruction::RoundPacked {
                width: VectorWidth::Ymm,
                element: Element::Double,
                destination: register(0),
                source: MoveSource::Memory,
                immediate: 8,
            },
            32,
        ),
        (
            CompoundInstruction::DotDouble {
                destination: register(0),
                first: register(1),
                second: MoveSource::Memory,
                immediate: 0,
            },
            16,
        ),
    ];
    for (operation, bytes) in operations {
        assert_eq!(
            operation.memory(),
            Some(MemoryRequirement {
                access: Access::Read,
                bytes: bytes as u8,
                alignment: 1
            })
        );
        for length in [0, bytes - 1, bytes + 1] {
            let mut vector = state();
            let saved = vector;
            assert_eq!(
                vector.execute_compound(operation, &vec![0; length]),
                Err(ArithmeticError::OperandLength)
            );
            assert_eq!(vector, saved);
        }
        let mut vector = state();
        vector.simd.mxcsr |= 0x10000;
        let saved = vector;
        assert_eq!(
            vector.execute_compound(operation, &vec![0; bytes]),
            Err(ArithmeticError::InvalidState)
        );
        assert_eq!(vector, saved);
    }
}
