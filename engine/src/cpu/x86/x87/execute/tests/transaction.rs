use super::*;

fn one() -> X87State {
    let mut state = X87State {
        tag: 0xfffc,
        opcode: 0x345,
        instruction_pointer: 0x1234_5678,
        data_pointer: 0x8765_4321,
        code_selector: 0x1b,
        data_selector: 0x23,
        ..X87State::default()
    };
    state.registers[0] = Ext80::ONE.0;
    state
}

#[test]
fn in_place_register_success_keeps_unused_output_and_public_input_unchanged() {
    let original = one();
    let public = original
        .evaluate(X87Operation::ChangeSign, &[], 0x8d7)
        .unwrap();
    let mut state = original;
    let mut output = [0xa5; 108];
    let effects = state
        .evaluate_in_place(
            X87Operation::ChangeSign,
            &[],
            0x8d7,
            Some(&mut output),
            &mut Default::default(),
        )
        .unwrap();
    let mut expected = original;
    expected.registers[0] = Ext80::ONE.negated().0;
    assert_eq!(state, expected);
    assert_eq!(public.state, expected);
    assert_eq!(original, one());
    assert_eq!(effects.memory_len, 0);
    assert_eq!(effects.eflags, None);
    assert!(!effects.unmasked_exception);
    assert_eq!(output, [0xa5; 108]);
}

#[test]
fn in_place_evaluator_error_restores_status_cleared_before_rejection() {
    for operation in [
        X87Operation::Trigonometric { function: 4 },
        X87Operation::Binary {
            kind: X87Binary::Add,
            destination: 0,
            source: 8,
            pop: true,
        },
    ] {
        let mut original = one();
        original.status = C2 | 0x20;
        let mut state = original;
        let mut output = [0xa5; 108];
        assert_eq!(
            state
                .evaluate_in_place(
                    operation,
                    &[],
                    0x8d7,
                    Some(&mut output),
                    &mut Default::default(),
                )
                .err(),
            Some(X87Error::InvalidOperand)
        );
        assert_eq!(state, original);
        assert_eq!(output, [0xa5; 108]);
    }
}

#[test]
fn in_place_suppressed_store_keeps_attempted_bytes_without_pop() {
    let mut original = one();
    original.registers[0] = Ext80::from_parts(false, 16383 + 128, 1 << 63).0;
    original.control &= !8;
    let mut state = original;
    let mut output = [0xa5; 108];
    let effects = state
        .evaluate_in_place(
            X87Operation::Store {
                format: X87Format::Float32,
                pop: true,
                truncate: false,
            },
            &[],
            0x8d7,
            Some(&mut output),
            &mut Default::default(),
        )
        .unwrap();
    let mut expected = original;
    expected.status = 0x8088;
    assert_eq!(state, expected);
    assert_eq!(effects.memory_len, 0);
    assert_eq!(effects.eflags, None);
    assert!(effects.unmasked_exception);
    assert_eq!(&output[..4], &[0, 0, 0x80, 0x7f]);
    assert_eq!(&output[4..], &[0xa5; 104]);
}

#[test]
fn in_place_pending_exception_keeps_stack_and_suppresses_output() {
    let mut original = one();
    original.control &= !INVALID;
    original.status = INVALID;
    let mut state = original;
    let mut output = [0xa5; 108];
    let effects = state
        .evaluate_in_place(
            X87Operation::LoadConstant { index: 0 },
            &[],
            0x8d7,
            Some(&mut output),
            &mut Default::default(),
        )
        .unwrap();
    let mut expected = original;
    expected.status = 0x8081;
    assert_eq!(state, expected);
    assert_eq!(effects.memory_len, 0);
    assert_eq!(effects.eflags, None);
    assert!(effects.unmasked_exception);
    assert_eq!(output, [0xa5; 108]);
}

#[test]
fn in_place_invalid_input_and_missing_output_preserve_state() {
    let original = one();
    for input in [&[][..], &[0; 3][..], &[0; 5][..]] {
        let mut state = original;
        let mut output = [0xa5; 108];
        assert_eq!(
            state
                .evaluate_in_place(
                    X87Operation::Load(X87Format::Float32),
                    input,
                    0x8d7,
                    Some(&mut output),
                    &mut Default::default(),
                )
                .err(),
            Some(X87Error::InvalidInputLength)
        );
        assert_eq!(state, original);
        assert_eq!(output, [0xa5; 108]);
    }
    let mut state = original;
    assert_eq!(
        state
            .evaluate_in_place(
                X87Operation::StoreControl,
                &[],
                0x8d7,
                None,
                &mut Default::default(),
            )
            .err(),
        Some(X87Error::InvalidInputLength)
    );
    assert_eq!(state, original);
}
