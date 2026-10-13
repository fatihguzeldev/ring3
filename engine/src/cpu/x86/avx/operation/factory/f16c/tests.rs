use super::*;
use crate::memory::Access;

#[test]
fn descriptors_roundtrip_exact_width_and_canonical_operand_roles() {
    for kind in 1..=4 {
        for destination in 0..8 {
            for source in 0..8 {
                let descriptor = 0x01001200 | kind;
                let operands = destination | source << 6;
                let immediate = if kind <= 2 { 0 } else { 255 };
                let value =
                    AvxInstruction::decode_descriptor(descriptor, operands, immediate).unwrap();
                assert_eq!(value.encode(), (descriptor, operands, immediate));
                assert_eq!(value.memory(), MemoryEffect::None);
                assert_eq!(value.inputs(), [None, None]);
                assert_eq!(value.output(), None);
                assert!(!value.writes_flags());
                let memory_operands = if kind <= 2 {
                    destination | 1 << 12
                } else {
                    source << 6 | 1 << 13
                };
                let value =
                    AvxInstruction::decode_descriptor(descriptor, memory_operands, immediate)
                        .unwrap();
                assert_eq!(
                    value.memory(),
                    MemoryEffect::Full(MemoryRequirement {
                        access: if kind <= 2 {
                            Access::Read
                        } else {
                            Access::Write
                        },
                        bytes: if kind & 1 != 0 { 8 } else { 16 },
                        alignment: 1,
                    })
                );
                assert_eq!(value.encode(), (descriptor, memory_operands, immediate));
            }
        }
    }
}

#[test]
fn descriptors_reject_unknown_kinds_unused_slots_and_wrong_memory_direction() {
    for kind in [0, 5, 255] {
        assert_eq!(
            AvxInstruction::decode_descriptor(0x01001200 | kind, 0, 0),
            Err(AvxDecodeError::Family)
        );
    }
    for kind in 1..=4 {
        let descriptor = 0x01001200 | kind;
        for unused in [1 << 3, 1 << 9] {
            assert_eq!(
                AvxInstruction::decode_descriptor(descriptor, unused, 0),
                Err(AvxDecodeError::Operands)
            );
        }
        let wrong = if kind <= 2 { 1 << 13 } else { 1 << 12 };
        assert_eq!(
            AvxInstruction::decode_descriptor(descriptor, wrong, 0),
            Err(AvxDecodeError::Operands)
        );
        let noncanonical = if kind <= 2 {
            1 << 12 | 1 << 6
        } else {
            1 << 13 | 1
        };
        assert_eq!(
            AvxInstruction::decode_descriptor(descriptor, noncanonical, 0),
            Err(AvxDecodeError::Operands)
        );
        assert_eq!(
            AvxInstruction::decode_descriptor(descriptor, 0, 256),
            Err(AvxDecodeError::Immediate)
        );
    }
    assert_eq!(
        AvxInstruction::decode_descriptor(0x01001201, 0, 1),
        Err(AvxDecodeError::Immediate)
    );
}
