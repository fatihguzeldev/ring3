use super::*;

fn register(index: u8) -> VectorRegister {
    VectorRegister::new(index).unwrap()
}
fn state() -> VectorState {
    let mut state = VectorState {
        simd: super::super::SimdState {
            mxcsr: 0xffff,
            ..Default::default()
        },
        ..Default::default()
    };
    for r in 0..8 {
        state.simd.xmm[r] = std::array::from_fn(|i| (r * 23 + i * 11 + 1) as u8);
        state.upper[r] = std::array::from_fn(|i| (r * 31 + i * 7 + 3) as u8);
    }
    state
}

#[test]
fn broadcasts_copy_raw_patterns_with_exact_source_width_and_upper_zeroing() {
    let source = [
        0x01, 0x00, 0x80, 0x7f, 0x00, 0x00, 0x00, 0x80, 0xff, 0xff, 0xff, 0x7f, 0x53, 0xa7, 0xb3,
        0xc9,
    ];
    for destination in 0..8 {
        for (operation, bytes, width) in [
            (
                CrossLaneInstruction::BroadcastSingle {
                    width: VectorWidth::Xmm,
                    destination: register(destination),
                },
                4,
                16,
            ),
            (
                CrossLaneInstruction::BroadcastSingle {
                    width: VectorWidth::Ymm,
                    destination: register(destination),
                },
                4,
                32,
            ),
            (
                CrossLaneInstruction::BroadcastDouble {
                    destination: register(destination),
                },
                8,
                32,
            ),
            (
                CrossLaneInstruction::BroadcastBlock {
                    destination: register(destination),
                },
                16,
                32,
            ),
        ] {
            let mut vector = state();
            let mut operand = source[..bytes].to_vec();
            let mut expected = vector;
            let mut value = [0; 32];
            for (index, byte) in value[..width].iter_mut().enumerate() {
                *byte = source[index % bytes];
            }
            expected.publish(register(destination), value);
            assert_eq!(
                operation.memory(),
                Some(MemoryRequirement {
                    access: Access::Read,
                    bytes: bytes as u8,
                    alignment: 1
                })
            );
            vector.execute_cross_lane(operation, &mut operand).unwrap();
            assert_eq!(vector, expected);
            assert_eq!(operand, source[..bytes]);
        }
    }
}

#[test]
fn permute_all_controls_select_literal_blocks_with_source_destination_aliases() {
    for immediate in 0..=255_u8 {
        for (d, a, b) in [
            (0, 1, Some(2)),
            (1, 1, Some(2)),
            (2, 1, Some(2)),
            (0, 1, Some(1)),
            (0, 0, Some(0)),
            (0, 1, None),
        ] {
            let mut vector = state();
            let mut operand = if b.is_none() {
                (0..32).map(|i| (i * 13 + 0x53) as u8).collect::<Vec<_>>()
            } else {
                vec![]
            };
            let first = vector.read(register(a));
            let second = b.map_or_else(
                || operand.as_slice().try_into().unwrap(),
                |r| vector.read(register(r)),
            );
            let sources = [&first[..16], &first[16..], &second[..16], &second[16..]];
            let low = if immediate & 8 == 0 {
                sources[usize::from(immediate & 3)]
            } else {
                &[0; 16]
            };
            let high = if immediate & 128 == 0 {
                sources[usize::from(immediate >> 4 & 3)]
            } else {
                &[0; 16]
            };
            let mut result = [0; 32];
            result[..16].copy_from_slice(low);
            result[16..].copy_from_slice(high);
            let mut expected = vector;
            expected.publish(register(d), result);
            let saved = operand.clone();
            vector
                .execute_cross_lane(
                    CrossLaneInstruction::Permute {
                        destination: register(d),
                        first: register(a),
                        second: b.map_or(MoveSource::Memory, |r| MoveSource::Register(register(r))),
                        immediate,
                    },
                    &mut operand,
                )
                .unwrap();
            assert_eq!(vector, expected, "imm{immediate}/{d}/{a}/{b:?}");
            assert_eq!(operand, saved);
        }
    }
}

#[test]
fn insert_extract_preserve_other_blocks_ignore_high_immediate_bits_and_snapshot_aliases() {
    for immediate in 0..=255_u8 {
        for (d, a, b) in [
            (0, 1, Some(2)),
            (1, 1, Some(2)),
            (2, 1, Some(2)),
            (0, 0, Some(0)),
            (0, 1, None),
        ] {
            let mut vector = state();
            let mut operand = if b.is_none() {
                (0..16).map(|i| (i * 13 + 0x53) as u8).collect::<Vec<_>>()
            } else {
                vec![]
            };
            let second = b.map_or_else(
                || <[u8; 16]>::try_from(operand.as_slice()).unwrap(),
                |r| vector.simd.xmm[usize::from(r)],
            );
            let mut expected = vector;
            let mut result = vector.read(register(a));
            let offset = usize::from(immediate & 1) * 16;
            result[offset..offset + 16].copy_from_slice(&second);
            expected.publish(register(d), result);
            let saved = operand.clone();
            vector
                .execute_cross_lane(
                    CrossLaneInstruction::Insert {
                        destination: register(d),
                        first: register(a),
                        second: b.map_or(MoveSource::Memory, |r| MoveSource::Register(register(r))),
                        immediate,
                    },
                    &mut operand,
                )
                .unwrap();
            assert_eq!(vector, expected);
            assert_eq!(operand, saved);
        }
        for destination in [
            BlockTarget::Register(register(0)),
            BlockTarget::Register(register(1)),
            BlockTarget::Memory,
        ] {
            let mut vector = state();
            let input = vector.read(register(1));
            let selected =
                &input[usize::from(immediate & 1) * 16..usize::from(immediate & 1) * 16 + 16];
            let mut expected = vector;
            let mut operand = if destination == BlockTarget::Memory {
                vec![0xa7; 16]
            } else {
                vec![]
            };
            let expected_operand = if let BlockTarget::Register(d) = destination {
                let mut result = [0; 32];
                result[..16].copy_from_slice(selected);
                expected.publish(d, result);
                vec![]
            } else {
                selected.to_vec()
            };
            vector
                .execute_cross_lane(
                    CrossLaneInstruction::Extract {
                        source: register(1),
                        destination,
                        immediate,
                    },
                    &mut operand,
                )
                .unwrap();
            assert_eq!(vector, expected);
            assert_eq!(operand, expected_operand);
        }
    }
}

#[test]
fn operand_and_host_state_refusals_precede_vector_or_store_publication() {
    let operations = [
        CrossLaneInstruction::BroadcastSingle {
            width: VectorWidth::Xmm,
            destination: register(0),
        },
        CrossLaneInstruction::BroadcastSingle {
            width: VectorWidth::Ymm,
            destination: register(0),
        },
        CrossLaneInstruction::BroadcastDouble {
            destination: register(0),
        },
        CrossLaneInstruction::BroadcastBlock {
            destination: register(0),
        },
        CrossLaneInstruction::Permute {
            destination: register(0),
            first: register(1),
            second: MoveSource::Memory,
            immediate: 0x88,
        },
        CrossLaneInstruction::Insert {
            destination: register(0),
            first: register(1),
            second: MoveSource::Memory,
            immediate: 0xff,
        },
        CrossLaneInstruction::Extract {
            source: register(1),
            destination: BlockTarget::Memory,
            immediate: 0xff,
        },
    ];
    for operation in operations {
        let requirement = operation.memory().unwrap();
        assert_eq!(requirement.alignment, 1);
        assert_eq!(
            requirement.access,
            if matches!(operation, CrossLaneInstruction::Extract { .. }) {
                Access::Write
            } else {
                Access::Read
            }
        );
        let length = usize::from(requirement.bytes);
        for wrong in [0, length - 1, length + 1] {
            let mut vector = state();
            let before = vector;
            let mut operand = vec![0xa7; wrong];
            let saved = operand.clone();
            assert_eq!(
                vector.execute_cross_lane(operation, &mut operand),
                Err(ArithmeticError::OperandLength)
            );
            assert_eq!(vector, before);
            assert_eq!(operand, saved);
        }
        let mut vector = state();
        vector.simd.mxcsr |= 1 << 16;
        let before = vector;
        let mut operand = vec![0xb3; length];
        assert_eq!(
            vector.execute_cross_lane(operation, &mut operand),
            Err(ArithmeticError::InvalidState)
        );
        assert_eq!(vector, before);
        assert_eq!(operand, vec![0xb3; length]);
    }
    let mut vector = state();
    let before = vector;
    let mut operand = [0xa7];
    assert_eq!(
        vector.execute_cross_lane(
            CrossLaneInstruction::Extract {
                source: register(1),
                destination: BlockTarget::Register(register(0)),
                immediate: 0
            },
            &mut operand
        ),
        Err(ArithmeticError::OperandLength)
    );
    assert_eq!(vector, before);
    assert_eq!(operand, [0xa7]);
}
