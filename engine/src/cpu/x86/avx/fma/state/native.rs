use super::*;

#[test]
fn all_source_alias_partitions_capture_old_destination_before_publication() {
    for (second, third, values) in [
        (0, 0, [0x40c0_0000, 0x40c0_0000, 0x40c0_0000]),
        (0, 1, [0x4100_0000, 0x40e0_0000, 0x4100_0000]),
        (1, 0, [0x40e0_0000, 0x4100_0000, 0x4100_0000]),
        (1, 1, [0x4110_0000, 0x4110_0000, 0x4130_0000]),
        (1, 2, [0x4150_0000, 0x4130_0000, 0x4188_0000]),
    ] {
        for shape in shapes(Element::Single) {
            for (order, value) in [FmaOrder::Order132, FmaOrder::Order213, FmaOrder::Order231]
                .into_iter()
                .zip(values)
            {
                let mut state = seed();
                for (index, bits) in [(0, 0x4000_0000), (1, 0x4040_0000), (2, 0x40a0_0000)] {
                    fill(&mut state, index, Element::Single, bits);
                }
                let expected = expected_result(state, shape, &[value]);
                let mut operation = instruction(FmaKind::Add, order, shape);
                operation.second = register(second);
                operation.third = MoveSource::Register(register(third));
                assert_eq!(state.execute_fma(operation, &[]), Ok(()));
                assert_eq!(state, expected);
            }
        }
    }
}

#[test]
fn memory_requirements_capture_only_exact_scalar_or_packed_third_operand() {
    for (element, two, three, five, results) in [
        (
            Element::Single,
            0x4000_0000,
            0x4040_0000,
            0x40a0_0000u64,
            [0x4150_0000, 0x4130_0000, 0x4188_0000],
        ),
        (
            Element::Double,
            0x4000_0000_0000_0000,
            0x4008_0000_0000_0000,
            0x4014_0000_0000_0000,
            [
                0x402a_0000_0000_0000,
                0x4026_0000_0000_0000,
                0x4031_0000_0000_0000,
            ],
        ),
    ] {
        for shape in shapes(element) {
            for (order, value) in [FmaOrder::Order132, FmaOrder::Order213, FmaOrder::Order231]
                .into_iter()
                .zip(results)
            {
                let mut operation = instruction(FmaKind::Add, order, shape);
                assert_eq!(operation.memory(), None);
                operation.third = MoveSource::Memory;
                assert_eq!(
                    operation.memory(),
                    Some(MemoryRequirement {
                        access: Access::Read,
                        bytes: shape.bytes() as u8,
                        alignment: 1
                    })
                );
                let mut state = seed();
                fill(&mut state, 0, element, two);
                fill(&mut state, 1, element, three);
                fill(&mut state, 2, element, 0x7ff0_0000_0000_0001);
                let before = state;
                for length in [shape.bytes() - 1, shape.bytes() + 1] {
                    assert_eq!(
                        state.execute_fma(operation, &vec![0; length]),
                        Err(ArithmeticError::OperandLength)
                    );
                    assert_eq!(state, before);
                }
                let mut operand = [0; 32];
                for lane in operand[..shape.bytes()].chunks_exact_mut(element.bytes()) {
                    lane.copy_from_slice(&five.to_le_bytes()[..element.bytes()]);
                }
                let expected = expected_result(before, shape, &[value]);
                assert_eq!(
                    state.execute_fma(operation, &operand[..shape.bytes()]),
                    Ok(())
                );
                assert_eq!(state, expected);
            }
        }
    }
}

#[test]
fn invalid_shapes_and_host_state_are_effect_neutral() {
    for element in [Element::Single, Element::Double] {
        for kind in [FmaKind::AddSubtract, FmaKind::SubtractAdd] {
            let mut state = seed();
            let before = state;
            let operation = instruction(kind, FmaOrder::Order132, FmaShape::Scalar { element });
            assert_eq!(
                state.execute_fma(operation, &[]),
                Err(ArithmeticError::InvalidOperation)
            );
            assert_eq!(state, before);
        }
    }
    let mut state = seed();
    state.simd.mxcsr = 0x1_1f80;
    let before = state;
    let operation = instruction(
        FmaKind::Add,
        FmaOrder::Order132,
        FmaShape::Scalar {
            element: Element::Single,
        },
    );
    assert_eq!(
        state.execute_fma(operation, &[]),
        Err(ArithmeticError::InvalidState)
    );
    assert_eq!(state, before);
}
