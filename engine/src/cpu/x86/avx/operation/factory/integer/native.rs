use super::*;
use crate::memory::Access;

const LEGACY_KINDS: [(PackedKind, u8); 44] = [
    (PackedKind::GreaterSigned, 1),
    (PackedKind::GreaterSigned, 2),
    (PackedKind::GreaterSigned, 4),
    (PackedKind::Equal, 1),
    (PackedKind::Equal, 2),
    (PackedKind::Equal, 4),
    (PackedKind::Add, 8),
    (PackedKind::SubtractUnsignedSaturate, 1),
    (PackedKind::SubtractUnsignedSaturate, 2),
    (PackedKind::MinimumUnsigned, 1),
    (PackedKind::AddUnsignedSaturate, 1),
    (PackedKind::AddUnsignedSaturate, 2),
    (PackedKind::MaximumUnsigned, 1),
    (PackedKind::AverageUnsigned, 1),
    (PackedKind::AverageUnsigned, 2),
    (PackedKind::SubtractSignedSaturate, 1),
    (PackedKind::SubtractSignedSaturate, 2),
    (PackedKind::MinimumSigned, 2),
    (PackedKind::AddSignedSaturate, 1),
    (PackedKind::AddSignedSaturate, 2),
    (PackedKind::MaximumSigned, 2),
    (PackedKind::Subtract, 1),
    (PackedKind::Subtract, 2),
    (PackedKind::Subtract, 4),
    (PackedKind::Subtract, 8),
    (PackedKind::Add, 1),
    (PackedKind::Add, 2),
    (PackedKind::Add, 4),
    (PackedKind::Sign, 1),
    (PackedKind::Sign, 2),
    (PackedKind::Sign, 4),
    (PackedKind::Absolute, 1),
    (PackedKind::Absolute, 2),
    (PackedKind::Absolute, 4),
    (PackedKind::Equal, 8),
    (PackedKind::GreaterSigned, 8),
    (PackedKind::MinimumSigned, 1),
    (PackedKind::MinimumSigned, 4),
    (PackedKind::MinimumUnsigned, 2),
    (PackedKind::MinimumUnsigned, 4),
    (PackedKind::MaximumSigned, 1),
    (PackedKind::MaximumSigned, 4),
    (PackedKind::MaximumUnsigned, 2),
    (PackedKind::MaximumUnsigned, 4),
];

fn expected(
    kind: PackedKind,
    lane_bytes: u8,
    operands: u32,
    immediate: u8,
) -> Result<AvxSemantic, AvxDecodeError> {
    let unary = kind == PackedKind::Absolute;
    let memory = operands & (1 << 12) != 0;
    if operands & (1 << 13) != 0
        || operands & (7 << 9) != 0
        || unary && operands & (7 << 3) != 0
        || memory && operands & (7 << 6) != 0
    {
        return Err(AvxDecodeError::Operands);
    }
    if immediate != 0 {
        return Err(AvxDecodeError::Immediate);
    }
    let destination = VectorRegister((operands & 7) as u8);
    let first = (!unary).then_some(VectorRegister(((operands >> 3) & 7) as u8));
    let second = if memory {
        MoveSource::Memory
    } else {
        MoveSource::Register(VectorRegister(((operands >> 6) & 7) as u8))
    };
    Ok(AvxSemantic::Integer(
        IntegerInstruction::new(
            IntegerLaneOperation::new(kind, lane_bytes).unwrap(),
            destination,
            first,
            second,
        )
        .unwrap(),
    ))
}

#[test]
fn every_legacy_kind_preserves_all_slot_decisions_and_descriptor_metadata() {
    for (index, (kind, lane_bytes)) in LEGACY_KINDS.into_iter().enumerate() {
        let descriptor = 0x0100_0500 | (index as u32 + 1);
        for operands in 0..=0x3fff {
            for immediate in [0, 1, 255] {
                let wanted = expected(kind, lane_bytes, operands, immediate);
                let actual =
                    AvxInstruction::decode_descriptor(descriptor, operands, u32::from(immediate));
                assert_eq!(
                    actual.map(|value| value.semantic()),
                    wanted,
                    "kind={}, operands={operands:#06x}, immediate={immediate}",
                    index + 1,
                );
                if let Ok(value) = actual {
                    assert_eq!(value.encode(), (descriptor, operands, 0));
                    let memory = if operands & (1 << 12) != 0 {
                        MemoryEffect::Full(MemoryRequirement {
                            access: Access::Read,
                            bytes: 16,
                            alignment: 1,
                        })
                    } else {
                        MemoryEffect::None
                    };
                    assert_eq!(value.memory(), memory);
                    assert_eq!(value.inputs(), [None, None]);
                    assert_eq!(value.output(), None);
                    assert!(!value.writes_flags());
                }
            }
        }
    }
}

#[test]
fn every_immediate_preserves_operand_error_priority_for_all_legacy_kinds() {
    for (index, (kind, lane_bytes)) in LEGACY_KINDS.into_iter().enumerate() {
        let descriptor = 0x0100_0500 | (index as u32 + 1);
        for operands in [0, 7, 7 << 6, 1 << 12, 7 | 1 << 12, 1 << 9, 1 << 13, 0x3fff] {
            for immediate in 0..=255 {
                assert_eq!(
                    AvxInstruction::decode_descriptor(descriptor, operands, immediate)
                        .map(|value| value.semantic()),
                    expected(kind, lane_bytes, operands, immediate as u8),
                    "kind={}, operands={operands:#06x}, immediate={immediate}",
                    index + 1,
                );
            }
        }
        assert_eq!(
            AvxInstruction::decode_descriptor(descriptor, 0x3fff, 256),
            Err(AvxDecodeError::Immediate),
        );
        assert_eq!(
            AvxInstruction::decode_descriptor(descriptor, 1 << 14, 0),
            Err(AvxDecodeError::ReservedBits),
        );
    }
}

#[test]
fn unknown_integer_kinds_preserve_family_error_priority() {
    for kind in (0..=255).filter(|kind| !(1..=44).contains(kind)) {
        for operands in [0, 1 << 9, 1 << 12, 1 << 13, 0x3fff] {
            for immediate in [0, 1, 255] {
                assert_eq!(
                    AvxInstruction::decode_descriptor(0x0100_0500 | kind, operands, immediate),
                    Err(AvxDecodeError::Family),
                );
            }
        }
    }
}
