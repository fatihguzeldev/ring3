use super::super::*;

fn register(index: u8) -> VectorRegister {
    VectorRegister::new(index).unwrap()
}

fn state() -> VectorState {
    VectorState {
        simd: crate::cpu::x86::simd::SimdState {
            mxcsr: 0x1f80,
            xmm: std::array::from_fn(|index| [0x30 + index as u8; 16]),
        },
        upper: std::array::from_fn(|index| [0x70 + index as u8; 16]),
    }
}

#[test]
fn logical_memory_sources_preserve_raw_bits_and_first_source_aliases() {
    for kind in [
        LogicalOperation::And,
        LogicalOperation::AndNot,
        LogicalOperation::Or,
        LogicalOperation::Xor,
    ] {
        for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
            for destination in [0, 1] {
                let mut current = state();
                let before = current;
                let operand: [u8; 32] = std::array::from_fn(|i| (i as u8).wrapping_mul(17));
                let first = before.read(register(1));
                let mut value = [0; 32];
                for i in 0..width.bytes() {
                    value[i] = match kind {
                        LogicalOperation::And => first[i] & operand[i],
                        LogicalOperation::AndNot => !first[i] & operand[i],
                        LogicalOperation::Or => first[i] | operand[i],
                        LogicalOperation::Xor => first[i] ^ operand[i],
                    };
                }
                let mut expected = before;
                expected.publish(register(destination), value);
                current
                    .execute_with_operand(
                        Instruction::Logical(LogicalInstruction {
                            kind,
                            width,
                            destination: register(destination),
                            first: register(1),
                            second: MoveSource::Memory,
                        }),
                        &operand[..width.bytes()],
                    )
                    .unwrap();
                assert_eq!(current, expected);
            }
        }
    }
}
