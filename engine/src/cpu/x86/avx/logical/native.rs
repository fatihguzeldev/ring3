use super::super::*;

fn register(index: u8) -> VectorRegister {
    VectorRegister::new(index).unwrap()
}

fn bytes(values: &[u64], element: Element) -> [u8; 32] {
    let mut result = [0; 32];
    for (index, value) in values.iter().enumerate() {
        let offset = index * element.bytes();
        result[offset..offset + element.bytes()]
            .copy_from_slice(&value.to_le_bytes()[..element.bytes()]);
    }
    result
}

fn fill(state: &mut VectorState, index: usize, value: [u8; 32]) {
    state.simd.xmm[index].copy_from_slice(&value[..16]);
    state.upper[index].copy_from_slice(&value[16..]);
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
fn integer_logical_bits_use_both_sources_and_preserve_mxcsr() {
    for (kind, expected) in [
        (LogicalOperation::And, 0x0303_c0c0),
        (LogicalOperation::AndNot, 0x3030_0c0c),
        (LogicalOperation::Or, 0x3f3f_fcfc),
        (LogicalOperation::Xor, 0x3c3c_3c3c),
    ] {
        for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
            let mut current = state();
            current.simd.mxcsr = 0xfff3;
            fill(&mut current, 1, bytes(&[0x0f0f_f0f0; 8], Element::Single));
            fill(&mut current, 0, bytes(&[0x3333_cccc; 8], Element::Single));
            let before = current;
            current
                .execute(Instruction::Logical(LogicalInstruction {
                    kind,
                    width,
                    destination: register(0),
                    first: register(1),
                    second: MoveSource::Register(register(0)),
                }))
                .unwrap();
            assert_eq!(
                current.read(register(0)),
                bytes(&vec![expected; width.bytes() / 4], Element::Single)
            );
            assert_eq!(current.simd.mxcsr, before.simd.mxcsr);
            for index in 1..8 {
                assert_eq!(current.read(register(index)), before.read(register(index)));
            }
        }
    }
}
