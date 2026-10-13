use super::*;
use crate::cpu::x86::simd::PackedKind;

fn register(index: u8) -> VectorRegister {
    VectorRegister::new(index).unwrap()
}

#[test]
fn widths_preserve_count_shapes_and_exact_m128_authority() {
    let mut operations = vec![ShiftOperation::bytes(false), ShiftOperation::bytes(true)];
    for kind in [
        PackedKind::ShiftLeft,
        PackedKind::ShiftRight,
        PackedKind::ShiftRightSigned,
    ] {
        for lane in [2, 4, 8] {
            if let Some(operation) = ShiftOperation::lanes(kind, lane) {
                operations.push(operation);
            }
        }
    }
    assert_eq!(operations.len(), 10);
    let mut valid = 0;
    for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
        for operation in &operations {
            for count in [
                ShiftCount::Immediate(255),
                ShiftCount::Register(register(2)),
                ShiftCount::Memory,
            ] {
                let instruction = ShiftInstruction::with_width(
                    width,
                    *operation,
                    register(0),
                    register(1),
                    count,
                );
                if operation.byte_kind().is_some() && !matches!(count, ShiftCount::Immediate(_)) {
                    assert!(instruction.is_none());
                    continue;
                }
                let instruction = instruction.unwrap();
                assert_eq!(instruction.width(), width);
                assert_eq!(instruction.operation(), *operation);
                assert_eq!(instruction.count(), count);
                assert_eq!(
                    instruction.memory(),
                    if count == ShiftCount::Memory {
                        Some(MemoryRequirement {
                            access: Access::Read,
                            bytes: 16,
                            alignment: 1,
                        })
                    } else {
                        None
                    }
                );
                if width == VectorWidth::Xmm {
                    assert_eq!(
                        Some(instruction),
                        ShiftInstruction::new(*operation, register(0), register(1), count)
                    );
                }
                valid += 1;
            }
        }
    }
    assert_eq!(valid, 52);
    let instruction = ShiftInstruction::with_width(
        VectorWidth::Ymm,
        ShiftOperation::lanes(PackedKind::ShiftLeft, 4).unwrap(),
        register(0),
        register(1),
        ShiftCount::Memory,
    )
    .unwrap();
    let mut state = VectorState::default();
    state.simd.xmm[1] = std::array::from_fn(|byte| byte as u8 + 1);
    state.upper[1] = std::array::from_fn(|byte| byte as u8 + 129);
    for length in [0, 8, 15, 17, 32] {
        let before = state;
        assert_eq!(
            state.execute_shift(instruction, &vec![0; length]),
            Err(ShiftError::OperandLength)
        );
        assert_eq!(state, before);
    }
    state.simd.mxcsr = 0x10000;
    let before = state;
    assert_eq!(
        state.execute_shift(instruction, &[0; 16]),
        Err(ShiftError::InvalidState)
    );
    assert_eq!(state, before);
    state.simd.mxcsr = 0xffff;
    let mut expected = state;
    expected.simd.xmm[0] = state.simd.xmm[1];
    expected.upper[0] = state.upper[1];
    let mut count = [255; 16];
    count[..8].fill(0);
    state.execute_shift(instruction, &count).unwrap();
    assert_eq!(state, expected);
}
