use super::*;

const ORIGINAL: &[(PackedKind, u8)] = &[
    (PackedKind::MultiplyLow, 2),
    (PackedKind::MultiplyHighUnsigned, 2),
    (PackedKind::MultiplyHighSigned, 2),
    (PackedKind::MultiplyUnsignedDword, 4),
    (PackedKind::MultiplyAddWords, 2),
    (PackedKind::SumAbsoluteByteDifferences, 1),
    (PackedKind::HorizontalAdd, 2),
    (PackedKind::HorizontalAdd, 4),
    (PackedKind::HorizontalAddSignedSaturate, 2),
    (PackedKind::MultiplyAddUnsignedSignedBytes, 1),
    (PackedKind::HorizontalSubtract, 2),
    (PackedKind::HorizontalSubtract, 4),
    (PackedKind::HorizontalSubtractSignedSaturate, 2),
    (PackedKind::MultiplyHighRoundSigned, 2),
    (PackedKind::MultiplySignedDword, 4),
    (PackedKind::MultiplyLow, 4),
    (PackedKind::MinimumPositionUnsignedWord, 2),
    (PackedKind::MultipleSumAbsoluteByteDifferences, 1),
];

#[test]
fn original_mapping_and_slots_survive_width_selection() {
    assert_eq!(ORIGINAL.len(), 18);
    for (index, &(packed, lane)) in ORIGINAL.iter().enumerate() {
        let kind = index as u32 + 1;
        for (width, bit) in [(VectorWidth::Xmm, 0), (VectorWidth::Ymm, 0x80)] {
            if kind > 16 && width == VectorWidth::Ymm {
                continue;
            }
            for destination in 0..8 {
                let first = if kind == 17 { 0 } else { (destination + 3) % 8 };
                for operands in [
                    destination | (first << 3) | (((destination + 1) % 8) << 6),
                    destination | (first << 3) | (1 << 12),
                ] {
                    for immediate in if kind == 18 {
                        &[0, 7, 0x29, 255][..]
                    } else {
                        &[0][..]
                    } {
                        let descriptor = 0x01000800 | bit | kind;
                        let value =
                            AvxInstruction::decode_descriptor(descriptor, operands, *immediate)
                                .unwrap();
                        assert_eq!(value.encode(), (descriptor, operands, *immediate));
                        let AvxSemantic::Multiply(instruction) = value.semantic() else {
                            panic!("multiply family")
                        };
                        assert_eq!(instruction.width(), width);
                        assert_eq!(
                            instruction.operation(),
                            MultiplyOperation::new(packed, lane, *immediate as u8).unwrap()
                        );
                        assert_eq!(instruction.destination().index(), destination as usize);
                        assert_eq!(
                            instruction.first().map(|r| r.index()),
                            (kind != 17).then_some(first as usize)
                        );
                        assert_eq!(
                            value.memory(),
                            if operands & (1 << 12) == 0 {
                                MemoryEffect::None
                            } else {
                                MemoryEffect::Full(MemoryRequirement {
                                    access: crate::memory::Access::Read,
                                    bytes: width.bytes() as u8,
                                    alignment: 1,
                                })
                            }
                        );
                        assert_eq!(value.inputs(), [None, None]);
                        assert_eq!(value.output(), None);
                        assert!(!value.writes_flags());
                    }
                }
            }
        }
    }
}

#[test]
fn unassigned_widths_and_slots_keep_exact_error_priority() {
    for kind in 0..=255 {
        let normalized = kind & 0x7f;
        let assigned = (1..=18).contains(&normalized) && (kind & 0x80 == 0 || normalized <= 16);
        if !assigned {
            for operands in [0, 1 << 9, 1 << 12, 1 << 13, 0x3fff] {
                assert_eq!(
                    AvxInstruction::decode_descriptor(0x01000800 | kind, operands, 255),
                    Err(AvxDecodeError::Family)
                );
            }
            continue;
        }
        let base = 1 | (if normalized == 17 { 0 } else { 2 << 3 }) | (3 << 6);
        for extra in [1 << 9, 1 << 13] {
            assert_eq!(
                AvxInstruction::decode_descriptor(0x01000800 | kind, base | extra, 255),
                Err(AvxDecodeError::Operands)
            );
        }
        assert_eq!(
            AvxInstruction::decode_descriptor(0x01000800 | kind, base | (1 << 12), 0),
            Err(AvxDecodeError::Operands)
        );
        if normalized == 17 {
            assert_eq!(
                AvxInstruction::decode_descriptor(0x01000800 | kind, base | (1 << 3), 0),
                Err(AvxDecodeError::Operands)
            );
        }
        if normalized != 18 {
            assert_eq!(
                AvxInstruction::decode_descriptor(0x01000800 | kind, base, 1),
                Err(AvxDecodeError::Immediate)
            );
        }
    }
}
