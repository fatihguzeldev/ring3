use super::*;

fn one() -> X87State {
    let mut state = X87State {
        tag: 0xfffc,
        ..X87State::default()
    };
    state.registers[0] = Ext80::ONE.0;
    state
}

fn compare(state: X87State, operation: X87Operation, input: &[u8]) -> X87Outcome {
    let expected = state.evaluate(operation, input, 0x8d7).unwrap();
    let mut memory = [0; 108];
    let mut workspace = Default::default();
    {
        let mut actual_state = state;
        let actual = actual_state
            .evaluate_in_place(operation, input, 0x8d7, Some(&mut memory), &mut workspace)
            .unwrap();
        assert_eq!(actual_state, expected.state);
        assert_eq!(actual.memory_len, expected.memory_len);
        assert_eq!(actual.eflags, expected.eflags);
        assert_eq!(actual.unmasked_exception, expected.unmasked_exception);
        assert_eq!(
            &memory[..actual.memory_len],
            &expected.memory[..expected.memory_len]
        );
    }
    assert_eq!(memory, expected.memory);
    if operation.write_size() == 0 {
        let mut actual_state = state;
        let actual = actual_state
            .evaluate_in_place(operation, input, 0x8d7, None, &mut workspace)
            .unwrap();
        assert_eq!(actual_state, expected.state);
        assert_eq!(actual.memory_len, 0);
        assert_eq!(actual.eflags, expected.eflags);
        assert_eq!(actual.unmasked_exception, expected.unmasked_exception);
        assert_eq!(expected.memory, [0; 108]);
    }
    expected
}

#[test]
fn register_load_flags_and_exception_paths_do_not_require_output_memory() {
    let changed = compare(one(), X87Operation::ChangeSign, &[]);
    assert_eq!(changed.state.stack(0), Some(Ext80::ONE.negated()));
    let loaded = compare(
        one(),
        X87Operation::Load(X87Format::Float32),
        &1.25_f32.to_le_bytes(),
    );
    assert_eq!(
        loaded.state.stack(0),
        Some(Ext80::from_parts(false, 0x3fff, 0xa000_0000_0000_0000))
    );
    let compared = compare(
        one(),
        X87Operation::Compare {
            index: 0,
            quiet: true,
            pop: 0,
            integer_flags: true,
        },
        &[],
    );
    assert_eq!(compared.eflags, Some(0x42));
    let empty = compare(X87State::default(), X87Operation::ChangeSign, &[]);
    assert_eq!(empty.state.status & 0x41, 0x41);
    assert!(!empty.unmasked_exception);
    let mut pending = one();
    pending.control &= !INVALID;
    pending.status = INVALID;
    let blocked = compare(pending, X87Operation::LoadConstant { index: 0 }, &[]);
    assert_eq!(blocked.state.status, 0x8081);
    assert_eq!(blocked.state.registers, pending.registers);
    assert!(blocked.unmasked_exception);
}

#[test]
fn register_execution_preserves_a_supplied_unused_output_buffer() {
    let mut memory = [0xa5; 108];
    let mut workspace = Default::default();
    let result = one()
        .evaluate_in_place(
            X87Operation::ChangeSign,
            &[],
            0x8d7,
            Some(&mut memory),
            &mut workspace,
        )
        .unwrap();
    assert_eq!(result.memory_len, 0);
    assert_eq!(memory, [0xa5; 108]);
}

#[test]
fn stores_preserve_the_public_zero_tail_and_attempted_bytes_when_suppressed() {
    for format in [
        X87Format::Float32,
        X87Format::Float64,
        X87Format::Extended80,
        X87Format::Integer16,
        X87Format::Integer32,
        X87Format::Integer64,
        X87Format::PackedBcd,
    ] {
        let result = compare(
            one(),
            X87Operation::Store {
                format,
                pop: true,
                truncate: false,
            },
            &[],
        );
        assert_eq!(result.memory_len, format.size());
        assert_eq!(result.memory[format.size()..], [0; 108][format.size()..]);
        assert_eq!(result.state.tag, 0xffff);
    }
    for (value, mask, attempted, status) in [
        (
            Ext80::from_parts(false, 16383 + 128, 1 << 63),
            8,
            [0, 0, 0x80, 0x7f],
            0x8088,
        ),
        (
            Ext80::from_parts(false, 16383 - 149, 1 << 63),
            16,
            [1, 0, 0, 0],
            0x8090,
        ),
    ] {
        let mut state = one();
        state.registers[0] = value.0;
        state.control &= !mask;
        let result = compare(
            state,
            X87Operation::Store {
                format: X87Format::Float32,
                pop: true,
                truncate: false,
            },
            &[],
        );
        let mut expected = [0; 108];
        expected[..4].copy_from_slice(&attempted);
        assert_eq!(result.memory, expected);
        assert_eq!(result.memory_len, 0);
        assert_eq!(result.state.registers, state.registers);
        assert_eq!(result.state.tag, state.tag);
        assert_eq!(result.state.status, status);
        assert!(result.unmasked_exception);
    }
}

#[test]
fn environment_and_full_save_output_preserve_width_order_and_restore() {
    let mut state = one();
    state.status = 0x1800;
    state.control = 0x2340;
    state.opcode = 0x345;
    state.instruction_pointer = 0x1234_5678;
    state.data_pointer = 0x8765_4321;
    state.code_selector = 0x1b;
    state.data_selector = 0x23;
    for operand16 in [true, false] {
        let environment_size = if operand16 { 14 } else { 28 };
        let environment = compare(state, X87Operation::StoreEnvironment { operand16 }, &[]);
        assert_eq!(environment.memory_len, environment_size);
        assert_eq!(&environment.memory[..2], &state.control.to_le_bytes());
        assert_eq!(
            environment.memory[environment_size..],
            [0; 108][environment_size..]
        );
        let loaded = compare(
            X87State::default(),
            X87Operation::LoadEnvironment { operand16 },
            &environment.memory[..environment_size],
        );
        assert_eq!(loaded.state.control, state.control);
        assert_eq!(loaded.state.status, state.status);
        let saved = compare(state, X87Operation::Save { operand16 }, &[]);
        assert_eq!(saved.memory_len, environment_size + 80);
        assert_eq!(saved.state.control, 0x37f);
        assert_eq!(saved.state.tag, 0xffff);
        assert_eq!(saved.state.registers, state.registers);
        let restored = compare(
            X87State::default(),
            X87Operation::Restore { operand16 },
            &saved.memory[..saved.memory_len],
        );
        let mut expected = state;
        if operand16 {
            expected.instruction_pointer &= 0xffff;
            expected.data_pointer &= 0xffff;
            expected.opcode = 0;
        }
        assert_eq!(restored.state, expected);
    }
    for operation in [X87Operation::StoreControl, X87Operation::StoreStatus] {
        let result = compare(state, operation, &[]);
        assert_eq!(result.memory_len, 2);
        let expected = if operation == X87Operation::StoreControl {
            state.control
        } else {
            state.status
        };
        assert_eq!(&result.memory[..2], &expected.to_le_bytes());
        assert_eq!(result.memory[2..], [0; 108][2..]);
    }
}

#[test]
fn input_and_output_preconditions_fail_before_output_mutation() {
    let state = one();
    let mut workspace = Default::default();
    for input in [&[][..], &[0; 3][..], &[0; 5][..]] {
        let mut memory = [0xa5; 108];
        let mut actual_state = state;
        assert_eq!(
            actual_state
                .evaluate_in_place(
                    X87Operation::Load(X87Format::Float32),
                    input,
                    0x8d7,
                    Some(&mut memory),
                    &mut workspace,
                )
                .err(),
            Some(X87Error::InvalidInputLength)
        );
        assert_eq!(actual_state, state);
        assert_eq!(memory, [0xa5; 108]);
        assert_eq!(
            state.evaluate(X87Operation::Load(X87Format::Float32), input, 0x8d7),
            Err(X87Error::InvalidInputLength)
        );
    }
    for operation in [
        X87Operation::Store {
            format: X87Format::Float32,
            pop: true,
            truncate: false,
        },
        X87Operation::StoreControl,
        X87Operation::StoreStatus,
        X87Operation::StoreEnvironment { operand16: true },
        X87Operation::StoreEnvironment { operand16: false },
        X87Operation::Save { operand16: true },
        X87Operation::Save { operand16: false },
    ] {
        let mut actual_state = state;
        assert_eq!(
            actual_state
                .evaluate_in_place(operation, &[], 0x8d7, None, &mut workspace)
                .err(),
            Some(X87Error::InvalidInputLength)
        );
        assert_eq!(actual_state, state);
    }
}

#[test]
fn scalar_output_capacity_is_checked_before_pop_and_accepts_exact_or_larger_slices() {
    let mut original = one();
    original.status = 0x4704;
    original.opcode = 0x345;
    original.instruction_pointer = 0x1234_5678;
    original.data_pointer = 0x8765_4321;
    original.code_selector = 0x43;
    original.data_selector = 0x4b;
    let operation = X87Operation::Store {
        format: X87Format::Float32,
        pop: true,
        truncate: false,
    };
    let mut expected_state = original;
    expected_state.status = 0x4d04;
    expected_state.tag = 0xffff;
    for capacity in [3, 4, 10] {
        for direct in [false, true] {
            let mut state = original;
            let mut output = [0xa5; 18];
            let effects = if direct {
                common::evaluate(
                    &mut state,
                    operation,
                    &[],
                    Some(&mut output[4..4 + capacity]),
                )
            } else {
                let result = state.evaluate_in_place(
                    operation,
                    &[],
                    0x8d7,
                    Some(&mut output[4..4 + capacity]),
                    &mut Default::default(),
                );
                if capacity == 3 {
                    assert_eq!(result.err(), Some(X87Error::InvalidInputLength));
                    None
                } else {
                    Some(result.unwrap())
                }
            };
            if capacity == 3 {
                assert!(effects.is_none());
                assert_eq!(state, original);
                assert_eq!(output, [0xa5; 18]);
            } else {
                let effects = effects.unwrap();
                assert_eq!(state, expected_state);
                assert_eq!(effects.memory_len, 4);
                assert_eq!(effects.eflags, None);
                assert!(!effects.unmasked_exception);
                let mut expected_output = [0xa5; 18];
                expected_output[4..8].copy_from_slice(&[0, 0, 0x80, 0x3f]);
                assert_eq!(output, expected_output);
            }
        }
    }
}

#[test]
fn environment_and_save_outputs_require_their_exact_image_capacity_before_mutation() {
    let mut original = X87State {
        control: 0x2340,
        status: 0x1800,
        tag: 0xff3f,
        opcode: 0x345,
        instruction_pointer: 0x1234_5678,
        data_pointer: 0x8765_4321,
        code_selector: 0x43,
        data_selector: 0x4b,
        registers: [[0; 10]; 8],
    };
    for (index, register) in original.registers.iter_mut().enumerate() {
        register.fill(index as u8 + 1);
    }
    for operand16 in [true, false] {
        let environment: &[u8] = if operand16 {
            &[
                0x40, 0x23, 0, 0x18, 0x3f, 0xff, 0x78, 0x56, 0x43, 0, 0x21, 0x43, 0x4b, 0,
            ]
        } else {
            &[
                0x40, 0x23, 0xff, 0xff, 0, 0x18, 0xff, 0xff, 0x3f, 0xff, 0xff, 0xff, 0x78, 0x56,
                0x34, 0x12, 0x43, 0, 0x45, 3, 0x21, 0x43, 0x65, 0x87, 0x4b, 0, 0, 0,
            ]
        };
        for save in [false, true] {
            let operation = if save {
                X87Operation::Save { operand16 }
            } else {
                X87Operation::StoreEnvironment { operand16 }
            };
            let mut image = environment.to_vec();
            if save {
                for value in [4, 5, 6, 7, 8, 1, 2, 3] {
                    image.extend_from_slice(&[value; 10]);
                }
            }
            let expected_state = if save {
                X87State {
                    control: 0x037f,
                    status: 0,
                    tag: 0xffff,
                    opcode: 0,
                    instruction_pointer: 0,
                    data_pointer: 0,
                    code_selector: 0,
                    data_selector: 0,
                    registers: original.registers,
                }
            } else {
                let mut state = original;
                state.control = 0x237f;
                state
            };
            for capacity in [image.len() - 1, image.len()] {
                let mut state = original;
                let mut output = [0xa5; 116];
                let result = state.evaluate_in_place(
                    operation,
                    &[],
                    0x8d7,
                    Some(&mut output[4..4 + capacity]),
                    &mut Default::default(),
                );
                if capacity < image.len() {
                    assert_eq!(result.err(), Some(X87Error::InvalidInputLength));
                    assert_eq!(state, original);
                    assert_eq!(output, [0xa5; 116]);
                } else {
                    let effects = result.unwrap();
                    assert_eq!(state, expected_state);
                    assert_eq!(effects.memory_len, image.len());
                    assert_eq!(effects.eflags, None);
                    assert!(!effects.unmasked_exception);
                    let mut expected_output = [0xa5; 116];
                    expected_output[4..4 + image.len()].copy_from_slice(&image);
                    assert_eq!(output, expected_output);
                }
            }
            let public = original.evaluate(operation, &[], 0x8d7).unwrap();
            assert_eq!(public.state, expected_state);
            assert_eq!(public.memory_len, image.len());
            assert_eq!(&public.memory[..image.len()], image);
            assert_eq!(public.memory[image.len()..], [0; 108][image.len()..]);
        }
    }
}
