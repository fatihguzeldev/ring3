use ring3_engine::{
    abi::{
        arena::{HELPER_OFFSET, TRANSFER_OFFSET, X87_OFFSET},
        x86::{encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, dbt::RegistryError, x86::State32},
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError, StoreCompletion},
    windows::CallingConvention32,
};

const KEY: u64 = 0x4341_5316_4845_4c50;
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
    upload(&mut e, 0x3000, &[0x11, 0x22, 0x33, 0x44]);
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
    let arena = e.arena_mut().unwrap();
    arena[X87_OFFSET + 16..X87_OFFSET + 18].copy_from_slice(&0x027f_u16.to_le_bytes());
    arena[X87_OFFSET + 18..X87_OFFSET + 20].copy_from_slice(&0x81a5_u16.to_le_bytes());
    for (i, byte) in arena[X87_OFFSET + 40..X87_OFFSET + 120]
        .iter_mut()
        .enumerate()
    {
        *byte = (i as u8).wrapping_mul(29).wrapping_add(17);
    }
    (e, id)
}

fn compare_exchange(
    e: &mut EngineInstance,
    resident: bool,
    id: u64,
    address: u32,
    expected: u32,
    desired: u32,
) -> Result<StoreCompletion, HostError> {
    if resident {
        e.compare_exchange_resident16(KEY, id, address, expected, desired)
    } else {
        e.compare_exchange16(address, expected, desired)
    }
}

fn record(fields: [u32; 6]) -> [u8; 40] {
    let mut out = [0; 40];
    out[..16].copy_from_slice(&[b'R', b'3', b'M', b'H', 6, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]);
    for (i, field) in fields.into_iter().enumerate() {
        out[16 + i * 4..20 + i * 4].copy_from_slice(&field.to_le_bytes());
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
fn word_compare_exchange_helpers_mask_inputs_and_return_old_for_both_outcomes() {
    for resident in [false, true] {
        let (mut e, id) = fixture(resident, false);
        for address in [DATA + 16, DATA + 1, DATA + 4095, u32::MAX - 1] {
            let initial = if address == u32::MAX - 1 {
                &[0x5a, 0xb6, 0xa7][..]
            } else {
                &[0x5a, 0xb6, 0xa7, 0xa5][..]
            };
            upload(&mut e, address - 1, initial);
            for (expected, desired, old, committed) in [
                (0xa1b2_3456, 0xc3d4_d3e4, 0xa7b6, 0xa7b6_u16),
                (0xa1b2_a7b6, 0xc3d4_d3e4, 0xa7b6, 0xd3e4),
                (0xffff_d3e4, 0xffff_d3e4, 0xd3e4, 0xd3e4),
            ] {
                let before = e.arena().to_vec();
                assert_eq!(
                    compare_exchange(&mut e, resident, id, address, expected, desired),
                    Ok(StoreCompletion::Complete)
                );
                only_helper(&e, &before, [0, old, 0, 0, 0, 2]);
                let mut frame = initial.to_vec();
                frame[1..3].copy_from_slice(&committed.to_le_bytes());
                assert_eq!(read(&e, address - 1, initial.len()), frame);
            }
        }
    }
}

#[test]
fn word_compare_exchange_faults_are_read_first_and_repairs_preserve_current_bytes() {
    for resident in [false, true] {
        let (mut e, id) = fixture(resident, false);
        for bits in 0..8 {
            upload(&mut e, DATA, &[0x5a, 0xb6, 0xa7, 0xa5]);
            e.protect(DATA, 1, bits).unwrap();
            let before = e.arena().to_vec();
            assert_eq!(
                compare_exchange(&mut e, resident, id, DATA + 1, 0x3456, 0xd3e4),
                Ok(StoreCompletion::Complete)
            );
            only_helper(
                &e,
                &before,
                if bits == 3 || bits == 7 {
                    [0, 0xa7b6, 0, 0, 0, 2]
                } else {
                    [1, 0, 2, DATA + 1, if bits & 1 == 0 { 1 } else { 2 }, 2]
                },
            );
            e.protect(DATA, 1, 3).unwrap();
            assert_eq!(read(&e, DATA, 4), [0x5a, 0xb6, 0xa7, 0xa5]);
        }
        upload(&mut e, DATA + 4094, &[0x5a, 0xb6, 0xa7, 0xa5]);
        for (first, second, access, at) in [
            (1, 2, 1, DATA + 4096),
            (1, 1, 2, DATA + 4095),
            (3, 1, 2, DATA + 4096),
        ] {
            e.protect(DATA, 1, first).unwrap();
            e.protect(DATA + 4096, 1, second).unwrap();
            for _ in 0..2 {
                let before = e.arena().to_vec();
                assert_eq!(
                    compare_exchange(&mut e, resident, id, DATA + 4095, 0x3456, 0xd3e4),
                    Ok(StoreCompletion::Complete)
                );
                only_helper(&e, &before, [1, 0, 2, at, access, 2]);
                assert_eq!(read(&e, DATA + 4094, 2), [0x5a, 0xb6]);
                if second & 1 != 0 {
                    assert_eq!(read(&e, DATA + 4096, 2), [0xa7, 0xa5]);
                }
            }
        }
        e.protect(DATA + 4096, 1, 3).unwrap();
        let before = e.arena().to_vec();
        assert_eq!(
            compare_exchange(&mut e, resident, id, DATA + 4095, 0x3456, 0xd3e4),
            Ok(StoreCompletion::Complete)
        );
        only_helper(&e, &before, [0, 0xa7b6, 0, 0, 0, 2]);
        assert_eq!(read(&e, DATA + 4094, 4), [0x5a, 0xb6, 0xa7, 0xa5]);
        e.unmap(DATA + 4096, 1).unwrap();
        for _ in 0..2 {
            let before = e.arena().to_vec();
            assert_eq!(
                compare_exchange(&mut e, resident, id, DATA + 4095, 0xa7b6, 0xd3e4),
                Ok(StoreCompletion::Complete)
            );
            only_helper(&e, &before, [1, 0, 1, DATA + 4096, 1, 2]);
            assert_eq!(read(&e, DATA + 4094, 2), [0x5a, 0xb6]);
        }
        e.map(DATA + 4096, 1, 3).unwrap();
        for (expected, old, committed) in [(0xa7b6, 0xb6, 0xb6_u16), (0xb6, 0xb6, 0xd3e4)] {
            let before = e.arena().to_vec();
            assert_eq!(
                compare_exchange(&mut e, resident, id, DATA + 4095, expected, 0xd3e4),
                Ok(StoreCompletion::Complete)
            );
            only_helper(&e, &before, [0, old, 0, 0, 0, 2]);
            let bytes = committed.to_le_bytes();
            assert_eq!(read(&e, DATA + 4094, 4), [0x5a, bytes[0], bytes[1], 0]);
        }
        for mapped in [false, true] {
            let (mut e, id) = fixture(resident, false);
            upload(&mut e, 0, &[0x5a, 0xa5]);
            upload(&mut e, u32::MAX - 2, &[0x5a, 0xb6, 0xa7]);
            if mapped {
                e.protect(0xffff_f000, 1, 0).unwrap();
            } else {
                e.unmap(0xffff_f000, 1).unwrap();
            }
            for _ in 0..2 {
                let before = e.arena().to_vec();
                assert_eq!(
                    compare_exchange(&mut e, resident, id, u32::MAX, 0, 1),
                    Ok(StoreCompletion::Complete)
                );
                only_helper(&e, &before, [1, 0, 3, u32::MAX, 1, 2]);
                assert_eq!(read(&e, 0, 2), [0x5a, 0xa5]);
            }
            if mapped {
                e.protect(0xffff_f000, 1, 1).unwrap();
                assert_eq!(read(&e, u32::MAX - 2, 3), [0x5a, 0xb6, 0xa7]);
            }
        }
    }
}

#[test]
fn word_compare_exchange_mismatch_and_identity_writes_invalidate_consumed_code() {
    for resident in [false, true] {
        for (matched, changed) in [(false, false), (true, false), (true, true)] {
            let (mut e, id) = fixture(resident, false);
            let unrelated = e
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(0x3000), 4)
                .unwrap();
            let before = e.arena().to_vec();
            assert_eq!(
                compare_exchange(&mut e, resident, id, 0x3000, 0x2210, 0xffff),
                Ok(StoreCompletion::Complete)
            );
            only_helper(&e, &before, [0, 0x2211, 0, 0, 0, 2]);
            assert!(!e.memory().unwrap().is_code_current(&unrelated));
            assert_eq!(read(&e, 0x3000, 4), [0x11, 0x22, 0x33, 0x44]);
            if resident {
                e.guard_resident(KEY, id).unwrap();
            } else {
                e.guard(KEY, id as u32).unwrap();
            }
            let before = e.arena().to_vec();
            assert_eq!(
                compare_exchange(
                    &mut e,
                    resident,
                    id,
                    CODE,
                    if matched { 0xeb90 } else { 0xeb91 },
                    if changed { 0xeb91 } else { 0xeb90 }
                ),
                Ok(StoreCompletion::CodeInvalidated)
            );
            only_helper(&e, &before, [0, 0xeb90, 0, 0, 0, 2]);
            assert_eq!(
                read(&e, CODE, 3),
                [if changed { 0x91 } else { 0x90 }, 0xeb, 0]
            );
            let before = e.arena().to_vec();
            assert_eq!(
                compare_exchange(&mut e, resident, id, DATA, 0, 1),
                Err(if resident {
                    HostError::Resident(RegistryError::CodeInvalidated)
                } else {
                    HostError::CodeInvalidated
                })
            );
            assert_eq!(e.arena(), before);
            assert_eq!(read(&e, DATA, 2), [0, 0]);
        }
    }
}

#[test]
fn word_compare_exchange_owner_pending_and_closed_rejections_are_neutral() {
    let mut e = EngineInstance::new(1, KEY).unwrap();
    let before = e.arena().to_vec();
    assert_eq!(
        e.compare_exchange16(u32::MAX, 0, 1),
        Err(HostError::InvalidArtifact)
    );
    assert_eq!(e.arena(), before);
    let (mut e, id) = fixture(true, false);
    for (key, wrong_id, error) in [
        (KEY ^ 1, id, HostError::InvalidArtifact),
        (KEY, 0, HostError::Resident(RegistryError::InvalidUnit)),
        (
            KEY,
            u64::MAX,
            HostError::Resident(RegistryError::InvalidUnit),
        ),
    ] {
        let before = e.arena().to_vec();
        assert_eq!(
            e.compare_exchange_resident16(key, wrong_id, u32::MAX, 0, 1),
            Err(error)
        );
        assert_eq!(e.arena(), before);
        assert_eq!(read(&e, DATA, 2), [0, 0]);
    }
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
            compare_exchange(&mut e, resident, id, u32::MAX, 0, 1),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(e.arena(), before);
        assert_eq!(read(&e, DATA, 2), [0, 0]);
        e.close();
        assert_eq!(e.compare_exchange16(u32::MAX, 0, 1), Err(HostError::Closed));
        assert_eq!(
            e.compare_exchange_resident16(KEY, id, u32::MAX, 0, 1),
            Err(HostError::Closed)
        );
        assert_eq!(e.arena(), before);
    }
}
