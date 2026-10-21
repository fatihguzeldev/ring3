use super::*;

const ONE: [u8; 10] = [0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f];
const TWO: [u8; 10] = [0, 0, 0, 0, 0, 0, 0, 0x80, 0, 0x40];
const THREE: [u8; 10] = [0, 0, 0, 0, 0, 0, 0, 0xc0, 0, 0x40];

fn initial() -> X87State {
    let mut state = X87State {
        control: 0x037f,
        status: 0x5f20,
        tag: 0xfc3f,
        opcode: 0x345,
        instruction_pointer: 0x1234_5678,
        data_pointer: 0x8765_4321,
        code_selector: 0x43,
        data_selector: 0x4b,
        ..X87State::default()
    };
    for (index, register) in state.registers.iter_mut().enumerate() {
        register.fill(index as u8 + 0xa0);
    }
    state.registers[3] = ONE;
    state.registers[4] = TWO;
    state
}

fn admitted(
    original: X87State,
    operation: X87Operation,
    input: &[u8],
    expected: X87State,
    bytes: &[u8],
) {
    let mut state = original;
    let mut output = [0xa5; 108];
    let effects = common::evaluate(&mut state, operation, input, Some(&mut output))
        .expect("masked common operation must use the common evaluator");
    assert_eq!(state, expected, "{operation:?}");
    assert_eq!(effects.memory_len, bytes.len(), "{operation:?}");
    assert_eq!(effects.eflags, None);
    assert!(!effects.unmasked_exception);
    assert_eq!(&output[..bytes.len()], bytes);
    assert_eq!(&output[bytes.len()..], &[0xa5; 108][bytes.len()..]);
    if bytes.is_empty() {
        let mut no_output = original;
        let effects = common::evaluate(&mut no_output, operation, input, None)
            .expect("output-free common operation must not require an output buffer");
        assert_eq!(no_output, expected);
        assert_eq!(
            (
                effects.memory_len,
                effects.eflags,
                effects.unmasked_exception
            ),
            (0, None, false)
        );
    }

    let public = original.evaluate(operation, input, 0x8d7).unwrap();
    assert_eq!(public.state, expected, "public {operation:?}");
    assert_eq!(public.memory_len, bytes.len());
    assert_eq!(&public.memory[..bytes.len()], bytes);
    assert_eq!(public.memory[bytes.len()..], [0; 108][bytes.len()..]);
    assert_eq!(public.eflags, None);
    assert!(!public.unmasked_exception);
}

#[test]
fn common_transfers_keep_raw_payloads_and_have_distinct_c1_and_pop_rules() {
    let original = initial();
    let mut loaded = original;
    loaded.registers[2] = TWO;
    loaded.tag = 0xfc0f;
    loaded.status = 0x5720;
    admitted(
        original,
        X87Operation::LoadRegister { index: 1 },
        &[],
        loaded,
        &[],
    );

    let mut stored = original;
    stored.registers[5] = ONE;
    stored.tag = 0xf03f;
    admitted(
        original,
        X87Operation::StoreRegister {
            index: 2,
            pop: false,
        },
        &[],
        stored,
        &[],
    );

    let mut popped = original;
    popped.tag = 0xfcff;
    popped.status = 0x6720;
    admitted(
        original,
        X87Operation::StoreRegister {
            index: 0,
            pop: true,
        },
        &[],
        popped,
        &[],
    );

    let mut exchanged = original;
    exchanged.registers[3] = TWO;
    exchanged.registers[4] = ONE;
    exchanged.status = 0x5d20;
    admitted(
        original,
        X87Operation::Exchange { index: 1 },
        &[],
        exchanged,
        &[],
    );
    let mut self_exchange = original;
    self_exchange.status = 0x5d20;
    admitted(
        original,
        X87Operation::Exchange { index: 0 },
        &[],
        self_exchange,
        &[],
    );

    let mut negative = original;
    negative.registers[3][9] = 0xbf;
    negative.status = 0x5d20;
    admitted(original, X87Operation::ChangeSign, &[], negative, &[]);
    let mut absolute = original;
    absolute.status = 0x5d20;
    admitted(negative, X87Operation::Absolute, &[], absolute, &[]);
}

#[test]
fn common_binary_uses_pre_pop_logical_indices_and_reads_aliases_before_writing() {
    let original = initial();
    let mut sum = original;
    sum.registers[4] = THREE;
    sum.tag = 0xfcff;
    sum.status = 0x6520;
    admitted(
        original,
        X87Operation::Binary {
            kind: X87Binary::Add,
            destination: 1,
            source: 0,
            pop: true,
        },
        &[],
        sum,
        &[],
    );

    let mut doubled = original;
    doubled.registers[3] = TWO;
    doubled.status = 0x5d20;
    admitted(
        original,
        X87Operation::Binary {
            kind: X87Binary::Add,
            destination: 0,
            source: 0,
            pop: false,
        },
        &[],
        doubled,
        &[],
    );
}

#[test]
fn common_memory_formats_preserve_the_other_seven_physical_registers_and_output_tail() {
    let formats: [(X87Format, &[u8], &[u8]); 7] = [
        (X87Format::Float32, &[0, 0, 0x40, 0x40], &[0, 0, 0x80, 0x3f]),
        (
            X87Format::Float64,
            &[0, 0, 0, 0, 0, 0, 8, 0x40],
            &[0, 0, 0, 0, 0, 0, 0xf0, 0x3f],
        ),
        (X87Format::Extended80, &THREE, &ONE),
        (X87Format::Integer16, &[3, 0], &[1, 0]),
        (X87Format::Integer32, &[3, 0, 0, 0], &[1, 0, 0, 0]),
        (
            X87Format::Integer64,
            &[3, 0, 0, 0, 0, 0, 0, 0],
            &[1, 0, 0, 0, 0, 0, 0, 0],
        ),
        (
            X87Format::PackedBcd,
            &[3, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        ),
    ];
    for (format, input, output) in formats {
        let original = initial();
        let mut loaded = original;
        loaded.registers[2] = THREE;
        loaded.tag = 0xfc0f;
        loaded.status = 0x5520;
        admitted(original, X87Operation::Load(format), input, loaded, &[]);
        let mut stored = original;
        stored.tag = 0xfcff;
        stored.status = 0x6520;
        admitted(
            original,
            X87Operation::Store {
                format,
                pop: true,
                truncate: false,
            },
            &[],
            stored,
            output,
        );
    }
}

#[test]
fn common_truncating_store_overrides_rounding_without_changing_control_or_stale_payload() {
    for (control, sign, bytes) in [(0x037f, 0x3f, [1, 0]), (0x077f, 0xbf, [0xff, 0xff])] {
        let mut original = initial();
        original.control = control;
        original.status = 0x5f04;
        original.registers[3] = [0, 0, 0, 0, 0, 0, 0, 0xc0, 0xff, sign];
        let mut expected = original;
        expected.status = 0x6524;
        expected.tag = 0xfcff;
        admitted(
            original,
            X87Operation::Store {
                format: X87Format::Integer16,
                pop: true,
                truncate: true,
            },
            &[],
            expected,
            &bytes,
        );
    }
}

#[test]
fn common_arithmetic_obeys_all_precision_controls_and_signed_directed_rounding() {
    for (control, tiny, result, status) in [
        (0x007f, [0, 0, 0, 0, 0, 0, 0, 0x80, 0xe6, 0x3f], ONE, 0x5d24),
        (
            0x027f,
            [0, 0, 0, 0, 0, 0, 0, 0x80, 0xe6, 0x3f],
            [0, 0, 0, 0, 0x40, 0, 0, 0x80, 0xff, 0x3f],
            0x5d04,
        ),
        (
            0x037f,
            [0, 0, 0, 0, 0, 0, 0, 0x80, 0xe6, 0x3f],
            [0, 0, 0, 0, 0x40, 0, 0, 0x80, 0xff, 0x3f],
            0x5d04,
        ),
        (0x007f, [0, 0, 0, 0, 0, 0, 0, 0x80, 0xc3, 0x3f], ONE, 0x5d24),
        (0x027f, [0, 0, 0, 0, 0, 0, 0, 0x80, 0xc3, 0x3f], ONE, 0x5d24),
        (
            0x037f,
            [0, 0, 0, 0, 0, 0, 0, 0x80, 0xc3, 0x3f],
            [8, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f],
            0x5d04,
        ),
    ] {
        let mut original = initial();
        original.control = control;
        original.status = 0x5f04;
        original.registers[4] = tiny;
        let mut expected = original;
        expected.registers[3] = result;
        expected.status = status;
        admitted(
            original,
            X87Operation::Binary {
                kind: X87Binary::Add,
                destination: 0,
                source: 1,
                pop: false,
            },
            &[],
            expected,
            &[],
        );
    }
    for negative in [false, true] {
        for mode in 0..4 {
            let mut original = initial();
            original.control = 0x007f | (mode << 10);
            original.status = 0x5f04;
            original.registers[3] = ONE;
            original.registers[4] = [0, 0, 0, 0, 0, 0, 0, 0x80, 0xe7, 0x3f];
            if negative {
                original.registers[3][9] = 0xbf;
                original.registers[4][9] = 0xbf;
            }
            let increment = (!negative && mode == 2) || (negative && mode == 1);
            let mut expected = original;
            expected.registers[3] = [
                0,
                0,
                0,
                0,
                0,
                if increment { 1 } else { 0 },
                0,
                0x80,
                0xff,
                if negative { 0xbf } else { 0x3f },
            ];
            expected.status = if increment { 0x5f24 } else { 0x5d24 };
            admitted(
                original,
                X87Operation::Binary {
                    kind: X87Binary::Add,
                    destination: 0,
                    source: 1,
                    pop: false,
                },
                &[],
                expected,
                &[],
            );
        }
    }
}

#[test]
fn common_special_operands_keep_nan_payloads_and_invalid_priority_over_new_denormal() {
    for (payload, result, status) in [
        (
            [0x55, 0, 0, 0, 0, 0, 0, 0xc0, 0xff, 0x7f],
            [0x55, 0, 0, 0, 0, 0, 0, 0xc0, 0xff, 0x7f],
            0x5d04,
        ),
        (
            [0x55, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x7f],
            [0x55, 0, 0, 0, 0, 0, 0, 0xc0, 0xff, 0x7f],
            0x5d05,
        ),
        (
            [0, 0, 0, 0, 0, 0, 0, 0x40, 0xff, 0x3f],
            [0, 0, 0, 0, 0, 0, 0, 0xc0, 0xff, 0xff],
            0x5d05,
        ),
    ] {
        let mut original = initial();
        original.status = 0x5f04;
        original.tag = 0xfcbf;
        original.registers[3] = payload;
        let mut expected = original;
        expected.registers[3] = result;
        expected.status = status;
        admitted(
            original,
            X87Operation::BinaryMemory {
                kind: X87Binary::Add,
                format: X87Format::Float32,
            },
            &[1, 0, 0, 0],
            expected,
            &[],
        );
    }
}

#[test]
fn common_declines_unmasked_empty_invalid_and_noncommon_operations_without_effects() {
    let reject = |original: X87State, operation, input: &[u8], output: bool| {
        let mut state = original;
        let mut memory = [0xa5; 108];
        assert!(
            common::evaluate(&mut state, operation, input, output.then_some(&mut memory)).is_none()
        );
        assert_eq!(state, original);
        assert_eq!(memory, [0xa5; 108]);
    };
    for bit in 0..6 {
        let mut original = initial();
        original.control &= !(1 << bit);
        reject(original, X87Operation::ChangeSign, &[], true);
    }
    let mut empty = initial();
    empty.tag = 0xfcff;
    reject(empty, X87Operation::ChangeSign, &[], true);
    reject(
        initial(),
        X87Operation::LoadRegister { index: 8 },
        &[],
        true,
    );
    reject(
        initial(),
        X87Operation::Load(X87Format::Float32),
        &[0; 3],
        true,
    );
    reject(
        initial(),
        X87Operation::Store {
            format: X87Format::Float32,
            pop: true,
            truncate: false,
        },
        &[],
        false,
    );
    reject(
        initial(),
        X87Operation::Compare {
            index: 1,
            quiet: false,
            pop: 0,
            integer_flags: true,
        },
        &[],
        true,
    );
    reject(
        initial(),
        X87Operation::Trigonometric { function: 0 },
        &[],
        true,
    );
}
