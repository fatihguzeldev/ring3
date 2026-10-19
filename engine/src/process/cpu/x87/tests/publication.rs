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
        engine.x87_op(
            X87Operation::ChangeSign.encode(),
            0x7000,
            0x46,
            0x1000,
            0x1e0,
            1
        ),
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
fn memory_load_and_store_only_operations_preserve_exact_records_and_unwritten_bytes() {
    let mut engine = engine();
    let mut original = [0xa5; 32];
    original[..4].copy_from_slice(&1.25_f32.to_le_bytes());
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x5000), &original)
        .unwrap();
    let before = engine.arena().to_vec();
    let version = engine.memory().unwrap().transaction_version();
    assert_eq!(
        engine.x87_op(
            X87Operation::Load(X87Format::Float32).encode(),
            0x5000,
            0x46,
            0x1000,
            0x100,
            1
        ),
        Ok(StoreCompletion::Complete)
    );
    let mut loaded = X87State {
        status: 0x3800,
        tag: 0x3fff,
        opcode: 0x100,
        instruction_pointer: 0x1000,
        data_pointer: 0x5000,
        code_selector: 0x1b,
        data_selector: 0x23,
        ..X87State::default()
    };
    loaded.registers[7] = [0, 0, 0, 0, 0, 0, 0, 0xa0, 0xff, 0x3f];
    let mut expected = expected_packet(&before, 0, 0, 4);
    expected[HELPER_OFFSET + 20..HELPER_OFFSET + 24].copy_from_slice(&0x46_u32.to_le_bytes());
    encode_x87(&loaded, &mut expected[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap();
    assert_eq!(engine.arena(), expected);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);

    let before = engine.arena().to_vec();
    assert_eq!(
        engine.x87_op(
            X87Operation::Store {
                format: X87Format::Float32,
                pop: true,
                truncate: false,
            }
            .encode(),
            0x5010,
            0x46,
            0x1000,
            0x118,
            1
        ),
        Ok(StoreCompletion::Complete)
    );
    loaded.status = 0;
    loaded.tag = 0xffff;
    loaded.opcode = 0x118;
    loaded.data_pointer = 0x5010;
    let mut expected = expected_packet(&before, 0, 0, 4);
    expected[HELPER_OFFSET + 20..HELPER_OFFSET + 24].copy_from_slice(&0x46_u32.to_le_bytes());
    encode_x87(&loaded, &mut expected[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap();
    assert_eq!(engine.arena(), expected);
    let mut actual = [0; 32];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x5000), &mut actual)
        .unwrap();
    original[16..20].copy_from_slice(&1.25_f32.to_le_bytes());
    assert_eq!(actual, original);
}

#[test]
fn malformed_fp_records_remain_neutral_before_operand_read_or_publication() {
    let mut engine = engine();
    let original = engine.arena().to_vec();
    for (offset, value) in [
        (0, b'X'),
        (4, 2),
        (6, 2),
        (8, 127),
        (12, 1),
        (23, 8),
        (36, 1),
        (120, 1),
    ] {
        engine.arena_mut().unwrap().copy_from_slice(&original);
        engine.arena_mut().unwrap()[X87_OFFSET + offset] = value;
        let before = engine.arena().to_vec();
        let version = engine.memory().unwrap().transaction_version();
        assert_eq!(
            engine.x87_op(
                X87Operation::Load(X87Format::Float32).encode(),
                0x7000,
                0x46,
                0x1000,
                0x100,
                1
            ),
            Err(HostError::Infrastructure)
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.memory().unwrap().transaction_version(), version);
        assert_eq!(engine.cr2(), None);
    }
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

#[test]
fn unmasked_store_suppression_never_publishes_attempted_output_or_pop() {
    let mut engine = engine();
    let mut original = one();
    original.control &= !8;
    original.registers[0] = crate::cpu::x86::Ext80::from_parts(false, 16383 + 128, 1 << 63).0;
    encode_x87(
        &original,
        &mut engine.arena_mut().unwrap()[X87_OFFSET..X87_OFFSET + X87_SIZE],
    )
    .unwrap();
    let before = engine.arena().to_vec();
    let version = engine.memory().unwrap().transaction_version();
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
                truncate: false,
            }
            .encode(),
            0x5000,
            0x46,
            0x1000,
            0x118,
            1,
        ),
        Ok(StoreCompletion::Complete)
    );
    original.status = 0x8088;
    original.instruction_pointer = 0x1000;
    original.data_pointer = 0x5000;
    original.opcode = 0x118;
    original.code_selector = 0x1b;
    original.data_selector = 0x23;
    let mut expected = expected_packet(&before, 0, 0, 4);
    expected[HELPER_OFFSET + 20..HELPER_OFFSET + 24].copy_from_slice(&0x46_u32.to_le_bytes());
    encode_x87(&original, &mut expected[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap();
    assert_eq!(engine.arena(), expected);
    let mut memory_after = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x5000), &mut memory_after)
        .unwrap();
    assert_eq!(memory_after, memory_before);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
}
