use ring3_engine::{
    abi::{
        arena::TRANSFER_OFFSET,
        x86::{encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, dbt::RegistryError, x86::State32},
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError, StoreCompletion},
    windows::CallingConvention32,
};

const KEY: u64 = 0x5843_4847_4845_4c50;
const CODE: u32 = 0x1000;
const DATA: u32 = 0x5000;

fn upload(e: &mut EngineInstance, address: u32, bytes: &[u8]) {
    e.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()].copy_from_slice(bytes);
    e.upload(address, bytes.len() as u32).unwrap();
}

fn fixture(resident: bool, gate: bool) -> (EngineInstance, u64) {
    let mut e = EngineInstance::new(7, KEY).unwrap();
    for page in [CODE, 0x3000, DATA, DATA + 4096, 0x8000, 0xffff_f000] {
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
    descriptor[140..144].copy_from_slice(&CODE.to_le_bytes());
    descriptor[144..148].copy_from_slice(&if gate { 2_u32 } else { 3_u32 }.to_le_bytes());
    descriptor[148..152].copy_from_slice(&CODE.to_le_bytes());
    descriptor[152..156].copy_from_slice(&17_u32.to_le_bytes());
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
        e.exchange_resident32(KEY, id, address, value)
    } else {
        e.exchange32(address, value)
    }
}

fn record(fields: [u32; 6]) -> [u8; 40] {
    let mut out = [0; 40];
    out[..16].copy_from_slice(&[b'R', b'3', b'M', b'H', 1, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]);
    for (index, field) in fields.into_iter().enumerate() {
        out[16 + index * 4..20 + index * 4].copy_from_slice(&field.to_le_bytes());
    }
    out
}

fn only_helper(e: &EngineInstance, before: &[u8], fields: [u32; 6]) {
    let mut expected = before.to_vec();
    expected[100..140].copy_from_slice(&record(fields));
    assert_eq!(e.arena(), expected);
}

fn read(e: &EngineInstance, address: u32) -> u32 {
    let mut out = [0; 4];
    e.memory()
        .unwrap()
        .read(GuestAddress(address), &mut out)
        .unwrap();
    u32::from_le_bytes(out)
}

#[test]
fn exchange_helpers_return_the_original_little_endian_word_and_touch_only_the_packet() {
    for resident in [false, true] {
        let (mut e, id) = fixture(resident, false);
        for address in [DATA + 1, DATA + 4093, DATA + 4095, 0xffff_fffc] {
            upload(&mut e, address - 1, &[0x5a, 0x11, 0x22, 0x33, 0x44]);
            let before = e.arena().to_vec();
            assert_eq!(
                exchange(&mut e, resident, id, address, 0x89ab_cdef),
                Ok(StoreCompletion::Complete)
            );
            only_helper(&e, &before, [0, 0x4433_2211, 0, 0, 0, 0]);
            assert_eq!(read(&e, address), 0x89ab_cdef);
            let mut neighbor = [0];
            e.memory()
                .unwrap()
                .read(GuestAddress(address - 1), &mut neighbor)
                .unwrap();
            assert_eq!(neighbor, [0x5a]);
            let before = e.arena().to_vec();
            assert_eq!(
                exchange(&mut e, resident, id, address, 0x4433_2211),
                Ok(StoreCompletion::Complete)
            );
            only_helper(&e, &before, [0, 0x89ab_cdef, 0, 0, 0, 0]);
            assert_eq!(read(&e, address), 0x4433_2211);
        }
    }
}

#[test]
fn read_first_preflight_faults_do_not_expose_old_words_or_partially_write_ram() {
    for resident in [false, true] {
        let (mut e, id) = fixture(resident, false);
        let address = DATA + 4094;
        e.write32(address, 0x4433_2211).unwrap();
        e.protect(DATA, 1, 1).unwrap();
        e.protect(DATA + 4096, 1, 2).unwrap();
        let before = e.arena().to_vec();
        assert_eq!(
            exchange(&mut e, resident, id, address, u32::MAX),
            Ok(StoreCompletion::Complete)
        );
        only_helper(&e, &before, [1, 0, 2, DATA + 4096, 1, 4]);
        e.protect(DATA + 4096, 1, 1).unwrap();
        assert_eq!(read(&e, address), 0x4433_2211);
        let before = e.arena().to_vec();
        assert_eq!(
            exchange(&mut e, resident, id, address, u32::MAX),
            Ok(StoreCompletion::Complete)
        );
        only_helper(&e, &before, [1, 0, 2, address, 2, 4]);
        assert_eq!(read(&e, address), 0x4433_2211);
        e.protect(DATA, 1, 3).unwrap();
        e.protect(DATA + 4096, 1, 3).unwrap();
        assert_eq!(
            exchange(&mut e, resident, id, address, u32::MAX),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(&e.arena()[100..140], &record([0, 0x4433_2211, 0, 0, 0, 0]));
        assert_eq!(read(&e, address), u32::MAX);
    }
}

#[test]
fn overflow_and_unmapped_reads_replace_stale_values_without_wrapped_writes() {
    for resident in [false, true] {
        let (mut e, id) = fixture(resident, false);
        e.map(0, 1, 3).unwrap();
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
                exchange(&mut e, resident, id, address, 0),
                Ok(StoreCompletion::Complete)
            );
            only_helper(&e, &before, [1, 0, detail, address, 1, 4]);
            assert_eq!(read(&e, 0), 0x7654_3210);
            assert_eq!(read(&e, 0xffff_fffc), 0x89ab_cdef);
        }
    }
}

#[test]
fn identity_code_exchange_commits_once_and_reports_consumed_code_invalidation() {
    for resident in [false, true] {
        let (mut e, id) = fixture(resident, false);
        let old = read(&e, CODE);
        let before = e.arena().to_vec();
        assert_eq!(
            exchange(&mut e, resident, id, CODE, old),
            Ok(StoreCompletion::CodeInvalidated)
        );
        only_helper(&e, &before, [0, old, 0, 0, 0, 0]);
        assert_eq!(read(&e, CODE), old);
        let before = e.arena().to_vec();
        let rejected = exchange(&mut e, resident, id, DATA, 123);
        assert_eq!(
            rejected,
            Err(if resident {
                HostError::Resident(RegistryError::CodeInvalidated)
            } else {
                HostError::CodeInvalidated
            })
        );
        assert_eq!(e.arena(), before);
        assert_eq!(read(&e, DATA), 0);
    }
}

#[test]
fn exchange_to_unconsumed_executable_code_keeps_the_calling_owner_current() {
    for resident in [false, true] {
        let (mut e, id) = fixture(resident, false);
        upload(&mut e, 0x3000, &[0x11, 0x22, 0x33, 0x44]);
        assert_eq!(
            exchange(&mut e, resident, id, 0x3000, 0x4433_2211),
            Ok(StoreCompletion::Complete)
        );
        if resident {
            e.guard_resident(KEY, id).unwrap();
        } else {
            e.guard(KEY, id as u32).unwrap();
        }
        assert_eq!(&e.arena()[100..140], &record([0, 0x4433_2211, 0, 0, 0, 0]));
    }
}

#[test]
fn pending_call_admission_rejects_before_exchange_or_helper_publication() {
    for resident in [false, true] {
        let (mut e, id) = fixture(resident, true);
        e.write32(0x8000, 0x3000).unwrap();
        let state = State32 {
            registers: [1, 2, 3, 4, 0x8000, 6, 7, 8],
            eip: CODE,
            eflags: 0xcd7,
        };
        encode_state(&state, &mut e.arena_mut().unwrap()[..56]).unwrap();
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
        assert_eq!(read(&e, DATA), 0);
    }
}

#[test]
fn missing_owner_wrong_resident_identity_and_closed_rejections_are_neutral() {
    let mut e = EngineInstance::new(1, KEY).unwrap();
    let before = e.arena().to_vec();
    assert_eq!(
        e.exchange32(0xffff_ffff, 1),
        Err(HostError::InvalidArtifact)
    );
    assert_eq!(e.arena(), before);
    let (mut e, id) = fixture(true, false);
    for (key, wrong_id) in [(KEY ^ 1, id), (KEY, 0), (KEY, u64::MAX)] {
        let before = e.arena().to_vec();
        assert!(
            e.exchange_resident32(key, wrong_id, 0xffff_ffff, 1)
                .is_err()
        );
        assert_eq!(e.arena(), before);
        assert_eq!(read(&e, DATA), 0);
    }
    e.close();
    let before = e.arena().to_vec();
    assert_eq!(e.exchange32(0, 1), Err(HostError::Closed));
    assert_eq!(e.exchange_resident32(KEY, id, 0, 1), Err(HostError::Closed));
    assert_eq!(e.arena(), before);
}
