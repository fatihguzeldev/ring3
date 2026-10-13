use super::*;

#[test]
fn whole_sources_report_full_read_and_refuse_incomplete_host_operands_atomically() {
    for operation in [
        PermuteInstruction::WholeDwords {
            destination: VectorRegister(0),
            indices: VectorRegister(0),
            source: MoveSource::Memory,
        },
        PermuteInstruction::WholeQwords {
            destination: VectorRegister(0),
            source: MoveSource::Memory,
            immediate: 0,
        },
    ] {
        assert!(operation.requires_avx2());
        assert_eq!(
            operation.memory(),
            Some(MemoryRequirement {
                access: Access::Read,
                bytes: 32,
                alignment: 1
            })
        );
        let mut state = VectorState::default();
        state.simd.xmm[0] = [0x91; 16];
        state.upper[0] = [0x35; 16];
        let mut general = State32::default();
        for length in [0, 16, 31, 33] {
            let before = state;
            let before_general = general;
            let mut operand = vec![0; length];
            let before_operand = operand.clone();
            assert_eq!(
                state.execute_permute(operation, &mut general, &mut operand),
                Err(PermuteError::OperandLength)
            );
            assert_eq!(state.simd, before.simd);
            assert_eq!(state.upper, before.upper);
            assert_eq!(general, before_general);
            assert_eq!(operand, before_operand);
        }
        state.simd.mxcsr = 0x1ffff;
        let before = state;
        let before_general = general;
        let mut operand = [0x7f; 32];
        assert_eq!(
            state.execute_permute(operation, &mut general, &mut operand),
            Err(PermuteError::InvalidState)
        );
        assert_eq!(state.simd, before.simd);
        assert_eq!(state.upper, before.upper);
        assert_eq!(general, before_general);
        assert_eq!(operand, [0x7f; 32]);
    }
    for operation in [
        PermuteInstruction::Immediate {
            element: Element::Double,
            width: VectorWidth::Ymm,
            destination: VectorRegister(0),
            source: MoveSource::Memory,
            immediate: 0,
        },
        PermuteInstruction::Variable {
            element: Element::Single,
            width: VectorWidth::Ymm,
            destination: VectorRegister(0),
            source: VectorRegister(1),
            control: MoveSource::Memory,
        },
    ] {
        assert!(!operation.requires_avx2());
    }
}
