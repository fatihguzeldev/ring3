use super::*;

fn one() -> X87State {
    let mut state = X87State {
        tag: 0xfffc,
        ..X87State::default()
    };
    state.registers[0] = [0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f];
    state
}

#[test]
fn register_outcome_publishes_exact_fp_and_helper_records_without_memory_changes() {
    let mut engine = engine();
    let original = one();
    encode_x87(
        &original,
        &mut engine.arena_mut().unwrap()[X87_OFFSET..X87_OFFSET + X87_SIZE],
    )
    .unwrap();
    let before = engine.arena().to_vec();
    let version = engine.memory().unwrap().transaction_version();
    assert_eq!(
        engine.x87_op(X87Operation::ChangeSign.encode(), 0, 0x46, 0x1000, 0x1e0, 1),
        Ok(StoreCompletion::Complete)
    );
    let mut expected_state = original;
    expected_state.registers[0][9] = 0xbf;
    expected_state.instruction_pointer = 0x1000;
    expected_state.opcode = 0x1e0;
    expected_state.code_selector = 0x1b;
    let mut expected = expected_packet(&before, 0, 0, 0);
    expected[HELPER_OFFSET + 20..HELPER_OFFSET + 24].copy_from_slice(&0x46_u32.to_le_bytes());
    encode_x87(
        &expected_state,
        &mut expected[X87_OFFSET..X87_OFFSET + X87_SIZE],
    )
    .unwrap();
    assert_eq!(engine.arena(), expected);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
}

#[test]
fn failed_store_commit_keeps_preinstruction_fp_state_and_guest_bytes() {
    let mut engine = engine();
    let original = one();
    encode_x87(
        &original,
        &mut engine.arena_mut().unwrap()[X87_OFFSET..X87_OFFSET + X87_SIZE],
    )
    .unwrap();
    engine.memory.as_mut().unwrap().exhaust_versions_for_test();
    let before = engine.arena().to_vec();
    let mut memory_before = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x5000), &mut memory_before)
        .unwrap();
    assert_eq!(
        engine.x87_op(
            X87Operation::Store {
                format: X87Format::Float32,
                pop: true,
                truncate: false
            }
            .encode(),
            0x5000,
            0x46,
            0x1000,
            0x11b,
            1,
        ),
        Ok(StoreCompletion::Complete)
    );
    assert_eq!(engine.arena(), expected_packet(&before, 2, 1, 4));
    let mut actual = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x5000), &mut actual)
        .unwrap();
    assert_eq!(actual, memory_before);
    assert_eq!(engine.memory().unwrap().transaction_version(), u64::MAX);
}
