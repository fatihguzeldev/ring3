use super::*;
use crate::cpu::x86::simd::{PackedKind as P, SimdState};

fn register(index: u8) -> VectorRegister {
    VectorRegister::new(index).unwrap()
}

fn operations() -> Vec<IntegerLaneOperation> {
    let mut result = Vec::new();
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
        for width in [1, 2, 4, 8] {
            if let Some(operation) = IntegerLaneOperation::new(kind, width) {
                result.push(operation);
            }
        }
    }
    assert_eq!(result.len(), 44);
    result
}

fn pattern(width: usize, seed: usize) -> [u8; 16] {
    let mask = u64::MAX >> (64 - width * 8);
    let sign = 1_u64 << (width * 8 - 1);
    let values = [0, mask, sign, sign - 1, 1, mask - 1, sign + 1, mask / 3];
    std::array::from_fn(|byte| {
        values[(byte / width + seed) % values.len()].to_le_bytes()[byte % width]
    })
}

fn state(width: usize, seed: usize) -> VectorState {
    VectorState {
        simd: SimdState {
            mxcsr: 0xe5af,
            xmm: std::array::from_fn(|r| pattern(width, seed + r * 3)),
        },
        upper: std::array::from_fn(|r| std::array::from_fn(|byte| (r * 19 + byte * 37 + 1) as u8)),
    }
}

fn number(bytes: &[u8]) -> i128 {
    bytes
        .iter()
        .rev()
        .fold(0, |n, &byte| (n << 8) | i128::from(byte))
}

fn oracle(operation: IntegerLaneOperation, first: [u8; 16], second: [u8; 16]) -> [u8; 16] {
    let width = usize::from(operation.lane_bytes());
    let modulus = 1_i128 << (width * 8);
    let sign = modulus / 2;
    let signed = |n| if n >= sign { n - modulus } else { n };
    let mut result = [0; 16];
    for offset in (0..16).step_by(width) {
        let a = number(&first[offset..offset + width]);
        let b = number(&second[offset..offset + width]);
        let sa = signed(a);
        let sb = signed(b);
        let value = match operation.kind() {
            P::Add => a + b,
            P::Subtract => a - b,
            P::AddSignedSaturate => (sa + sb).clamp(-sign, sign - 1),
            P::AddUnsignedSaturate => (a + b).min(modulus - 1),
            P::SubtractSignedSaturate => (sa - sb).clamp(-sign, sign - 1),
            P::SubtractUnsignedSaturate => (a - b).max(0),
            P::MinimumSigned => sa.min(sb),
            P::MaximumSigned => sa.max(sb),
            P::MinimumUnsigned => a.min(b),
            P::MaximumUnsigned => a.max(b),
            P::AverageUnsigned => (a + b + 1) / 2,
            P::Equal => {
                if a == b {
                    modulus - 1
                } else {
                    0
                }
            }
            P::GreaterSigned => {
                if sa > sb {
                    modulus - 1
                } else {
                    0
                }
            }
            P::Absolute => sb.abs(),
            P::Sign => sa * sb.signum(),
            _ => unreachable!(),
        };
        for byte in 0..width {
            result[offset + byte] = ((value.rem_euclid(modulus) >> (byte * 8)) & 255) as u8;
        }
    }
    result
}

fn instruction(
    operation: IntegerLaneOperation,
    destination: u8,
    first: u8,
    second: MoveSource,
) -> IntegerInstruction {
    IntegerInstruction::new(
        operation,
        register(destination),
        (!operation.unary()).then_some(register(first)),
        second,
    )
    .unwrap()
}

#[test]
fn typed_shapes_admit_exactly_supported_lane_forms_and_operand_counts() {
    assert_eq!(operations().len(), 44);
    for operation in operations() {
        assert!(
            IntegerInstruction::new(
                operation,
                register(0),
                (!operation.unary()).then_some(register(1)),
                MoveSource::Register(register(2))
            )
            .is_some()
        );
        assert!(
            IntegerInstruction::new(
                operation,
                register(0),
                operation.unary().then_some(register(1)),
                MoveSource::Register(register(2))
            )
            .is_none()
        );
    }
    for kind in [P::Add, P::Subtract, P::Equal, P::GreaterSigned] {
        for width in [0, 3, 5, 16, 32, 255] {
            assert!(IntegerLaneOperation::new(kind, width).is_none());
        }
    }
    for kind in [
        P::AddSignedSaturate,
        P::AddUnsignedSaturate,
        P::SubtractSignedSaturate,
        P::SubtractUnsignedSaturate,
        P::AverageUnsigned,
    ] {
        for width in [0, 3, 4, 8, 16] {
            assert!(IntegerLaneOperation::new(kind, width).is_none());
        }
    }
    for kind in [
        P::MinimumSigned,
        P::MaximumSigned,
        P::MinimumUnsigned,
        P::MaximumUnsigned,
        P::Absolute,
        P::Sign,
    ] {
        assert!(IntegerLaneOperation::new(kind, 8).is_none());
    }
    for kind in [
        P::And,
        P::MultiplyLow,
        P::ShiftLeft,
        P::HorizontalAdd,
        P::Test,
    ] {
        for width in [1, 2, 4, 8] {
            assert!(IntegerLaneOperation::new(kind, width).is_none());
        }
    }
}

#[test]
fn literal_boundaries_cover_wrap_saturation_signed_minimum_average_and_sign() {
    let literals = [
        (P::Add, 8, u64::MAX, 1, 0),
        (P::Subtract, 8, 0, 1, u64::MAX),
        (P::AddSignedSaturate, 1, 0x7f, 1, 0x7f),
        (P::AddSignedSaturate, 2, 0x8000, 0xffff, 0x8000),
        (P::SubtractSignedSaturate, 1, 0x80, 1, 0x80),
        (P::SubtractSignedSaturate, 2, 0x7fff, 0xffff, 0x7fff),
        (P::AddUnsignedSaturate, 1, 0xff, 1, 0xff),
        (P::SubtractUnsignedSaturate, 2, 0, 1, 0),
        (P::MinimumSigned, 4, 0x80000000, 0x7fffffff, 0x80000000),
        (P::MaximumSigned, 4, 0x80000000, 0x7fffffff, 0x7fffffff),
        (P::MinimumUnsigned, 4, 0x80000000, 0x7fffffff, 0x7fffffff),
        (P::MaximumUnsigned, 4, 0x80000000, 0x7fffffff, 0x80000000),
        (P::AverageUnsigned, 1, 0xff, 0xfe, 0xff),
        (P::AverageUnsigned, 2, 0xffff, 0xffff, 0xffff),
        (
            P::Equal,
            8,
            0x8000000000000000,
            0x8000000000000000,
            u64::MAX,
        ),
        (
            P::GreaterSigned,
            8,
            0x7fffffffffffffff,
            0x8000000000000000,
            u64::MAX,
        ),
        (P::GreaterSigned, 8, 0x8000000000000000, 0, 0),
        (P::Absolute, 1, 0, 0x80, 0x80),
        (P::Absolute, 2, 0, 0x8000, 0x8000),
        (P::Absolute, 4, 0, 0x80000000, 0x80000000),
        (P::Sign, 1, 0x80, 0xff, 0x80),
        (P::Sign, 2, 0x8000, 0xffff, 0x8000),
        (P::Sign, 4, 0x80000000, 0xffffffff, 0x80000000),
        (P::Sign, 4, 0x12345678, 0, 0),
        (P::Sign, 4, 0x87654321, 1, 0x87654321),
    ];
    for (kind, width, a, b, literal) in literals {
        let operation = IntegerLaneOperation::new(kind, width).unwrap();
        let width = usize::from(width);
        let fill = |value: u64| std::array::from_fn(|byte| value.to_le_bytes()[byte % width]);
        let mut vector = state(width, 0);
        vector.simd.xmm[1] = fill(a);
        vector.simd.xmm[2] = fill(b);
        let mut expected = vector;
        expected.simd.xmm[0] = fill(literal);
        expected.upper[0] = [0; 16];
        vector
            .execute_integer(
                instruction(operation, 0, 1, MoveSource::Register(register(2))),
                &[],
            )
            .unwrap();
        assert_eq!(vector, expected, "{kind:?}/{width}: {a:x},{b:x}");
    }
}

#[test]
fn every_operation_uses_original_sources_under_aliasing_and_clears_only_destination_upper() {
    for operation in operations() {
        for seed in 0..16 {
            for (d, a, b) in [(0, 1, 2), (0, 0, 2), (0, 1, 0), (0, 1, 1), (0, 0, 0)] {
                let mut vector = state(usize::from(operation.lane_bytes()), seed);
                let mut expected = vector;
                expected.simd.xmm[usize::from(d)] = oracle(
                    operation,
                    vector.simd.xmm[usize::from(a)],
                    vector.simd.xmm[usize::from(b)],
                );
                expected.upper[usize::from(d)] = [0; 16];
                vector
                    .execute_integer(
                        instruction(operation, d, a, MoveSource::Register(register(b))),
                        &[],
                    )
                    .unwrap();
                assert_eq!(
                    vector, expected,
                    "{operation:?}, seed{seed}, aliases{d}/{a}/{b}"
                );
            }
        }
    }
}

#[test]
fn memory_sources_use_full_unaligned_16_byte_span_and_refusals_are_neutral() {
    for operation in operations() {
        let instruction = instruction(operation, 0, 0, MoveSource::Memory);
        assert_eq!(
            instruction.memory(),
            Some(MemoryRequirement {
                access: Access::Read,
                bytes: 16,
                alignment: 1
            })
        );
        let mut storage = [0xa5; 19];
        storage[1..17].copy_from_slice(&pattern(usize::from(operation.lane_bytes()), 7));
        let mut vector = state(usize::from(operation.lane_bytes()), 3);
        let before = vector;
        let original_storage = storage;
        let mut expected = before;
        expected.simd.xmm[0] = oracle(
            operation,
            before.simd.xmm[0],
            storage[1..17].try_into().unwrap(),
        );
        expected.upper[0] = [0; 16];
        vector
            .execute_integer(instruction, &storage[1..17])
            .unwrap();
        assert_eq!(vector, expected);
        assert_eq!(storage, original_storage);
        for bytes in [0, 1, 15, 17, 19] {
            let mut vector = before;
            assert_eq!(
                vector.execute_integer(instruction, &storage[..bytes]),
                Err(IntegerError::OperandLength)
            );
            assert_eq!(vector, before);
            assert_eq!(storage, original_storage);
        }
        let mut vector = before;
        vector.simd.mxcsr |= 0x10000;
        let invalid = vector;
        assert_eq!(
            vector.execute_integer(instruction, &storage[1..17]),
            Err(IntegerError::InvalidState)
        );
        assert_eq!(vector, invalid);
    }
}
