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
        let actual = state
            .evaluate_into(operation, input, 0x8d7, Some(&mut memory), &mut workspace)
            .unwrap();
        assert_eq!(actual.state, expected.state);
        assert_eq!(actual.memory_len, expected.memory_len);
        assert_eq!(actual.eflags, expected.eflags);
        assert_eq!(actual.unmasked_exception, expected.unmasked_exception);
        assert_eq!(actual.memory(), &expected.memory[..expected.memory_len]);
    }
    assert_eq!(memory, expected.memory);
    if operation.write_size() == 0 {
        let actual = state
            .evaluate_into(operation, input, 0x8d7, None, &mut workspace)
            .unwrap();
        assert_eq!(actual.state, expected.state);
        assert_eq!(actual.memory_len, 0);
        assert_eq!(actual.eflags, expected.eflags);
        assert_eq!(actual.unmasked_exception, expected.unmasked_exception);
        assert!(actual.memory().is_empty());
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
        .evaluate_into(
            X87Operation::ChangeSign,
            &[],
            0x8d7,
            Some(&mut memory),
            &mut workspace,
        )
        .unwrap();
    assert_eq!(result.memory_len, 0);
    assert!(result.memory().is_empty());
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
        assert_eq!(
            state
                .evaluate_into(
                    X87Operation::Load(X87Format::Float32),
                    input,
                    0x8d7,
                    Some(&mut memory),
                    &mut workspace,
                )
                .err(),
            Some(X87Error::InvalidInputLength)
        );
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
        assert_eq!(
            state
                .evaluate_into(operation, &[], 0x8d7, None, &mut workspace)
                .err(),
            Some(X87Error::InvalidInputLength)
        );
    }
}
