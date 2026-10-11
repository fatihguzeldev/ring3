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

const KEY: u64 = 0x5841_4444_3248_454c;
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

fn exchange_add(
    e: &mut EngineInstance,
    resident: bool,
    id: u64,
    address: u32,
    source: u32,
) -> Result<StoreCompletion, HostError> {
    if resident {
        e.exchange_add_resident32(KEY, id, address, source)
    } else {
        e.exchange_add32(address, source)
    }
}

fn record(fields: [u32; 6]) -> [u8; 40] {
    let mut out = [0; 40];
    out[..16].copy_from_slice(&[b'R', b'3', b'M', b'H', 1, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]);
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

fn read32(e: &EngineInstance, address: u32) -> u32 {
    u32::from_le_bytes(read(e, address, 4).try_into().unwrap())
}

#[test]
fn exchange_add_helpers_return_old_for_wrapping_sums_and_zero_source() {
    for resident in [false, true] {
        let (mut e, id) = fixture(resident, false);
        for address in [DATA + 16, DATA + 1, DATA + 4093, DATA + 4095, 0xffff_fffc] {
            for (old, source, committed) in [
                (0x7fff_fffe_u32, 4, 0x8000_0002_u32),
                (0xffff_fff0, 0x20, 0x10),
                (0, 0xffff_ffff, 0xffff_ffff),
                (0x4433_2211, 0, 0x4433_2211),
            ] {
                let mut frame = vec![0x5a];
                frame.extend_from_slice(&old.to_le_bytes());
                upload(&mut e, address - 1, &frame);
                let before = e.arena().to_vec();
                assert_eq!(
                    exchange_add(&mut e, resident, id, address, source),
                    Ok(StoreCompletion::Complete)
                );
                only_helper(&e, &before, [0, old, 0, 0, 0, 0]);
                frame[1..5].copy_from_slice(&committed.to_le_bytes());
                assert_eq!(read(&e, address - 1, 5), frame);
            }
        }
    }
}

#[test]
fn exchange_add_faults_are_read_first_and_repairs_use_current_inputs() {
    for resident in [false, true] {
        let (mut e, id) = fixture(resident, false);
        let address = DATA + 4094;
        e.write32(address, 0x4433_2211).unwrap();
        let original = read(&e, DATA, 8192);
        e.protect(DATA, 1, 1).unwrap();
        e.protect(DATA + 4096, 1, 2).unwrap();
        for source in [0, 1] {
            let before = e.arena().to_vec();
            assert_eq!(
                exchange_add(&mut e, resident, id, address, source),
                Ok(StoreCompletion::Complete)
            );
            only_helper(&e, &before, [1, 0, 2, DATA + 4096, 1, 4]);
        }
        e.protect(DATA + 4096, 1, 1).unwrap();
        assert_eq!(read(&e, DATA, 8192), original);
        for source in [0, 1] {
            let before = e.arena().to_vec();
            assert_eq!(
                exchange_add(&mut e, resident, id, address, source),
                Ok(StoreCompletion::Complete)
            );
            only_helper(&e, &before, [1, 0, 2, address, 2, 4]);
            assert_eq!(read(&e, DATA, 8192), original);
        }
        for page in [DATA, DATA + 4096] {
            e.protect(page, 1, 3).unwrap();
        }
        let before = e.arena().to_vec();
        assert_eq!(
            exchange_add(&mut e, resident, id, address, 0x10),
            Ok(StoreCompletion::Complete)
        );
        only_helper(&e, &before, [0, 0x4433_2211, 0, 0, 0, 0]);
        let mut current = original;
        current[4094..4098].copy_from_slice(&0x4433_2221_u32.to_le_bytes());
        assert_eq!(read(&e, DATA, 8192), current);
        e.write32(address, 0xffff_fff0).unwrap();
        let before = e.arena().to_vec();
        assert_eq!(
            exchange_add(&mut e, resident, id, address, 0x20),
            Ok(StoreCompletion::Complete)
        );
        only_helper(&e, &before, [0, 0xffff_fff0, 0, 0, 0, 0]);
        current[4094..4098].copy_from_slice(&0x10_u32.to_le_bytes());
        assert_eq!(read(&e, DATA, 8192), current);
        e.write32(0, 0x7654_3210).unwrap();
        e.write32(0xffff_fffc, 0x89ab_cdef).unwrap();
        for (address, detail) in [
            (0x9008, 1),
            (0xffff_fffd, 3),
            (0xffff_fffe, 3),
            (0xffff_ffff, 3),
        ] {
            let before = e.arena().to_vec();
            assert_eq!(
                exchange_add(&mut e, resident, id, address, 1),
                Ok(StoreCompletion::Complete)
            );
            only_helper(&e, &before, [1, 0, detail, address, 1, 4]);
            assert_eq!(read32(&e, 0), 0x7654_3210);
            assert_eq!(read32(&e, 0xffff_fffc), 0x89ab_cdef);
        }
    }
}

#[test]
fn exchange_add_identity_and_changed_writes_invalidate_only_consumed_code() {
    for resident in [false, true] {
        for source in [0, 1] {
            let (mut e, id) = fixture(resident, false);
            let unrelated = e
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(0x3000), 4)
                .unwrap();
            let before = e.arena().to_vec();
            assert_eq!(
                exchange_add(&mut e, resident, id, 0x3000, 0),
                Ok(StoreCompletion::Complete)
            );
            only_helper(&e, &before, [0, 0x4433_2211, 0, 0, 0, 0]);
            assert!(!e.memory().unwrap().is_code_current(&unrelated));
            assert_eq!(read32(&e, 0x3000), 0x4433_2211);
            if resident {
                e.guard_resident(KEY, id).unwrap();
            } else {
                e.guard(KEY, id as u32).unwrap();
            }
            let old = read32(&e, CODE);
            let before = e.arena().to_vec();
            assert_eq!(
                exchange_add(&mut e, resident, id, CODE, source),
                Ok(StoreCompletion::CodeInvalidated)
            );
            only_helper(&e, &before, [0, old, 0, 0, 0, 0]);
            assert_eq!(read32(&e, CODE), old.wrapping_add(source));
            let before = e.arena().to_vec();
            assert_eq!(
                exchange_add(&mut e, resident, id, DATA, 123),
                Err(if resident {
                    HostError::Resident(RegistryError::CodeInvalidated)
                } else {
                    HostError::CodeInvalidated
                })
            );
            assert_eq!(e.arena(), before);
            assert_eq!(read32(&e, DATA), 0);
        }
    }
}

#[test]
fn exchange_add_owner_pending_and_closed_rejections_are_neutral() {
    let mut e = EngineInstance::new(1, KEY).unwrap();
    let before = e.arena().to_vec();
    assert_eq!(
        e.exchange_add32(u32::MAX, 1),
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
            e.exchange_add_resident32(key, wrong_id, u32::MAX, 1),
            Err(error)
        );
        assert_eq!(e.arena(), before);
        assert_eq!(read32(&e, DATA), 0);
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
            exchange_add(&mut e, resident, id, u32::MAX, 1),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(e.arena(), before);
        assert_eq!(read32(&e, DATA), 0);
        e.close();
        assert_eq!(e.exchange_add32(u32::MAX, 1), Err(HostError::Closed));
        assert_eq!(
            e.exchange_add_resident32(KEY, id, u32::MAX, 1),
            Err(HostError::Closed)
        );
        assert_eq!(e.arena(), before);
    }
}
