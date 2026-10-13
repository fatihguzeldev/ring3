use super::*;
use crate::cpu::x86::simd::PackedKind as P;

#[test]
fn element_counts_close_shapes_and_distinguish_uniform_memory_footprints() {
    for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
        for kind in [P::ShiftLeft, P::ShiftRight, P::ShiftRightSigned] {
            for lane in [2, 4, 8] {
                let Some(operation) = ShiftOperation::lanes(kind, lane) else {
                    continue;
                };
                for source in [MoveSource::Memory, MoveSource::Register(VectorRegister(2))] {
                    let instruction = ShiftInstruction::with_width(
                        width,
                        operation,
                        VectorRegister(0),
                        VectorRegister(1),
                        ShiftCount::Elements(source),
                    );
                    let allowed = lane != 2 && !(kind == P::ShiftRightSigned && lane == 8);
                    assert_eq!(instruction.is_some(), allowed);
                    if let Some(instruction) = instruction {
                        assert!(instruction.requires_avx2());
                        assert_eq!(
                            instruction.memory(),
                            (source == MoveSource::Memory).then_some(MemoryRequirement {
                                access: Access::Read,
                                bytes: width.bytes() as u8,
                                alignment: 1
                            })
                        );
                    }
                }
                let uniform = ShiftInstruction::with_width(
                    width,
                    operation,
                    VectorRegister(0),
                    VectorRegister(1),
                    ShiftCount::Memory,
                )
                .unwrap();
                assert_eq!(uniform.memory().unwrap().bytes, 16);
                assert_eq!(uniform.requires_avx2(), width == VectorWidth::Ymm);
            }
        }
        for left in [false, true] {
            assert!(
                ShiftInstruction::with_width(
                    width,
                    ShiftOperation::bytes(left),
                    VectorRegister(0),
                    VectorRegister(1),
                    ShiftCount::Elements(MoveSource::Memory)
                )
                .is_none()
            );
        }
    }
}

#[test]
fn memory_length_and_invalid_state_fail_before_any_element_publication() {
    for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
        let instruction = ShiftInstruction::with_width(
            width,
            ShiftOperation::lanes(P::ShiftLeft, 8).unwrap(),
            VectorRegister(0),
            VectorRegister(0),
            ShiftCount::Elements(MoveSource::Memory),
        )
        .unwrap();
        let mut state = VectorState::default();
        state.simd.xmm[0] = [0x91; 16];
        state.upper[0] = [0x35; 16];
        for length in [0, 8, width.bytes() - 1, width.bytes() + 1] {
            let before = state;
            assert_eq!(
                state.execute_shift(instruction, &vec![0; length]),
                Err(ShiftError::OperandLength)
            );
            assert_eq!(state.simd, before.simd);
            assert_eq!(state.upper, before.upper);
        }
        state.simd.mxcsr = 0x1ffff;
        let before = state;
        assert_eq!(
            state.execute_shift(instruction, &vec![0; width.bytes()]),
            Err(ShiftError::InvalidState)
        );
        assert_eq!(state.simd, before.simd);
        assert_eq!(state.upper, before.upper);
    }
}
