use super::*;
use crate::memory::Access;

#[test]
fn whole_templates_keep_exact_source_roles_and_closed_slot_priority() {
    for kind in [31, 32] {
        for operands in 0..=0x3fff {
            for immediate in [0, 1, 255] {
                let descriptor = 0x01000400 | kind;
                let actual = AvxInstruction::decode_descriptor(descriptor, operands, immediate);
                let unused = (7 << 9) | (1 << 13) | if kind == 32 { 7 << 3 } else { 0 };
                if operands & unused != 0 || operands & (1 << 12) != 0 && operands & (7 << 6) != 0 {
                    assert_eq!(actual, Err(AvxDecodeError::Operands));
                    continue;
                }
                if kind == 31 && immediate != 0 {
                    assert_eq!(actual, Err(AvxDecodeError::Immediate));
                    continue;
                }
                let value = actual.unwrap();
                assert_eq!(value.encode(), (descriptor, operands, immediate));
                let source = if operands & (1 << 12) != 0 {
                    MoveSource::Memory
                } else {
                    MoveSource::Register(VectorRegister(((operands >> 6) & 7) as u8))
                };
                let destination = VectorRegister((operands & 7) as u8);
                let operation = if kind == 31 {
                    PermuteInstruction::WholeDwords {
                        destination,
                        indices: VectorRegister(((operands >> 3) & 7) as u8),
                        source,
                    }
                } else {
                    PermuteInstruction::WholeQwords {
                        destination,
                        source,
                        immediate: immediate as u8,
                    }
                };
                assert_eq!(value.semantic(), AvxSemantic::Permute(operation));
                assert!(operation.requires_avx2());
                assert_eq!(
                    value.memory(),
                    if source == MoveSource::Memory {
                        MemoryEffect::Full(MemoryRequirement {
                            access: Access::Read,
                            bytes: 32,
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
    for kind in [0].into_iter().chain(33..=255) {
        assert_eq!(
            AvxInstruction::decode_descriptor(0x01000400 | kind, 1 << 9, 1),
            Err(AvxDecodeError::Family)
        );
    }
}
