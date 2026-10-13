use super::*;

fn sign_bytes(pattern: u32, upper: bool) -> [u8; 16] {
    std::array::from_fn(|index| {
        let bit = index + if upper { 16 } else { 0 };
        (index as u8 * 7) | if pattern & (1 << bit) != 0 { 0x80 } else { 0 }
    })
}

#[test]
fn all_thirty_two_byte_signs_publish_only_the_selected_general_register() {
    let registers = [
        Register32::Eax,
        Register32::Ecx,
        Register32::Edx,
        Register32::Ebx,
        Register32::Esp,
        Register32::Ebp,
        Register32::Esi,
        Register32::Edi,
    ];
    for (index, destination) in registers.into_iter().enumerate() {
        for pattern in [0, u32::MAX, 1, 0x8000, 0x10000, 0x80000000, 0xaaaa5555] {
            let source = VectorRegister::new(index as u8).unwrap();
            let mut state = VectorState::default();
            state.simd.mxcsr = 0xffff;
            state.simd.xmm[index] = sign_bytes(pattern, false);
            state.upper[index] = sign_bytes(pattern, true);
            let before = state;
            let mut general = State32 {
                registers: [0xdeadbeef; 8],
                eip: 0x12345678,
                eflags: 0x244cd7,
            };
            for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
                let mut expected = general;
                expected.registers[destination.index()] = if width == VectorWidth::Ymm {
                    pattern
                } else {
                    pattern & 0xffff
                };
                let instruction = ScalarInstruction::Mask {
                    width,
                    source,
                    destination,
                };
                assert_eq!(instruction.memory(), None);
                state
                    .execute_scalar(instruction, &mut general, &mut [])
                    .unwrap();
                assert_eq!(general, expected);
                assert_eq!(state, before);
            }
        }
    }
}

#[test]
fn invalid_state_or_operand_length_cannot_publish_a_general_result() {
    let instruction = ScalarInstruction::Mask {
        width: VectorWidth::Ymm,
        source: VectorRegister::new(0).unwrap(),
        destination: Register32::Eax,
    };
    for (mxcsr, length, error) in [
        (0xffff, 1, ScalarError::OperandLength),
        (0x10000, 0, ScalarError::InvalidState),
    ] {
        let mut state = VectorState::default();
        state.simd.mxcsr = mxcsr;
        state.simd.xmm[0] = [0xff; 16];
        state.upper[0] = [0xff; 16];
        let mut general = State32::default();
        let before = state;
        let general_before = general;
        let mut operand = vec![0xa5; length];
        let operand_before = operand.clone();
        assert_eq!(
            state.execute_scalar(instruction, &mut general, &mut operand),
            Err(error)
        );
        assert_eq!(state, before);
        assert_eq!(general, general_before);
        assert_eq!(operand, operand_before);
    }
}
