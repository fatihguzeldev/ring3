use super::*;
use crate::{cpu::dbt::RegistryError, cpu::x86::Ext80};

#[test]
fn final_store_publishes_literal_fp32_and_pop_after_register_or_read_prefix() {
    for code in [REGISTER_STORE, READ_STORE] {
        let (mut paired, id, _) = fixture(code);
        let (mut ordinary, ordinary_id, descriptors) = fixture(code);
        for engine in [&mut paired, &mut ordinary] {
            fp(engine, one());
        }
        let mut expected_ram = ram(&paired);
        let version = paired.memory().unwrap().transaction_version();
        scalar(&mut ordinary, ordinary_id, descriptors[0], 0x5000);
        scalar(&mut ordinary, ordinary_id, descriptors[1], 0x5010);
        assert_eq!(
            paired.x87_resident_pair(KEY, id, 0x1000, [0x5000, 0x5010], FLAGS, 2),
            X87PairOutcome::complete(2, 1)
        );
        assert_same(&paired, &ordinary);
        let expected_fp = if code == REGISTER_STORE {
            expected_ram[16..20].copy_from_slice(&(-1.0_f32).to_le_bytes());
            stored_negative(0x5010)
        } else {
            expected_ram[16..20].copy_from_slice(&1.25_f32.to_le_bytes());
            let mut state = X87State {
                tag: 0xfffc,
                opcode: 0x119,
                instruction_pointer: 0x1002,
                data_pointer: 0x5010,
                code_selector: 0x1b,
                data_selector: 0x23,
                ..X87State::default()
            };
            state.registers[0] = one().registers[0];
            state.registers[7] = [0, 0, 0, 0, 0, 0, 0, 0xa0, 0xff, 0x3f];
            state
        };
        assert_eq!(actual_fp(&paired), expected_fp);
        assert_eq!(ram(&paired), expected_ram);
        assert_eq!(paired.memory().unwrap().transaction_version(), version + 1);
        assert_packet(&paired, 7, [0, FLAGS, 0, 0, 0, 4]);
    }
}

#[test]
fn unmasked_final_store_suppression_keeps_ram_and_does_not_pop() {
    let (mut paired, id, _) = fixture(REGISTER_STORE);
    let (mut ordinary, ordinary_id, descriptors) = fixture(REGISTER_STORE);
    let mut original = one();
    original.control &= !8;
    original.registers[0] = Ext80::from_parts(false, 16383 + 128, 1 << 63).0;
    for engine in [&mut paired, &mut ordinary] {
        fp(engine, original);
    }
    let before_ram = ram(&paired);
    let version = paired.memory().unwrap().transaction_version();
    scalar(&mut ordinary, ordinary_id, descriptors[0], 0);
    scalar(&mut ordinary, ordinary_id, descriptors[1], 0x5010);
    assert_eq!(
        paired.x87_resident_pair(KEY, id, 0x1000, [0, 0x5010], FLAGS, 2),
        X87PairOutcome::complete(2, 1)
    );
    assert_same(&paired, &ordinary);
    let mut expected = original;
    expected.registers[0][9] |= 0x80;
    expected.status = 0x8088;
    expected.instruction_pointer = 0x1002;
    expected.opcode = 0x119;
    expected.code_selector = 0x1b;
    expected.data_pointer = 0x5010;
    expected.data_selector = 0x23;
    assert_eq!(actual_fp(&paired), expected);
    assert_eq!(ram(&paired), before_ram);
    assert_eq!(paired.memory().unwrap().transaction_version(), version);
    assert_packet(&paired, 7, [0, FLAGS, 0, 0, 0, 4]);
}

#[test]
fn aliased_read_and_final_store_preserve_literal_operand_and_fp_stack() {
    let (mut paired, id, _) = fixture(READ_STORE);
    let (mut ordinary, ordinary_id, descriptors) = fixture(READ_STORE);
    let before_ram = ram(&paired);
    let version = paired.memory().unwrap().transaction_version();
    scalar(&mut ordinary, ordinary_id, descriptors[0], 0x5000);
    scalar(&mut ordinary, ordinary_id, descriptors[1], 0x5000);
    assert_eq!(
        paired.x87_resident_pair(KEY, id, 0x1000, [0x5000, 0x5000], FLAGS, 2),
        X87PairOutcome::complete(2, 1)
    );
    assert_same(&paired, &ordinary);
    let mut expected = X87State {
        opcode: 0x119,
        instruction_pointer: 0x1002,
        data_pointer: 0x5000,
        code_selector: 0x1b,
        data_selector: 0x23,
        ..X87State::default()
    };
    expected.registers[7] = [0, 0, 0, 0, 0, 0, 0, 0xa0, 0xff, 0x3f];
    assert_eq!(actual_fp(&paired), expected);
    assert_eq!(ram(&paired), before_ram);
    assert_eq!(paired.memory().unwrap().transaction_version(), version + 1);
}

#[test]
fn source_alias_store_commits_both_steps_and_reports_code_invalidation() {
    let (mut paired, id, _) = fixture(REGISTER_STORE);
    let (mut ordinary, ordinary_id, descriptors) = fixture(REGISTER_STORE);
    for engine in [&mut paired, &mut ordinary] {
        fp(engine, one());
    }
    let data_before = ram(&paired);
    scalar(&mut ordinary, ordinary_id, descriptors[0], 0);
    assert_eq!(
        scalar_result(&mut ordinary, ordinary_id, descriptors[1], 0x1000),
        Ok(StoreCompletion::CodeInvalidated)
    );
    let outcome = paired.x87_resident_pair(KEY, id, 0x1000, [0, 0x1000], FLAGS, 2);
    assert_eq!((outcome.completed, outcome.reported_step), (2, 1));
    assert_eq!(outcome.result, Ok(StoreCompletion::CodeInvalidated));
    assert_same(&paired, &ordinary);
    assert_eq!(actual_fp(&paired), stored_negative(0x1000));
    assert_eq!(ram(&paired), data_before);
    let mut source = [0; 6];
    paired
        .memory()
        .unwrap()
        .read(GuestAddress(0x1000), &mut source)
        .unwrap();
    assert_eq!(source, [0, 0, 0x80, 0xbf, 0xff, 0xe2]);
    assert!(matches!(
        paired.resident_bytes(id),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    ));
    assert_packet(&paired, 7, [0, FLAGS, 0, 0, 0, 4]);
}
