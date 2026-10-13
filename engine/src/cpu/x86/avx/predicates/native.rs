use super::*;
use crate::cpu::x86::simd::SimdState;

fn register(index: u8) -> VectorRegister {
    VectorRegister::new(index).unwrap()
}
fn instruction(kind: BitTestKind, width: VectorWidth, memory: bool) -> TestInstruction {
    TestInstruction {
        kind,
        width,
        first: register(0),
        second: if memory {
            MoveSource::Memory
        } else {
            MoveSource::Register(register(1))
        },
    }
}

#[test]
fn sign_tests_ignore_payload_bits_and_include_every_active_element() {
    for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
        for (kind, lane_bytes) in [(BitTestKind::SingleSign, 4), (BitTestKind::DoubleSign, 8)] {
            for bit in 0..256 {
                let mut vectors = VectorState::default();
                let mut value = [0; 32];
                value[bit / 8] = 1 << (bit % 8);
                vectors.simd.xmm[0].copy_from_slice(&value[..16]);
                vectors.upper[0].copy_from_slice(&value[16..]);
                vectors.simd.xmm[1] = vectors.simd.xmm[0];
                vectors.upper[1] = vectors.upper[0];
                let mut state = State32 {
                    eflags: 0x244fd7,
                    ..State32::default()
                };
                let before = vectors;
                vectors
                    .execute_test(instruction(kind, width, false), &mut state, &[])
                    .unwrap();
                let active_sign =
                    bit < width.bytes() * 8 && bit % (lane_bytes * 8) == lane_bytes * 8 - 1;
                assert_eq!(
                    state.eflags,
                    (0x244fd7 & !0x8d5) | 1 | if active_sign { 0 } else { 0x40 },
                    "{kind:?}/{width:?}/{bit}"
                );
                assert_eq!(vectors, before);
            }
        }
    }
}

#[test]
fn packed_test_uses_all_bits_and_directional_and_not() {
    for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
        for bit in 0..width.bytes() * 8 {
            let mut value = [0; 32];
            value[bit / 8] = 1 << (bit % 8);
            for (first, second, literal) in [
                (value, value, 1),
                ([0; 32], value, 0x40),
                (value, [0; 32], 0x41),
            ] {
                let mut vectors = VectorState {
                    simd: SimdState::default(),
                    upper: [[0; 16]; 8],
                };
                vectors.simd.xmm[0].copy_from_slice(&first[..16]);
                vectors.upper[0].copy_from_slice(&first[16..]);
                vectors.simd.xmm[1].copy_from_slice(&second[..16]);
                vectors.upper[1].copy_from_slice(&second[16..]);
                let mut state = State32 {
                    eflags: 0xcd7,
                    ..State32::default()
                };
                vectors
                    .execute_test(
                        instruction(BitTestKind::Bits, width, false),
                        &mut state,
                        &[],
                    )
                    .unwrap();
                assert_eq!(state.eflags, (0xcd7 & !0x8d5) | literal);
            }
        }
    }
}

#[test]
fn exact_memory_and_invalid_state_fail_before_flags_or_vectors_change() {
    for kind in [
        BitTestKind::Bits,
        BitTestKind::SingleSign,
        BitTestKind::DoubleSign,
    ] {
        for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
            let op = instruction(kind, width, true);
            let length = width.bytes();
            assert_eq!(op.memory().unwrap().bytes as usize, length);
            for bad_length in [0, length - 1, length + 1] {
                let vectors = VectorState::default();
                let mut state = State32 {
                    eflags: 0xcd7,
                    ..State32::default()
                };
                let before = state;
                assert_eq!(
                    vectors.execute_test(op, &mut state, &vec![0; bad_length]),
                    Err(ArithmeticError::OperandLength)
                );
                assert_eq!(state, before);
            }
            let mut vectors = VectorState::default();
            vectors.simd.mxcsr |= 1 << 16;
            let before = vectors;
            let mut state = State32 {
                eflags: 0xcd7,
                ..State32::default()
            };
            let saved = state;
            assert_eq!(
                vectors.execute_test(op, &mut state, &vec![0; length]),
                Err(ArithmeticError::InvalidState)
            );
            assert_eq!(state, saved);
            assert_eq!(vectors, before);
            vectors.simd.mxcsr = 0x8040;
            let before = vectors;
            vectors
                .execute_test(op, &mut state, &vec![0; length])
                .unwrap();
            assert_eq!(state.eflags, (0xcd7 & !0x8d5) | 0x41);
            assert_eq!(vectors, before);
        }
    }
}
