use super::super::*;
use crate::memory::Access;

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

fn repeated(bits: u64, element: Element) -> [u8; 32] {
    let mut bytes = [0; 32];
    for lane in bytes.chunks_exact_mut(element.bytes()) {
        lane.copy_from_slice(&bits.to_le_bytes()[..element.bytes()]);
    }
    bytes
}

fn floating(
    kind: FloatOperation,
    element: Element,
    width: VectorWidth,
    scalar: bool,
    destination: u8,
    source: MoveSource,
) -> Instruction {
    Instruction::Floating(FloatInstruction {
        kind,
        destination: register(destination),
        form: if scalar {
            FloatForm::Scalar {
                element,
                first: register(1),
                second: source,
            }
        } else if kind.unary() {
            FloatForm::PackedUnary {
                width,
                element,
                source,
            }
        } else {
            FloatForm::PackedBinary {
                width,
                element,
                first: register(1),
                second: source,
            }
        },
    })
}

#[test]
fn exact_memory_requirements_distinguish_scalar_and_packed_sources() {
    for element in [Element::Single, Element::Double] {
        for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
            for kind in [FloatOperation::Add, FloatOperation::SquareRoot] {
                for scalar in [false, true] {
                    let instruction = floating(kind, element, width, scalar, 0, MoveSource::Memory);
                    assert_eq!(
                        instruction.memory(),
                        Some(MemoryRequirement {
                            access: Access::Read,
                            bytes: if scalar {
                                element.bytes()
                            } else {
                                width.bytes()
                            } as u8,
                            alignment: 1,
                        })
                    );
                    assert_eq!(
                        floating(
                            kind,
                            element,
                            width,
                            scalar,
                            0,
                            MoveSource::Register(register(2))
                        )
                        .memory(),
                        None
                    );
                }
            }
            let logical = LogicalInstruction {
                kind: LogicalOperation::And,
                width,
                destination: register(0),
                first: register(1),
                second: MoveSource::Memory,
            };
            assert_eq!(
                Instruction::Logical(logical).memory(),
                Some(MemoryRequirement {
                    access: Access::Read,
                    bytes: width.bytes() as u8,
                    alignment: 1
                })
            );
        }
    }
}

#[test]
fn captured_memory_operands_keep_literal_results_aliases_and_other_state() {
    for (kind, single, double) in [
        (FloatOperation::Add, 0x40c0_0000, 0x4018_0000_0000_0000),
        (FloatOperation::Subtract, 0xc000_0000, 0xc000_0000_0000_0000),
        (FloatOperation::Multiply, 0x4100_0000, 0x4020_0000_0000_0000),
        (FloatOperation::Divide, 0x3f00_0000, 0x3fe0_0000_0000_0000),
        (FloatOperation::Minimum, 0x4000_0000, 0x4000_0000_0000_0000),
        (FloatOperation::Maximum, 0x4080_0000, 0x4010_0000_0000_0000),
        (
            FloatOperation::SquareRoot,
            0x4000_0000,
            0x4000_0000_0000_0000,
        ),
        (FloatOperation::Reciprocal, 0x3e80_0000, 0),
        (FloatOperation::ReciprocalSquareRoot, 0x3f00_0000, 0),
    ] {
        for element in [Element::Single, Element::Double] {
            if element == Element::Double && kind.approximate() {
                continue;
            }
            let (left, right, output): (u64, u64, u64) = if element == Element::Single {
                (0x4000_0000, 0x4080_0000, single)
            } else {
                (0x4000_0000_0000_0000, 0x4010_0000_0000_0000, double)
            };
            for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
                for scalar in [false, true] {
                    for destination in [0, 1] {
                        let mut current = state();
                        current.publish(register(1), repeated(left, element));
                        let before = current;
                        let operation = floating(
                            kind,
                            element,
                            width,
                            scalar,
                            destination,
                            MoveSource::Memory,
                        );
                        let source = repeated(right, element);
                        let operand = &source[..usize::from(operation.memory().unwrap().bytes)];
                        let mut expected = before;
                        let mut result = if scalar {
                            before.read(register(1))
                        } else {
                            [0; 32]
                        };
                        let count = if scalar {
                            1
                        } else {
                            width.bytes() / element.bytes()
                        };
                        for lane in
                            result[..count * element.bytes()].chunks_exact_mut(element.bytes())
                        {
                            lane.copy_from_slice(&output.to_le_bytes()[..element.bytes()]);
                        }
                        if scalar {
                            result[16..].fill(0);
                        }
                        expected.publish(register(destination), result);
                        current.execute_with_operand(operation, operand).unwrap();
                        assert_eq!(
                            current, expected,
                            "{kind:?}/{element:?}/{width:?}/{scalar}/{destination}"
                        );
                        assert_eq!(source, repeated(right, element));
                    }
                }
            }
        }
    }
}

#[test]
fn scalar_unary_ignores_first_numeric_lane_and_adjacent_source_bytes() {
    for element in [Element::Single, Element::Double] {
        let mut current = state();
        current.simd.mxcsr = 0x1f00;
        let snan = if element == Element::Single {
            0x7f80_0001
        } else {
            0x7ff0_0000_0000_0001
        };
        let four: u64 = if element == Element::Single {
            0x4080_0000
        } else {
            0x4010_0000_0000_0000
        };
        let two: u64 = if element == Element::Single {
            0x4000_0000
        } else {
            0x4000_0000_0000_0000
        };
        current.publish(register(1), repeated(snan, element));
        let before = current;
        let mut source = repeated(snan, element);
        source[..element.bytes()].copy_from_slice(&four.to_le_bytes()[..element.bytes()]);
        let mut expected = before;
        let mut result = before.read(register(1));
        result[..element.bytes()].copy_from_slice(&two.to_le_bytes()[..element.bytes()]);
        result[16..].fill(0);
        expected.publish(register(1), result);
        current
            .execute_with_operand(
                floating(
                    FloatOperation::SquareRoot,
                    element,
                    VectorWidth::Xmm,
                    true,
                    1,
                    MoveSource::Memory,
                ),
                &source[..element.bytes()],
            )
            .unwrap();
        assert_eq!(current, expected);
    }
}

#[test]
fn upper_memory_lane_faults_publish_only_sticky_mxcsr() {
    let mut current = state();
    current.simd.mxcsr = 0x1f00;
    current.publish(register(1), repeated(0x3f80_0000, Element::Single));
    let mut operand = repeated(0x3f80_0000, Element::Single);
    operand[28..].copy_from_slice(&0x7f80_0001_u32.to_le_bytes());
    let mut expected = current;
    expected.simd.mxcsr = 0x1f01;
    assert_eq!(
        current.execute_with_operand(
            floating(
                FloatOperation::Add,
                Element::Single,
                VectorWidth::Ymm,
                false,
                1,
                MoveSource::Memory
            ),
            &operand
        ),
        Err(ArithmeticError::FloatingPoint(0x1f01))
    );
    assert_eq!(current, expected);

    current.simd.mxcsr = 0x0f80;
    current.publish(register(1), repeated(0x3f80_0000, Element::Single));
    operand = repeated(0x3f80_0000, Element::Single);
    operand[28..].copy_from_slice(&0x4b80_0000_u32.to_le_bytes());
    let mut expected = current;
    expected.simd.mxcsr = 0x0fa0;
    assert_eq!(
        current.execute_with_operand(
            floating(
                FloatOperation::Add,
                Element::Single,
                VectorWidth::Ymm,
                false,
                1,
                MoveSource::Memory
            ),
            &operand
        ),
        Err(ArithmeticError::FloatingPoint(0x0fa0))
    );
    assert_eq!(current, expected);
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

#[test]
fn wrong_operand_lengths_and_invalid_host_state_are_effect_neutral() {
    for scalar in [false, true] {
        for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
            for element in [Element::Single, Element::Double] {
                let operation = floating(
                    FloatOperation::Add,
                    element,
                    width,
                    scalar,
                    0,
                    MoveSource::Memory,
                );
                let length = usize::from(operation.memory().unwrap().bytes);
                for wrong in [0, length - 1, length + 1, 33] {
                    let mut current = state();
                    let before = current;
                    assert_eq!(
                        current.execute_with_operand(operation, &vec![0; wrong]),
                        Err(ArithmeticError::OperandLength)
                    );
                    assert_eq!(current, before);
                }
                let mut current = state();
                current.simd.mxcsr = 0x1_1f80;
                let before = current;
                assert_eq!(
                    current.execute_with_operand(operation, &[0; 32][..length]),
                    Err(ArithmeticError::InvalidState)
                );
                assert_eq!(current, before);
            }
        }
    }
    let register_operation = floating(
        FloatOperation::Add,
        Element::Single,
        VectorWidth::Xmm,
        false,
        0,
        MoveSource::Register(register(2)),
    );
    let mut current = state();
    let before = current;
    assert_eq!(
        current.execute_with_operand(register_operation, &[0; 16]),
        Err(ArithmeticError::OperandLength)
    );
    assert_eq!(current, before);
}
