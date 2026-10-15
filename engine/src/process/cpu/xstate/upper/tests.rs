use super::*;
use crate::{
    abi::{
        arena::{SEGMENTS_OFFSET, TRANSFER_OFFSET},
        x86::{SEGMENTS_SIZE, encode_segment_context},
    },
    cpu::x86::{SegmentCache, SegmentContext},
    memory::GuestAddress,
};
fn engine() -> (EngineInstance, u64) {
    let mut engine = EngineInstance::new(4, 7).unwrap();
    for (address, pages, bits) in [(0x1000, 1, 7), (0x5000, 2, 3)] {
        engine.map(address, pages, bits).unwrap();
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0xeb, 0xfe])
        .unwrap();
    let descriptor = [0, 0x10, 0, 0, 2, 0, 0, 0];
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8].copy_from_slice(&descriptor);
    engine.compile(1).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8].copy_from_slice(&descriptor);
    let id = engine.compile_resident(1).unwrap().get();
    engine.arena_mut().unwrap()[YMM_OFFSET + 16..YMM_OFFSET + YMM_SIZE].fill(0x79);
    (engine, id)
}
fn context(engine: &mut EngineInstance, segment: SegmentRegister, limit: u32, kind: u32) {
    let mut context = SegmentContext::default();
    context.caches[segment.index()] =
        SegmentCache::from_bits(0xf200_0000 | (kind << 20) | limit).unwrap();
    encode_segment_context(
        &context,
        &mut engine.arena_mut().unwrap()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE],
    )
    .unwrap();
}
fn ram(engine: &EngineInstance) -> Vec<u8> {
    let mut bytes = vec![0; 8192];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x5000), &mut bytes)
        .unwrap();
    bytes
}
fn packet(engine: &EngineInstance, version: u16) -> [u32; 6] {
    let bytes = &engine.arena()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE];
    assert_eq!(&bytes[..4], b"R3MH");
    assert_eq!(&bytes[4..6], &version.to_le_bytes());
    assert_eq!(&bytes[6..16], b"\x01\0\x28\0\0\0\0\0\0\0");
    std::array::from_fn(|i| u32::from_le_bytes(bytes[16 + i * 4..20 + i * 4].try_into().unwrap()))
}
fn neutral(engine: &EngineInstance, before: &[u8], memory: &[u8], fields: [u32; 6]) {
    let mut expected = before.to_vec();
    let mut helper = [0; HELPER_SIZE];
    let error = match fields[0] {
        3 => CpuMemoryError::GeneralProtection(0),
        7 => CpuMemoryError::StackSegment(0),
        _ => panic!("segment error"),
    };
    encode_cpu_result_v8(832, 0, Err(error), &mut helper).unwrap();
    expected[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE].copy_from_slice(&helper);
    assert_eq!(engine.arena(), expected);
    assert_eq!(ram(engine), memory);
    assert_eq!(packet(engine, 8), fields);
}
#[test]
fn selected_cached_authority_precedes_alignment_and_paging_with_exact_ia32_endpoint() {
    for segment in [SegmentRegister::Ds, SegmentRegister::Ss] {
        for save in [false, true] {
            let (mut e, _) = engine();
            context(&mut e, segment, 0x50ff, 3);
            let before = e.arena().to_vec();
            let memory = ram(&e);
            if save {
                e.cpu_xsave_avx(segment.index() as u32, 0x5001, 4, 0)
                    .unwrap();
            } else {
                e.cpu_xrstor_avx(segment.index() as u32, 0x5001, 4, 0)
                    .unwrap();
            }
            neutral(
                &e,
                &before,
                &memory,
                [
                    if segment == SegmentRegister::Ss { 7 } else { 3 },
                    0,
                    0,
                    0,
                    0,
                    832,
                ],
            );
        }
    }
    let (mut e, _) = engine();
    context(&mut e, SegmentRegister::Ds, 0x52bf, 3);
    e.cpu_xsave_avx(3, 0x5000, 4, 0).unwrap();
    assert_eq!(packet(&e, 8), [0, 0, 0, 0, 0, 832]);
    let image = ram(&e);
    assert_eq!(&image[576..704], &[0x79; 128]);
    assert_eq!(&image[704..832], &[0; 128]);
    for segment in [SegmentRegister::Ds, SegmentRegister::Ss] {
        let (mut e, _) = engine();
        let before = e.arena().to_vec();
        let memory = ram(&e);
        e.cpu_xsave_avx(segment.index() as u32, 0xffff_fe00, 0, 0)
            .unwrap();
        neutral(
            &e,
            &before,
            &memory,
            [
                if segment == SegmentRegister::Ss { 7 } else { 3 },
                0,
                0,
                0,
                0,
                832,
            ],
        );
    }
}
#[test]
fn late_payload_faults_and_exhaustion_do_not_publish_ram_or_any_cpu_record() {
    for save in [false, true] {
        let (mut e, _) = engine();
        let address = 0x5dc0;
        let mut image = [0x5a; 832];
        image[512..536].fill(0);
        image[512] = 7;
        image[24..28].copy_from_slice(&0x5f80_u32.to_le_bytes());
        e.memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(address), &image)
            .unwrap();
        e.protect(0x6000, 1, if save { 1 } else { 2 }).unwrap();
        let before = e.arena().to_vec();
        let version = e.memory().unwrap().code_validation_version();
        if save {
            e.cpu_xsave_avx(3, address, 7, 0).unwrap();
        } else {
            e.cpu_xrstor_avx(3, address, 7, 0).unwrap();
        }
        assert_eq!(
            packet(&e, 15),
            [
                9,
                0,
                if save { 7 } else { 5 },
                0x6000,
                if save { 2 } else { 1 },
                832
            ]
        );
        assert_eq!(e.cr2(), Some(GuestAddress(0x6000)));
        let mut expected = before;
        expected[HELPER_OFFSET..HELPER_OFFSET + 40]
            .copy_from_slice(&e.arena()[HELPER_OFFSET..HELPER_OFFSET + 40]);
        assert_eq!(e.arena(), expected);
        assert_eq!(e.memory().unwrap().code_validation_version(), version);
        e.protect(0x6000, 1, 3).unwrap();
        let mut after = [0; 832];
        e.memory()
            .unwrap()
            .read(GuestAddress(address), &mut after)
            .unwrap();
        assert_eq!(after, image);
    }
    let (mut e, _) = engine();
    let before = e.arena().to_vec();
    let memory = ram(&e);
    let version = e.memory().unwrap().code_validation_version();
    e.memory.as_mut().unwrap().exhaust_versions_for_test();
    e.cpu_xsave_avx(3, 0x5000, 7, 0).unwrap();
    assert_eq!(packet(&e, 8), [2, 0, 1, 0, 0, 832]);
    assert_eq!(e.cr2(), None);
    let mut expected = before;
    expected[HELPER_OFFSET..HELPER_OFFSET + 40]
        .copy_from_slice(&e.arena()[HELPER_OFFSET..HELPER_OFFSET + 40]);
    assert_eq!(e.arena(), expected);
    assert_eq!(ram(&e), memory);
    assert_eq!(e.memory().unwrap().code_validation_version(), version);
}
#[test]
fn upper_save_uses_one_version_and_currentness_transaction_for_selected_code_spans() {
    for resident in [false, true] {
        let (mut e, id) = engine();
        let version = e.memory().unwrap().code_validation_version();
        let result = if resident {
            e.cpu_xsave_avx_resident(7, id, 3, 0x1000, 7, 0)
        } else {
            e.cpu_xsave_avx(3, 0x1000, 7, 0)
        };
        assert_eq!(result, Ok(StoreCompletion::CodeInvalidated));
        assert_eq!(e.memory().unwrap().code_validation_version(), version + 1);
        assert_eq!(packet(&e, 8), [0, 0, 0, 0, 0, 832]);
        let before = e.arena().to_vec();
        let rejected = if resident {
            e.cpu_xsave_avx_resident(7, id, 3, 0x1000, 4, 0)
        } else {
            e.cpu_xsave_avx(3, 0x1000, 4, 0)
        };
        assert!(rejected.is_err());
        assert_eq!(e.arena(), before);
    }
}
#[test]
fn malformed_upper_and_host_authority_fail_before_guest_packet_or_ram_publication() {
    let (mut e, id) = engine();
    let before = e.arena().to_vec();
    let memory = ram(&e);
    assert_eq!(
        e.cpu_xsave_avx_resident(8, id, 99, 1, 7, 0),
        Err(HostError::InvalidArtifact)
    );
    assert_eq!(
        e.cpu_xrstor_avx_resident(8, id, 99, 1, 7, 0),
        Err(HostError::InvalidArtifact)
    );
    assert_eq!(
        e.cpu_xsave_avx(6, 0x5000, 7, 0),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(e.arena(), before);
    assert_eq!(ram(&e), memory);
    e.arena_mut().unwrap()[YMM_OFFSET + 12] = 1;
    let before = e.arena().to_vec();
    assert_eq!(e.cpu_xsave_avx(3, 1, 7, 0), Err(HostError::Infrastructure));
    assert_eq!(e.cpu_xrstor_avx(3, 1, 7, 0), Err(HostError::Infrastructure));
    assert_eq!(e.arena(), before);
    assert_eq!(ram(&e), memory);
}

#[test]
fn pending_and_unauthorized_callback_authority_precede_all_new_guest_checks() {
    use crate::{
        cpu::x86::State32,
        process::{
            CallError,
            calls::{
                call::{PendingCall, PendingOwner},
                callback::{SuspendedCallback, SuspendedRecord},
            },
            cpu::context::ExtendedCpuState,
        },
        windows::{CallFrame32, CallingConvention32},
    };
    let (mut e, id) = engine();
    let state = State32 {
        registers: [0, 0, 0, 0, 0x5000, 0, 0, 0],
        ..State32::default()
    };
    let frame =
        CallFrame32::capture(e.memory().unwrap(), state, CallingConvention32::Cdecl, 0).unwrap();
    e.pending_call = Some(PendingCall {
        token: 1,
        owner: PendingOwner::Resident(id),
        frame,
        state: e.arena()[..56].try_into().unwrap(),
        exit: e.arena()[56..96].try_into().unwrap(),
        extended: ExtendedCpuState::capture(e.arena()).unwrap(),
    });
    fn blocked(e: &mut EngineInstance, id: u64) {
        let before = e.arena().to_vec();
        let memory = ram(e);
        let busy = HostError::Call(CallError::Busy);
        assert_eq!(e.cpu_xsave_avx(99, 1, 7, 0), Err(busy));
        assert_eq!(e.cpu_xrstor_avx(99, 1, 7, 0), Err(busy));
        assert_eq!(e.cpu_xsave_avx_resident(7, id, 99, 1, 7, 0), Err(busy));
        assert_eq!(e.cpu_xrstor_avx_resident(7, id, 99, 1, 7, 0), Err(busy));
        assert_eq!(e.arena(), before);
        assert_eq!(ram(e), memory);
    }
    blocked(&mut e, id);
    let outer = e.pending_call.take().unwrap();
    e.callback = Some(SuspendedCallback {
        outer,
        record: SuspendedRecord::Resident {
            record: crate::abi::resident_callback::ResidentCallbackRecord32 {
                token: 2,
                outer_token: 1,
                phase: 1,
                outcome: 0,
                entry_pc: 0x1000,
                entry_esp: 0x5000,
                return_pc: 0x1000,
                return_id: 7,
                stack_words: 0,
                result: 0,
                outer_unit_id: id,
                callback_unit_id: id,
            },
            authorized: false,
            active_unit_id: id,
        },
    });
    blocked(&mut e, id);
    e.callback = None;
    e.close();
    let before = e.arena().to_vec();
    assert_eq!(e.cpu_xsave_avx(99, 1, 7, 0), Err(HostError::Closed));
    assert_eq!(e.cpu_xrstor_avx(99, 1, 7, 0), Err(HostError::Closed));
    assert_eq!(e.arena(), before);
}
