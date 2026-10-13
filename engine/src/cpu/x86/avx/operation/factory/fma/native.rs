use super::*;
use crate::memory::Access;

#[test]
fn all_fused_descriptors_roundtrip_register_roles_and_exact_memory_widths() {
    for kind in 1..=96 {
        let descriptor = 0x0100_1300 | kind;
        for destination in 0..8 {
            for second in 0..8 {
                for third in 0..8 {
                    let operands = destination | second << 3 | third << 6;
                    let value = AvxInstruction::decode_descriptor(descriptor, operands, 0).unwrap();
                    assert_eq!(value.encode(), (descriptor, operands, 0));
                    let AvxSemantic::Fma(instruction) = value.semantic() else {
                        panic!("fused family");
                    };
                    assert_eq!(instruction.destination.index(), destination as usize);
                    assert_eq!(instruction.second.index(), second as usize);
                    assert_eq!(
                        instruction.third,
                        MoveSource::Register(VectorRegister(third as u8))
                    );
                    assert_eq!(value.memory(), MemoryEffect::None);
                    assert_eq!(value.inputs(), [None, None]);
                    assert_eq!(value.output(), None);
                    assert!(!value.writes_flags());
                }
                let operands = destination | second << 3 | 1 << 12;
                let value = AvxInstruction::decode_descriptor(descriptor, operands, 0).unwrap();
                let AvxSemantic::Fma(instruction) = value.semantic() else {
                    panic!("fused family");
                };
                let bytes = match instruction.shape {
                    FmaShape::Packed { width, .. } => width.bytes(),
                    FmaShape::Scalar { element } => element.bytes(),
                };
                assert_eq!(instruction.third, MoveSource::Memory);
                assert_eq!(
                    value.memory(),
                    MemoryEffect::Full(MemoryRequirement {
                        access: Access::Read,
                        bytes: bytes as u8,
                        alignment: 1
                    })
                );
                assert_eq!(value.encode(), (descriptor, operands, 0));
            }
        }
    }
}

#[test]
fn fused_descriptors_refuse_unknown_shapes_unused_slots_and_immediates() {
    for kind in [0, 97, 255] {
        assert_eq!(
            AvxInstruction::decode_descriptor(0x0100_1300 | kind, 0, 0),
            Err(AvxDecodeError::Family)
        );
    }
    for kind in 1..=96 {
        let descriptor = 0x0100_1300 | kind;
        for operands in [1 << 9, 1 << 13, 1 << 12 | 1 << 6, 1 << 12 | 1 << 13] {
            assert_eq!(
                AvxInstruction::decode_descriptor(descriptor, operands, 0),
                Err(AvxDecodeError::Operands)
            );
        }
        for immediate in [1, 255, 256] {
            assert_eq!(
                AvxInstruction::decode_descriptor(descriptor, 0, immediate),
                Err(AvxDecodeError::Immediate)
            );
        }
    }
}
