use super::*;

#[test]
fn unary_minimum_position_remains_xmm_only() {
    let operation = MultiplyOperation::new(PackedKind::MinimumPositionUnsignedWord, 2, 0).unwrap();
    let first = None;
    for second in [MoveSource::Memory, MoveSource::Register(VectorRegister(2))] {
        let old = MultiplyInstruction::new(operation, VectorRegister(0), first, second).unwrap();
        assert_eq!(old.width(), VectorWidth::Xmm);
        assert_eq!(
            MultiplyInstruction::with_width(
                VectorWidth::Xmm,
                operation,
                VectorRegister(0),
                first,
                second
            ),
            Some(old)
        );
        assert!(
            MultiplyInstruction::with_width(
                VectorWidth::Ymm,
                operation,
                VectorRegister(0),
                first,
                second
            )
            .is_none()
        );
    }
}

#[test]
fn operand_span_and_host_state_fail_before_either_half_is_published() {
    let operation = MultiplyOperation::new(PackedKind::MultiplyLow, 4, 0).unwrap();
    for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
        let instruction = MultiplyInstruction::with_width(
            width,
            operation,
            VectorRegister(0),
            Some(VectorRegister(0)),
            MoveSource::Memory,
        )
        .unwrap();
        assert_eq!(
            instruction.memory(),
            Some(MemoryRequirement {
                access: Access::Read,
                bytes: width.bytes() as u8,
                alignment: 1
            })
        );
        let mut state = VectorState::default();
        state.simd.xmm[0] = [0x91; 16];
        state.upper[0] = [0x35; 16];
        for length in [0, width.bytes() - 1, width.bytes() + 1] {
            let before = state;
            assert_eq!(
                state.execute_multiply(instruction, &vec![0; length]),
                Err(MultiplyError::OperandLength)
            );
            assert_eq!(state, before);
        }
        state.simd.mxcsr = 0x1ffff;
        let before = state;
        assert_eq!(
            state.execute_multiply(instruction, &vec![0; width.bytes()]),
            Err(MultiplyError::InvalidState)
        );
        assert_eq!(state, before);
    }
    assert!(
        MultiplyInstruction::with_width(
            VectorWidth::Ymm,
            operation,
            VectorRegister(0),
            None,
            MoveSource::Memory
        )
        .is_none()
    );
}
