use super::*;
use crate::memory::Access;

#[test]
fn appended_element_kinds_keep_closed_slots_and_exact_vector_memory_shape() {
    for (kind, packed, lane) in [
        (19, PackedKind::ShiftRight, 4),
        (20, PackedKind::ShiftRight, 8),
        (21, PackedKind::ShiftRightSigned, 4),
        (22, PackedKind::ShiftLeft, 4),
        (23, PackedKind::ShiftLeft, 8),
    ] {
        for (width, bit) in [(VectorWidth::Xmm, 0), (VectorWidth::Ymm, 0x80)] {
            for operands in 0..=0x3fff {
                for immediate in [0, 1, 255] {
                    let descriptor = 0x01000700 | kind | bit;
                    let actual = AvxInstruction::decode_descriptor(descriptor, operands, immediate);
                    if operands & ((7 << 9) | (1 << 13)) != 0
                        || operands & (1 << 12) != 0 && operands & (7 << 6) != 0
                    {
                        assert_eq!(actual, Err(AvxDecodeError::Operands));
                        continue;
                    }
                    if immediate != 0 {
                        assert_eq!(actual, Err(AvxDecodeError::Immediate));
                        continue;
                    }
                    let value = actual.unwrap();
                    assert_eq!(value.encode(), (descriptor, operands, immediate));
                    let AvxSemantic::Shift(instruction) = value.semantic() else {
                        panic!("shift family")
                    };
                    assert_eq!(instruction.width(), width);
                    assert_eq!(
                        instruction.operation(),
                        ShiftOperation::lanes(packed, lane).unwrap()
                    );
                    let count = if operands & (1 << 12) != 0 {
                        MoveSource::Memory
                    } else {
                        MoveSource::Register(VectorRegister(((operands >> 6) & 7) as u8))
                    };
                    assert_eq!(instruction.count(), ShiftCount::Elements(count));
                    assert!(instruction.requires_avx2());
                    assert_eq!(instruction.destination().index(), (operands & 7) as usize);
                    assert_eq!(instruction.source().index(), ((operands >> 3) & 7) as usize);
                    assert_eq!(
                        value.memory(),
                        if count == MoveSource::Memory {
                            MemoryEffect::Full(MemoryRequirement {
                                access: Access::Read,
                                bytes: width.bytes() as u8,
                                alignment: 1,
                            })
                        } else {
                            MemoryEffect::None
                        }
                    );
                    assert_eq!(value.inputs(), [None, None]);
                    assert_eq!(value.output(), None);
                    assert!(!value.writes_flags());
                }
            }
        }
    }
    for kind in (0..=255).filter(|kind| !(1..=23).contains(kind) && !(0x81..=0x97).contains(kind)) {
        for operands in [0, 1 << 9, 1 << 12, 1 << 13, 0x3fff] {
            for immediate in [0, 1, 255] {
                assert_eq!(
                    AvxInstruction::decode_descriptor(0x01000700 | kind, operands, immediate),
                    Err(AvxDecodeError::Family)
                );
            }
        }
    }
}
