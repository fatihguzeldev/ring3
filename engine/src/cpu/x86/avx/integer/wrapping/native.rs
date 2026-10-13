use super::*;
use crate::cpu::x86::simd::{PackedKind as P, SimdState};

fn register(index: u8) -> VectorRegister {
    VectorRegister::new(index).unwrap()
}

fn pattern(width: usize, seed: usize) -> [u8; 32] {
    let modulus = 1_u128 << (width * 8);
    let sign = modulus / 2;
    let values = [
        0,
        modulus - 1,
        sign,
        sign - 1,
        1,
        modulus - 2,
        sign + 1,
        modulus / 3,
    ];
    std::array::from_fn(|byte| {
        values[(byte / width + seed) % values.len()].to_le_bytes()[byte % width]
    })
}

fn oracle(width: usize, subtract: bool, first: [u8; 32], second: [u8; 32]) -> [u8; 32] {
    let modulus = 1_u128 << (width * 8);
    let mut result = [0; 32];
    for offset in (0..32).step_by(width) {
        let number = |bytes: &[u8]| {
            bytes
                .iter()
                .rev()
                .fold(0_u128, |value, byte| value * 256 + u128::from(*byte))
        };
        let a = number(&first[offset..offset + width]);
        let b = number(&second[offset..offset + width]);
        let value = if subtract {
            (a + modulus - b) % modulus
        } else {
            (a + b) % modulus
        };
        result[offset..offset + width].copy_from_slice(&value.to_le_bytes()[..width]);
    }
    result
}

fn seed(width: usize, index: usize) -> VectorState {
    let values: [[u8; 32]; 8] = std::array::from_fn(|r| pattern(width, index + r * 3));
    VectorState {
        simd: SimdState {
            mxcsr: 0xe5af,
            xmm: values.map(|value| value[..16].try_into().unwrap()),
        },
        upper: values.map(|value| value[16..].try_into().unwrap()),
    }
}

#[test]
fn explicit_width_extends_only_the_eight_wrapping_lane_shapes() {
    let mut narrow = 0;
    let mut wide = 0;
    for kind in [
        P::Add,
        P::Subtract,
        P::Equal,
        P::GreaterSigned,
        P::AddSignedSaturate,
        P::AddUnsignedSaturate,
        P::SubtractSignedSaturate,
        P::SubtractUnsignedSaturate,
        P::MinimumSigned,
        P::MaximumSigned,
        P::MinimumUnsigned,
        P::MaximumUnsigned,
        P::AverageUnsigned,
        P::Absolute,
        P::Sign,
    ] {
        for lane in [1, 2, 4, 8] {
            let Some(operation) = IntegerLaneOperation::new(kind, lane) else {
                continue;
            };
            let first = (!operation.unary()).then_some(register(1));
            let source = MoveSource::Register(register(2));
            let xmm = IntegerInstruction::with_width(
                VectorWidth::Xmm,
                operation,
                register(0),
                first,
                source,
            )
            .unwrap();
            assert_eq!(
                xmm,
                IntegerInstruction::new(operation, register(0), first, source).unwrap()
            );
            assert_eq!(xmm.width(), VectorWidth::Xmm);
            narrow += 1;
            let ymm = IntegerInstruction::with_width(
                VectorWidth::Ymm,
                operation,
                register(0),
                first,
                source,
            );
            assert_eq!(ymm.is_some(), matches!(kind, P::Add | P::Subtract));
            wide += usize::from(ymm.is_some());
            assert!(
                IntegerInstruction::with_width(
                    VectorWidth::Ymm,
                    operation,
                    register(0),
                    operation.unary().then_some(register(1)),
                    source
                )
                .is_none()
            );
        }
    }
    assert_eq!((narrow, wide), (44, 8));
}

#[test]
fn both_source_halves_are_captured_before_any_aliased_destination_publication() {
    for kind in [P::Add, P::Subtract] {
        for lane in [1, 2, 4, 8] {
            let operation = IntegerLaneOperation::new(kind, lane).unwrap();
            for index in 0..16 {
                for destination in 0..8 {
                    let next = (destination + 1) % 8;
                    let third = (destination + 2) % 8;
                    for (first, second) in [
                        (next, third),
                        (destination, next),
                        (next, destination),
                        (next, next),
                        (destination, destination),
                    ] {
                        let mut vectors = seed(usize::from(lane), index);
                        let before = vectors;
                        let value = oracle(
                            usize::from(lane),
                            kind == P::Subtract,
                            before.read(register(first)),
                            before.read(register(second)),
                        );
                        let mut expected = before;
                        expected.simd.xmm[usize::from(destination)] =
                            value[..16].try_into().unwrap();
                        expected.upper[usize::from(destination)] = value[16..].try_into().unwrap();
                        let instruction = IntegerInstruction::with_width(
                            VectorWidth::Ymm,
                            operation,
                            register(destination),
                            Some(register(first)),
                            MoveSource::Register(register(second)),
                        )
                        .unwrap();
                        vectors.execute_integer(instruction, &[]).unwrap();
                        assert_eq!(
                            vectors, expected,
                            "{kind:?}/{lane}: {destination}/{first}/{second}, seed{index}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn wide_memory_requires_exact_full_span_and_all_refusals_preserve_vector_state() {
    for kind in [P::Add, P::Subtract] {
        for lane in [1, 2, 4, 8] {
            let operation = IntegerLaneOperation::new(kind, lane).unwrap();
            let instruction = IntegerInstruction::with_width(
                VectorWidth::Ymm,
                operation,
                register(0),
                Some(register(0)),
                MoveSource::Memory,
            )
            .unwrap();
            assert_eq!(
                instruction.memory(),
                Some(MemoryRequirement {
                    access: Access::Read,
                    bytes: 32,
                    alignment: 1
                })
            );
            let mut operand = [0xa5; 35];
            operand[1..33].copy_from_slice(&pattern(usize::from(lane), 7));
            let original = operand;
            let mut vectors = seed(usize::from(lane), 3);
            let before = vectors;
            let value = oracle(
                usize::from(lane),
                kind == P::Subtract,
                before.read(register(0)),
                operand[1..33].try_into().unwrap(),
            );
            let mut expected = before;
            expected.simd.xmm[0] = value[..16].try_into().unwrap();
            expected.upper[0] = value[16..].try_into().unwrap();
            vectors
                .execute_integer(instruction, &operand[1..33])
                .unwrap();
            assert_eq!(vectors, expected);
            for length in [0, 1, 15, 16, 17, 31, 33, 35] {
                let mut vectors = before;
                assert_eq!(
                    vectors.execute_integer(instruction, &operand[..length]),
                    Err(IntegerError::OperandLength)
                );
                assert_eq!(vectors, before);
            }
            let mut vectors = before;
            vectors.simd.mxcsr |= 0x10000;
            let invalid = vectors;
            assert_eq!(
                vectors.execute_integer(instruction, &operand[1..33]),
                Err(IntegerError::InvalidState)
            );
            assert_eq!(vectors, invalid);
            assert_eq!(operand, original);
        }
    }
}
