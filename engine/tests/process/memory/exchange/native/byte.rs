use ring3_engine::{
    abi::{
        arena::{HELPER_OFFSET, TRANSFER_OFFSET},
        x86::{encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, dbt::RegistryError, x86::State32},
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError, StoreCompletion},
    windows::CallingConvention32,
};

const KEY: u64 = 0x5843_4847_4845_4c38;
const CODE: u32 = 0x1000;
const DATA: u32 = 0x5000;

fn upload(e: &mut EngineInstance, address: u32, bytes: &[u8]) {
    e.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()].copy_from_slice(bytes);
    e.upload(address, bytes.len() as u32).unwrap();
}

fn fixture(resident: bool, gate: bool) -> (EngineInstance, u64) {
    let mut e = EngineInstance::new(7, KEY).unwrap();
    for page in [0, CODE, 0x3000, DATA, DATA + 4096, 0x8000, 0xffff_f000] {
        e.map(page, 1, if page == CODE || page == 0x3000 { 7 } else { 3 })
            .unwrap();
    }
    upload(
        &mut e,
        CODE,
        if gate {
            &[0x0f, 0x0b]
        } else {
            &[0x90, 0xeb, 0]
        },
    );
    let descriptor = e.arena_mut().unwrap();
    descriptor[TRANSFER_OFFSET..TRANSFER_OFFSET + 4].copy_from_slice(&CODE.to_le_bytes());
    descriptor[TRANSFER_OFFSET + 4..TRANSFER_OFFSET + 8]
        .copy_from_slice(&if gate { 2_u32 } else { 3_u32 }.to_le_bytes());
    descriptor[TRANSFER_OFFSET + 8..TRANSFER_OFFSET + 12].copy_from_slice(&CODE.to_le_bytes());
    descriptor[TRANSFER_OFFSET + 12..TRANSFER_OFFSET + 16].copy_from_slice(&17_u32.to_le_bytes());
    let id = match (resident, gate) {
        (false, false) => u64::from(e.compile(1).unwrap()),
        (false, true) => u64::from(e.compile_with_gates(1, 1).unwrap()),
        (true, false) => e.compile_resident(1).unwrap().get(),
        (true, true) => e.compile_resident_with_gates(1, 1).unwrap().get(),
    };
    (e, id)
}

fn exchange(
    e: &mut EngineInstance,
    resident: bool,
    id: u64,
    address: u32,
    value: u32,
) -> Result<StoreCompletion, HostError> {
    if resident {
        e.exchange_resident8(KEY, id, address, value)
    } else {
        e.exchange8(address, value)
    }
}

fn record(fields: [u32; 6]) -> [u8; 40] {
    let mut out = [0; 40];
    out[..16].copy_from_slice(&[b'R', b'3', b'M', b'H', 5, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]);
    for (index, field) in fields.into_iter().enumerate() {
        out[16 + index * 4..20 + index * 4].copy_from_slice(&field.to_le_bytes());
    }
    out
}

fn only_helper(e: &EngineInstance, before: &[u8], fields: [u32; 6]) {
    let mut expected = before.to_vec();
    expected[HELPER_OFFSET..HELPER_OFFSET + 40].copy_from_slice(&record(fields));
    assert_eq!(e.arena(), expected);
}

fn read(e: &EngineInstance, address: u32, length: usize) -> Vec<u8> {
    let mut out = vec![0; length];
    e.memory()
        .unwrap()
        .read(GuestAddress(address), &mut out)
        .unwrap();
    out
}

#[test]
fn byte_exchange_truncates_input_returns_old_byte_and_preserves_all_neighbors_at_top() {
    for resident in [false, true] {
        let (mut e, id) = fixture(resident, false);
        for (address, start, bytes, selected) in [
            (0, 0, vec![0x11, 0xa5], 0),
            (DATA + 1, DATA, vec![0x5a, 0x11, 0xa5], 1),
            (DATA + 4095, DATA + 4094, vec![0x5a, 0x11, 0xa5], 1),
            (u32::MAX, u32::MAX - 1, vec![0x5a, 0x11], 1),
        ] {
            upload(&mut e, start, &bytes);
            let before = e.arena().to_vec();
            assert_eq!(
                exchange(&mut e, resident, id, address, 0x89ab_cdef),
                Ok(StoreCompletion::Complete)
            );
            only_helper(&e, &before, [0, 0x11, 0, 0, 0, 1]);
            let mut expected = bytes;
            expected[selected] = 0xef;
            assert_eq!(read(&e, start, expected.len()), expected);
            let before = e.arena().to_vec();
            assert_eq!(
                exchange(&mut e, resident, id, address, 0xffff_ffef),
                Ok(StoreCompletion::Complete)
            );
            only_helper(&e, &before, [0, 0xef, 0, 0, 0, 1]);
            assert_eq!(read(&e, start, expected.len()), expected);
        }
    }
}

#[test]
fn every_permission_mode_prioritizes_read_then_write_without_exposing_old_byte() {
    for resident in [false, true] {
        let (mut e, id) = fixture(resident, false);
        for bits in 0..8 {
            upload(&mut e, DATA, &[0x5a, 0x11, 0xa5]);
            e.protect(DATA, 1, bits).unwrap();
            let before = e.arena().to_vec();
            assert_eq!(
                exchange(&mut e, resident, id, DATA + 1, u32::MAX),
                Ok(StoreCompletion::Complete)
            );
            let success = bits == 3 || bits == 7;
            only_helper(
                &e,
                &before,
                if success {
                    [0, 0x11, 0, 0, 0, 1]
                } else {
                    [1, 0, 2, DATA + 1, if bits & 1 == 0 { 1 } else { 2 }, 1]
                },
            );
            if resident {
                e.guard_resident(KEY, id).unwrap();
            } else {
                e.guard(KEY, id as u32).unwrap();
            }
            e.protect(DATA, 1, 3).unwrap();
            assert_eq!(
                read(&e, DATA, 3),
                if success {
                    vec![0x5a, 0xff, 0xa5]
                } else {
                    vec![0x5a, 0x11, 0xa5]
                }
            );
        }
    }
}

#[test]
fn page_final_byte_does_not_touch_unmapped_neighbor_and_top_unmapped_is_a_read_fault() {
    for resident in [false, true] {
        let (mut e, id) = fixture(resident, false);
        e.unmap(DATA + 4096, 1).unwrap();
        upload(&mut e, DATA + 4094, &[0x5a, 0x11]);
        let before = e.arena().to_vec();
        assert_eq!(
            exchange(&mut e, resident, id, DATA + 4095, 0x22),
            Ok(StoreCompletion::Complete)
        );
        only_helper(&e, &before, [0, 0x11, 0, 0, 0, 1]);
        assert_eq!(read(&e, DATA + 4094, 2), [0x5a, 0x22]);
        e.unmap(0xffff_f000, 1).unwrap();
        for address in [0x9008, u32::MAX] {
            let before = e.arena().to_vec();
            assert_eq!(
                exchange(&mut e, resident, id, address, 0xff),
                Ok(StoreCompletion::Complete)
            );
            only_helper(&e, &before, [1, 0, 1, address, 1, 1]);
            assert_eq!(read(&e, 0, 2), [0, 0]);
        }
        e.map(0xffff_f000, 1, 3).unwrap();
        let before = e.arena().to_vec();
        assert_eq!(
            exchange(&mut e, resident, id, u32::MAX, 0xff),
            Ok(StoreCompletion::Complete)
        );
        only_helper(&e, &before, [0, 0, 0, 0, 0, 1]);
        assert_eq!(read(&e, u32::MAX, 1), [0xff]);
    }
}

#[test]
fn consumed_identity_byte_invalidates_once_while_unrelated_executable_write_keeps_owner() {
    for resident in [false, true] {
        let (mut e, id) = fixture(resident, false);
        upload(&mut e, 0x3000, &[0x5a, 0x11, 0xa5]);
        let before = e.arena().to_vec();
        assert_eq!(
            exchange(&mut e, resident, id, 0x3001, 0x11),
            Ok(StoreCompletion::Complete)
        );
        only_helper(&e, &before, [0, 0x11, 0, 0, 0, 1]);
        if resident {
            e.guard_resident(KEY, id).unwrap();
        } else {
            e.guard(KEY, id as u32).unwrap();
        }
        let before = e.arena().to_vec();
        assert_eq!(
            exchange(&mut e, resident, id, CODE, 0x90),
            Ok(StoreCompletion::CodeInvalidated)
        );
        only_helper(&e, &before, [0, 0x90, 0, 0, 0, 1]);
        assert_eq!(read(&e, CODE, 3), [0x90, 0xeb, 0]);
        let before = e.arena().to_vec();
        assert_eq!(
            exchange(&mut e, resident, id, DATA, 123),
            Err(if resident {
                HostError::Resident(RegistryError::CodeInvalidated)
            } else {
                HostError::CodeInvalidated
            })
        );
        assert_eq!(e.arena(), before);
        assert_eq!(read(&e, DATA, 1), [0]);
    }
}

#[test]
fn pending_call_rejects_before_any_byte_or_helper_change() {
    for resident in [false, true] {
        let (mut e, id) = fixture(resident, true);
        e.write32(0x8000, 0x3000).unwrap();
        encode_state(
            &State32 {
                registers: [1, 2, 3, 4, 0x8000, 6, 7, 8],
                eip: CODE,
                eflags: 0xcd7,
            },
            &mut e.arena_mut().unwrap()[..56],
        )
        .unwrap();
        encode_exit_v3(
            &ExecutionExit {
                retired: 1,
                reason: ExitReason::Gate { id: 17 },
            },
            &mut e.arena_mut().unwrap()[56..96],
        )
        .unwrap();
        if resident {
            e.capture_resident_call(KEY, id, CallingConvention32::Cdecl, 0)
                .unwrap();
        } else {
            e.capture_call(KEY, id as u32, CallingConvention32::Cdecl, 0)
                .unwrap();
        }
        let before = e.arena().to_vec();
        assert_eq!(
            exchange(&mut e, resident, id, DATA, 123),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(e.arena(), before);
        assert_eq!(read(&e, DATA, 1), [0]);
    }
}

#[test]
fn missing_owner_wrong_identity_and_closed_calls_are_neutral_before_top_address_access() {
    let mut e = EngineInstance::new(1, KEY).unwrap();
    let before = e.arena().to_vec();
    assert_eq!(e.exchange8(u32::MAX, 1), Err(HostError::InvalidArtifact));
    assert_eq!(e.arena(), before);
    let (mut e, id) = fixture(true, false);
    for (key, wrong_id) in [(KEY ^ 1, id), (KEY, 0), (KEY, u64::MAX)] {
        let before = e.arena().to_vec();
        assert!(e.exchange_resident8(key, wrong_id, u32::MAX, 1).is_err());
        assert_eq!(e.arena(), before);
        assert_eq!(read(&e, u32::MAX, 1), [0]);
    }
    e.close();
    let before = e.arena().to_vec();
    assert_eq!(e.exchange8(0, 1), Err(HostError::Closed));
    assert_eq!(e.exchange_resident8(KEY, id, 0, 1), Err(HostError::Closed));
    assert_eq!(e.arena(), before);
}
