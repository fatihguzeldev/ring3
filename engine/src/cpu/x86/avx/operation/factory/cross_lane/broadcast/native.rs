use super::*;

const TEMPLATES: &[(u8, BroadcastElement, VectorWidth, bool)] = &[
    (8, BroadcastElement::Byte, VectorWidth::Xmm, true),
    (9, BroadcastElement::Byte, VectorWidth::Ymm, true),
    (10, BroadcastElement::Word, VectorWidth::Xmm, true),
    (11, BroadcastElement::Word, VectorWidth::Ymm, true),
    (12, BroadcastElement::Qword, VectorWidth::Xmm, true),
    (13, BroadcastElement::Dword, VectorWidth::Xmm, false),
    (14, BroadcastElement::Dword, VectorWidth::Ymm, false),
    (15, BroadcastElement::Qword, VectorWidth::Ymm, false),
];

#[test]
fn templates_capture_only_declared_sources_and_exact_memory_shapes() {
    for &(kind, element, width, memory) in TEMPLATES {
        for destination in 0..8 {
            for source in 0..8 {
                let operands = destination | source << 6;
                let instruction =
                    AvxInstruction::decode_descriptor(0x01000e00 | u32::from(kind), operands, 0)
                        .unwrap();
                assert_eq!(
                    instruction.encode(),
                    (0x01000e00 | u32::from(kind), operands, 0)
                );
                assert_eq!(instruction.memory(), MemoryEffect::None);
                let expected = CrossLaneInstruction::Broadcast {
                    element,
                    width,
                    destination: VectorRegister(destination as u8),
                    source: MoveSource::Register(VectorRegister(source as u8)),
                };
                assert!(expected.requires_avx2());
                assert_eq!(instruction.semantic(), AvxSemantic::CrossLane(expected));
            }
            let result = AvxInstruction::decode_descriptor(
                0x01000e00 | u32::from(kind),
                destination | 1 << 12,
                0,
            );
            if memory {
                let instruction = result.unwrap();
                let expected = CrossLaneInstruction::Broadcast {
                    element,
                    width,
                    destination: VectorRegister(destination as u8),
                    source: MoveSource::Memory,
                };
                assert!(expected.requires_avx2());
                assert_eq!(instruction.semantic(), AvxSemantic::CrossLane(expected));
                assert_eq!(
                    instruction.memory(),
                    MemoryEffect::Full(MemoryRequirement {
                        access: crate::memory::Access::Read,
                        bytes: element.bytes(),
                        alignment: 1,
                    })
                );
            } else {
                assert_eq!(result, Err(AvxDecodeError::Operands));
            }
        }
    }
}

#[test]
fn unused_slots_immediates_and_unknown_kinds_keep_closed_error_priority() {
    for &(kind, _, _, memory) in TEMPLATES {
        let descriptor = 0x01000e00 | u32::from(kind);
        for extra in [1 << 3, 1 << 9, 1 << 13] {
            assert_eq!(
                AvxInstruction::decode_descriptor(descriptor, 1 | 3 << 6 | extra, 0),
                Err(AvxDecodeError::Operands)
            );
        }
        assert_eq!(
            AvxInstruction::decode_descriptor(descriptor, 1 | 3 << 6, 1),
            Err(AvxDecodeError::Immediate)
        );
        if memory {
            assert_eq!(
                AvxInstruction::decode_descriptor(descriptor, 1 | 1 << 12 | 1 << 6, 0),
                Err(AvxDecodeError::Operands)
            );
        }
    }
    for kind in [0].into_iter().chain(16..=255) {
        // family selection still precedes template-level unused slots/immediate checks.
        assert_eq!(
            AvxInstruction::decode_descriptor(0x01000e00 | kind, 1 << 3, 1),
            Err(AvxDecodeError::Family)
        );
    }
}

#[test]
fn original_scalar_memory_descriptors_keep_required_memory_and_avx1_semantics() {
    for (kind, element, width) in [
        (1, BroadcastElement::Dword, VectorWidth::Xmm),
        (2, BroadcastElement::Dword, VectorWidth::Ymm),
        (3, BroadcastElement::Qword, VectorWidth::Ymm),
    ] {
        let descriptor = 0x01000e00 | kind;
        for destination in 0..8 {
            let operands = destination | 1 << 12;
            let instruction = AvxInstruction::decode_descriptor(descriptor, operands, 0).unwrap();
            let expected = CrossLaneInstruction::Broadcast {
                element,
                width,
                destination: VectorRegister(destination as u8),
                source: MoveSource::Memory,
            };
            assert_eq!(instruction.encode(), (descriptor, operands, 0));
            assert_eq!(instruction.semantic(), AvxSemantic::CrossLane(expected));
            assert!(!expected.requires_avx2());
            for invalid in [
                destination,
                operands | 1 << 6,
                operands | 1 << 3,
                operands | 1 << 9,
                operands | 1 << 13,
            ] {
                assert_eq!(
                    AvxInstruction::decode_descriptor(descriptor, invalid, 0),
                    Err(AvxDecodeError::Operands)
                );
            }
            assert_eq!(
                AvxInstruction::decode_descriptor(descriptor, operands, 1),
                Err(AvxDecodeError::Immediate)
            );
        }
    }
    for (kind, operands, immediate) in [
        (4, 1 | 1 << 12, 0),
        (5, 1 | 2 << 3 | 3 << 6, 0xff),
        (6, 1 | 2 << 3 | 3 << 6, 0xff),
        (7, 1 | 3 << 6, 0xff),
    ] {
        let instruction =
            AvxInstruction::decode_descriptor(0x01000e00 | kind, operands, immediate).unwrap();
        let AvxSemantic::CrossLane(value) = instruction.semantic() else {
            panic!("cross-lane descriptor")
        };
        assert!(
            !value.requires_avx2(),
            "shared old template{kind} remains an avx1 semantic"
        );
        assert_eq!(
            instruction.encode(),
            (0x01000e00 | kind, operands, immediate)
        );
    }
}
