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

fn binary(
    kind: FloatOperation,
    element: Element,
    width: VectorWidth,
    destination: u8,
    first: u8,
    second: u8,
) -> Instruction {
    Instruction::Floating(FloatInstruction {
        kind,
        destination: register(destination),
        form: FloatForm::PackedBinary {
            width,
            element,
            first: register(first),
            second: register(second),
        },
    })
}

#[test]
fn elementwise_arithmetic_covers_both_widths_and_formats_with_literal_bits() {
    let cases = [
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
    ];
    for (kind, single, double) in cases {
        for element in [Element::Single, Element::Double] {
            if element == Element::Double && kind.approximate() {
                continue;
            }
            let (left, right, expected) = match element {
                Element::Single => (0x4000_0000, 0x4080_0000, single),
                Element::Double => (0x4000_0000_0000_0000, 0x4010_0000_0000_0000, double),
            };
            for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
                let mut current = state();
                fill(
                    &mut current,
                    1,
                    bytes(&vec![left; 32 / element.bytes()], element),
                );
                fill(
                    &mut current,
                    2,
                    bytes(&vec![right; 32 / element.bytes()], element),
                );
                let before = current;
                let form = if kind.unary() {
                    FloatForm::PackedUnary {
                        width,
                        element,
                        source: register(2),
                    }
                } else {
                    FloatForm::PackedBinary {
                        width,
                        element,
                        first: register(1),
                        second: register(2),
                    }
                };
                current
                    .execute(Instruction::Floating(FloatInstruction {
                        kind,
                        destination: register(0),
                        form,
                    }))
                    .unwrap();
                let expected = bytes(&vec![expected; width.bytes() / element.bytes()], element);
                assert_eq!(
                    current.read(register(0)),
                    expected,
                    "{kind:?} {element:?} {width:?}"
                );
                assert_eq!(current.simd.mxcsr, 0x1f80);
                for index in 1..8 {
                    assert_eq!(current.read(register(index)), before.read(register(index)));
                }
            }
        }
    }
}

#[test]
fn scalar_arithmetic_merges_first_source_and_zeroes_only_retired_upper_half() {
    for element in [Element::Single, Element::Double] {
        let mut current = state();
        let mut first = [0x55; 32];
        let mut second = [0x66; 32];
        let (left, right, result): (u64, u64, u64) = match element {
            Element::Single => (0x4000_0000, 0x4080_0000, 0x40c0_0000),
            Element::Double => (
                0x4000_0000_0000_0000,
                0x4010_0000_0000_0000,
                0x4018_0000_0000_0000,
            ),
        };
        first[..element.bytes()].copy_from_slice(&left.to_le_bytes()[..element.bytes()]);
        second[..element.bytes()].copy_from_slice(&right.to_le_bytes()[..element.bytes()]);
        fill(&mut current, 1, first);
        fill(&mut current, 2, second);
        let before = current;
        current
            .execute(Instruction::Floating(FloatInstruction {
                kind: FloatOperation::Add,
                destination: register(0),
                form: FloatForm::Scalar {
                    element,
                    first: register(1),
                    second: register(2),
                },
            }))
            .unwrap();
        let mut expected = first;
        expected[..element.bytes()].copy_from_slice(&result.to_le_bytes()[..element.bytes()]);
        expected[16..].fill(0);
        assert_eq!(current.read(register(0)), expected);
        for index in 1..8 {
            assert_eq!(current.read(register(index)), before.read(register(index)));
        }
    }
}

#[test]
fn all_register_alias_partitions_snapshot_sources_before_destination_publication() {
    for (destination, first, second, sum) in [
        (0, 1, 2, 0x40c0_0000),
        (0, 0, 2, 0x40a0_0000),
        (0, 1, 0, 0x4040_0000),
        (0, 1, 1, 0x4080_0000),
        (0, 0, 0, 0x4000_0000),
    ] {
        for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
            let mut current = state();
            for index in 0..8 {
                fill(
                    &mut current,
                    index,
                    bytes(&[0x3f80_0000 + ((index as u64) << 23); 8], Element::Single),
                );
            }
            let before = current;
            current
                .execute(binary(
                    FloatOperation::Add,
                    Element::Single,
                    width,
                    destination,
                    first,
                    second,
                ))
                .unwrap();
            assert_eq!(
                current.read(register(destination)),
                bytes(&vec![sum; width.bytes() / 4], Element::Single)
            );
            for index in 0..8 {
                if index != destination {
                    assert_eq!(current.read(register(index)), before.read(register(index)));
                }
            }
        }
    }
    let mut current = state();
    fill(&mut current, 0, bytes(&[0x4080_0000; 8], Element::Single));
    current
        .execute(Instruction::Floating(FloatInstruction {
            kind: FloatOperation::SquareRoot,
            destination: register(0),
            form: FloatForm::PackedUnary {
                width: VectorWidth::Ymm,
                element: Element::Single,
                source: register(0),
            },
        }))
        .unwrap();
    assert_eq!(
        current.read(register(0)),
        bytes(&[0x4000_0000; 8], Element::Single)
    );
}

#[test]
fn upper_lane_unmasked_exceptions_preserve_whole_destination_and_prior_sticky_flags() {
    let mut current = state();
    let mut first = [0x3f80_0000; 8];
    let mut second = [0x3f80_0000; 8];
    first[0] = 0x4b80_0000;
    first[4] = 0x7f80_0000;
    second[4] = 0xff80_0000;
    fill(&mut current, 1, bytes(&first, Element::Single));
    fill(&mut current, 2, bytes(&second, Element::Single));
    current.simd.mxcsr = 0x1f10;
    let mut expected = current;
    expected.simd.mxcsr = 0x1f11;
    assert_eq!(
        current.execute(binary(
            FloatOperation::Add,
            Element::Single,
            VectorWidth::Ymm,
            0,
            1,
            2
        )),
        Err(ArithmeticError::FloatingPoint(0x1f11))
    );
    assert_eq!(current, expected);

    first[0] = 1;
    fill(&mut current, 1, bytes(&first, Element::Single));
    current.simd.mxcsr = 0x1f00;
    let mut expected = current;
    expected.simd.mxcsr = 0x1f03;
    assert_eq!(
        current.execute(binary(
            FloatOperation::Add,
            Element::Single,
            VectorWidth::Ymm,
            0,
            1,
            2
        )),
        Err(ArithmeticError::FloatingPoint(0x1f03))
    );
    assert_eq!(current, expected);
}

#[test]
fn upper_lane_precision_trap_cannot_publish_lower_result_or_zero_upper_state() {
    for width in [VectorWidth::Xmm, VectorWidth::Ymm] {
        let mut current = state();
        let mut first = [0x3f80_0000; 8];
        first[width.bytes() / 4 - 1] = 0x4b80_0000;
        fill(&mut current, 1, bytes(&first, Element::Single));
        fill(&mut current, 2, bytes(&[0x3f80_0000; 8], Element::Single));
        current.simd.mxcsr = 0x0f80;
        let mut expected = current;
        expected.simd.mxcsr = 0x0fa0;
        assert_eq!(
            current.execute(binary(FloatOperation::Add, Element::Single, width, 0, 1, 2)),
            Err(ArithmeticError::FloatingPoint(0x0fa0))
        );
        assert_eq!(current, expected);
    }
}

#[test]
fn all_rounding_modes_daz_and_ftz_share_the_established_numeric_policy() {
    for element in [Element::Single, Element::Double] {
        let (large, one) = match element {
            Element::Single => (0x4b80_0000, 0x3f80_0000),
            Element::Double => (0x4340_0000_0000_0000, 0x3ff0_0000_0000_0000),
        };
        for rounding in 0..4 {
            let mut current = state();
            current.simd.mxcsr |= rounding << 13;
            fill(
                &mut current,
                1,
                bytes(&vec![large; 32 / element.bytes()], element),
            );
            fill(
                &mut current,
                2,
                bytes(&vec![one; 32 / element.bytes()], element),
            );
            current
                .execute(binary(
                    FloatOperation::Add,
                    element,
                    VectorWidth::Ymm,
                    0,
                    1,
                    2,
                ))
                .unwrap();
            let expected = large + u64::from(rounding == 2);
            assert_eq!(
                current.read(register(0)),
                bytes(&vec![expected; 32 / element.bytes()], element)
            );
            assert_eq!(current.simd.mxcsr, 0x1fa0 | rounding << 13);
        }
    }
    for (mxcsr, expected_mxcsr) in [(0x1f80, 0x1fa2), (0x1fc0, 0x1fc0)] {
        let mut current = state();
        current.simd.mxcsr = mxcsr;
        fill(&mut current, 1, bytes(&[1; 8], Element::Single));
        fill(&mut current, 2, bytes(&[0x3f80_0000; 8], Element::Single));
        current
            .execute(binary(
                FloatOperation::Add,
                Element::Single,
                VectorWidth::Ymm,
                0,
                1,
                2,
            ))
            .unwrap();
        assert_eq!(
            current.read(register(0)),
            bytes(&[0x3f80_0000; 8], Element::Single)
        );
        assert_eq!(current.simd.mxcsr, expected_mxcsr);
    }
    for (mxcsr, expected, expected_mxcsr) in [(0x1f80, 0x0040_0000, 0x1f80), (0x9f80, 0, 0x9fb0)] {
        let mut current = state();
        current.simd.mxcsr = mxcsr;
        fill(&mut current, 1, bytes(&[0x0080_0000; 8], Element::Single));
        fill(&mut current, 2, bytes(&[0x3f00_0000; 8], Element::Single));
        current
            .execute(binary(
                FloatOperation::Multiply,
                Element::Single,
                VectorWidth::Ymm,
                0,
                1,
                2,
            ))
            .unwrap();
        assert_eq!(
            current.read(register(0)),
            bytes(&[expected; 8], Element::Single)
        );
        assert_eq!(current.simd.mxcsr, expected_mxcsr);
    }
}

#[test]
fn nan_payloads_and_signed_zero_keep_sse_source_priority() {
    for (kind, first, second, expected, status) in [
        (
            FloatOperation::Add,
            0x7fc0_1234,
            0x7f80_1122,
            0x7fc0_1234,
            1,
        ),
        (
            FloatOperation::Minimum,
            0x7fc0_1234,
            0x7f80_1122,
            0x7f80_1122,
            1,
        ),
        (FloatOperation::Maximum, 0, 0x8000_0000, 0x8000_0000, 0),
        (FloatOperation::Minimum, 0x8000_0000, 0, 0, 0),
    ] {
        let mut current = state();
        fill(&mut current, 1, bytes(&[first; 8], Element::Single));
        fill(&mut current, 2, bytes(&[second; 8], Element::Single));
        current
            .execute(binary(kind, Element::Single, VectorWidth::Ymm, 0, 1, 2))
            .unwrap();
        assert_eq!(
            current.read(register(0)),
            bytes(&[expected; 8], Element::Single)
        );
        assert_eq!(current.simd.mxcsr, 0x1f80 | status);
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
                    second: register(0),
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

#[test]
fn invalid_state_registers_and_float_shapes_are_effect_neutral() {
    for index in 0..8 {
        assert_eq!(VectorRegister::new(index), Ok(VectorRegister(index)));
    }
    for index in [8, 15, 255] {
        assert_eq!(
            VectorRegister::new(index),
            Err(ArithmeticError::InvalidRegister)
        );
    }
    for instruction in [
        binary(
            FloatOperation::SquareRoot,
            Element::Single,
            VectorWidth::Xmm,
            0,
            1,
            2,
        ),
        binary(
            FloatOperation::Reciprocal,
            Element::Double,
            VectorWidth::Ymm,
            0,
            1,
            2,
        ),
        Instruction::Floating(FloatInstruction {
            kind: FloatOperation::Add,
            destination: register(0),
            form: FloatForm::PackedUnary {
                width: VectorWidth::Ymm,
                element: Element::Double,
                source: register(1),
            },
        }),
    ] {
        let mut current = state();
        let before = current;
        assert_eq!(
            current.execute(instruction),
            Err(ArithmeticError::InvalidOperation)
        );
        assert_eq!(current, before);
    }
    let mut current = state();
    current.simd.mxcsr = 0x1_0000;
    let before = current;
    assert_eq!(
        current.execute(binary(
            FloatOperation::Add,
            Element::Single,
            VectorWidth::Ymm,
            0,
            1,
            2
        )),
        Err(ArithmeticError::InvalidState)
    );
    assert_eq!(current, before);
}

#[test]
fn scalar_source_aliases_preserve_first_source_merge_bytes() {
    for (destination, first, second, sum) in [
        (0, 1, 2, 0x40c0_0000u32),
        (0, 0, 2, 0x40a0_0000),
        (0, 1, 0, 0x4040_0000),
        (0, 1, 1, 0x4080_0000),
        (0, 0, 0, 0x4000_0000),
    ] {
        let mut current = state();
        for index in 0..3 {
            let value = 0x3f80_0000u32 + ((index as u32) << 23);
            current.simd.xmm[index][..4].copy_from_slice(&value.to_le_bytes());
        }
        let before = current;
        let mut expected = before.read(register(first));
        expected[..4].copy_from_slice(&sum.to_le_bytes());
        expected[16..].fill(0);
        current
            .execute(Instruction::Floating(FloatInstruction {
                kind: FloatOperation::Add,
                destination: register(destination),
                form: FloatForm::Scalar {
                    element: Element::Single,
                    first: register(first),
                    second: register(second),
                },
            }))
            .unwrap();
        assert_eq!(current.read(register(destination)), expected);
        for index in 0..8 {
            if index != destination {
                assert_eq!(current.read(register(index)), before.read(register(index)));
            }
        }
    }
}

#[test]
fn scalar_square_root_ignores_first_source_numeric_lane_but_preserves_its_merge_bits() {
    let mut current = state();
    current.simd.mxcsr = 0x1f00;
    current.simd.xmm[1][..4].copy_from_slice(&0x7f80_0001u32.to_le_bytes());
    current.simd.xmm[2][..4].copy_from_slice(&0x4080_0000u32.to_le_bytes());
    let before = current;
    let mut expected = before.read(register(1));
    expected[..4].copy_from_slice(&0x4000_0000u32.to_le_bytes());
    expected[16..].fill(0);
    current
        .execute(Instruction::Floating(FloatInstruction {
            kind: FloatOperation::SquareRoot,
            destination: register(0),
            form: FloatForm::Scalar {
                element: Element::Single,
                first: register(1),
                second: register(2),
            },
        }))
        .unwrap();
    assert_eq!(current.read(register(0)), expected);
    assert_eq!(current.simd.mxcsr, before.simd.mxcsr);
}
