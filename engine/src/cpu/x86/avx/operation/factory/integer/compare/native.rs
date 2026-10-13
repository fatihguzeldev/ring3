use super::*;
use crate::memory::Access;

const COMPARISONS: [(u32, PackedKind, u8); 8] = [
    (1, PackedKind::GreaterSigned, 1),
    (2, PackedKind::GreaterSigned, 2),
    (3, PackedKind::GreaterSigned, 4),
    (4, PackedKind::Equal, 1),
    (5, PackedKind::Equal, 2),
    (6, PackedKind::Equal, 4),
    (35, PackedKind::Equal, 8),
    (36, PackedKind::GreaterSigned, 8),
];

#[test]
fn comparison_width_descriptors_preserve_roles_exact_spans_and_slot_policy() {
    for (kind, packed, lane) in COMPARISONS {
        for destination in 0..8 {
            for first in 0..8 {
                for second in 0..8 {
                    let operands = destination | first << 3 | second << 6;
                    for (width, bit) in [(VectorWidth::Xmm, 0), (VectorWidth::Ymm, 0x80)] {
                        let descriptor = 0x0100_0500 | kind | bit;
                        let value =
                            AvxInstruction::decode_descriptor(descriptor, operands, 0).unwrap();
                        assert_eq!(value.encode(), (descriptor, operands, 0));
                        let AvxSemantic::Integer(instruction) = value.semantic() else {
                            panic!("integer family")
                        };
                        assert_eq!(instruction.width(), width);
                        assert_eq!(instruction.operation().kind(), packed);
                        assert_eq!(instruction.operation().lane_bytes(), lane);
                        assert_eq!(instruction.destination().index(), destination as usize);
                        assert_eq!(instruction.first(), Some(VectorRegister(first as u8)));
                        assert_eq!(
                            instruction.second(),
                            MoveSource::Register(VectorRegister(second as u8))
                        );
                        assert_eq!(value.memory(), MemoryEffect::None);
                    }
                }
                let operands = destination | first << 3 | 1 << 12;
                for (bit, bytes) in [(0, 16), (0x80, 32)] {
                    let descriptor = 0x0100_0500 | kind | bit;
                    let value = AvxInstruction::decode_descriptor(descriptor, operands, 0).unwrap();
                    assert_eq!(value.encode(), (descriptor, operands, 0));
                    assert_eq!(
                        value.memory(),
                        MemoryEffect::Full(MemoryRequirement {
                            access: Access::Read,
                            bytes,
                            alignment: 1
                        })
                    );
                    assert_eq!(value.inputs(), [None, None]);
                    assert_eq!(value.output(), None);
                    assert!(!value.writes_flags());
                }
            }
        }
        let descriptor = 0x0100_0580 | kind;
        for operands in [1 << 9, 1 << 13, 1 << 12 | 1 << 6, 1 << 12 | 1 << 13] {
            for immediate in [0, 1, 255] {
                assert_eq!(
                    AvxInstruction::decode_descriptor(descriptor, operands, immediate),
                    Err(AvxDecodeError::Operands)
                );
            }
        }
        for immediate in [1, 255] {
            assert_eq!(
                AvxInstruction::decode_descriptor(descriptor, 0, immediate),
                Err(AvxDecodeError::Immediate)
            );
        }
        assert_eq!(
            AvxInstruction::decode_descriptor(descriptor | (1 << 16), 0, 0),
            Err(AvxDecodeError::ReservedBits)
        );
        assert_eq!(
            AvxInstruction::decode_descriptor(descriptor, 1 << 14, 0),
            Err(AvxDecodeError::ReservedBits)
        );
        assert_eq!(
            AvxInstruction::decode_descriptor(descriptor, 0, 256),
            Err(AvxDecodeError::Immediate)
        );
        assert_eq!(
            AvxInstruction::decode_descriptor(descriptor | 0x40, 0, 0),
            Err(AvxDecodeError::Family)
        );
    }
}
