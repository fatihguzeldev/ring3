use super::*;

#[test]
fn half_controls_remap_only_wide_sad_and_preserve_raw_encoding() {
    for immediate in 0_u8..=255 {
        let operation =
            MultiplyOperation::new(PackedKind::MultipleSumAbsoluteByteDifferences, 1, immediate)
                .unwrap();
        for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
            for second in [MoveSource::Memory, MoveSource::Register(VectorRegister(2))] {
                let instruction = MultiplyInstruction::with_width(
                    width,
                    operation,
                    VectorRegister(0),
                    Some(VectorRegister(1)),
                    second,
                )
                .unwrap();
                assert_eq!(instruction.operation(), operation);
                assert_eq!(
                    instruction.memory(),
                    (second == MoveSource::Memory).then_some(MemoryRequirement {
                        access: Access::Read,
                        bytes: width.bytes() as u8,
                        alignment: 1
                    })
                );
                for upper in [false, true] {
                    let expected = if width == VectorWidth::Ymm {
                        MultiplyOperation::new(
                            PackedKind::MultipleSumAbsoluteByteDifferences,
                            1,
                            immediate >> if upper { 3 } else { 0 } & 7,
                        )
                        .unwrap()
                    } else {
                        operation
                    };
                    assert_eq!(instruction.operation_for_half(upper), expected);
                }
                assert!(
                    MultiplyInstruction::with_width(
                        width,
                        operation,
                        VectorRegister(0),
                        None,
                        second
                    )
                    .is_none()
                );
            }
        }
    }
    let operation = MultiplyOperation::new(PackedKind::MultiplyAddWords, 2, 0).unwrap();
    for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
        let instruction = MultiplyInstruction::with_width(
            width,
            operation,
            VectorRegister(0),
            Some(VectorRegister(1)),
            MoveSource::Register(VectorRegister(2)),
        )
        .unwrap();
        assert_eq!(instruction.operation_for_half(false), operation);
        assert_eq!(instruction.operation_for_half(true), operation);
    }
}
