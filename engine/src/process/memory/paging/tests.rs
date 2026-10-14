use super::*;
use crate::{
    abi::{
        arena::{HELPER_OFFSET, TRANSFER_OFFSET},
        memory_helper::HELPER_SIZE,
    },
    cpu::x86::{PagingAccess32, PagingDomain32, VerifiedPageFault32},
    memory::{Access, FaultReason, MemoryFault},
    process::{HostError, calls::call::PendingOwner},
};

const KEY: u64 = 0x50414745;
fn fixture() -> EngineInstance {
    let mut engine = EngineInstance::new(5, KEY).unwrap();

    for page in [0x1000, 0x2000, 0x7000] {
        engine.map(page, 1, 7).unwrap();
    }
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
        .copy_from_slice(&0x13579bdf_u32.to_le_bytes());
    engine.upload(0x2000, 4).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    engine
}
fn pf(error: CpuPagingError32) -> VerifiedPageFault32 {
    let CpuPagingError32::PageFault(fault) = error else {
        panic!("expected application PF, got {error:?}")
    };
    fault
}

fn stage_pf_exit(engine: &mut EngineInstance, fault: VerifiedPageFault32) {
    use crate::abi::{
        arena::{EXIT_OFFSET, STATE_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, encode_exit_v12, encode_state_v3},
    };
    use crate::cpu::{ExecutionExit, ExitReason, x86::State32};
    let mut state = State32 {
        eip: 0x1000,
        ..Default::default()
    };
    state.registers[4] = 0x7800;
    let arena = engine.arena_mut().unwrap();
    encode_state_v3(&state, &mut arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]).unwrap();
    encode_exit_v12(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::PageFault {
                error_code: fault.error_code(),
                linear: fault.linear(),
                access: Access::Read,
                length: 4,
            },
        },
        &mut arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
}

#[test]
fn actual_certified_pf_is_required_to_seal_and_consumes_once() {
    use crate::{
        cpu::x86::{EventOrigin32, ExceptionCause32},
        process::{CallError, cpu::events::detection::DetectedEvent32},
    };
    let mut engine = fixture();

    let epoch = engine
        .begin_exception_detection(PendingOwner::Replacement(engine.generation()))
        .unwrap();
    let fault = VerifiedPageFault32::application(GuestAddress(0x3005), PagingAccess32::Read, false);
    let event =
        DetectedEvent32::Exception(EventOrigin32::Exception(ExceptionCause32::PageFault(fault)));
    stage_pf_exit(&mut engine, fault);
    assert_eq!(
        engine.finish_exception_detection(epoch, event, None),
        Err(HostError::Call(CallError::InvalidStop))
    );
    assert_eq!(engine.cr2(), None);
    engine.cpu_read32(0x3005).unwrap();
    assert_eq!(engine.certified_page_fault32(epoch), Some(fault));
    assert_eq!(engine.cr2(), Some(GuestAddress(0x3005)));
    assert_eq!(
        engine.begin_exception_detection(PendingOwner::Replacement(engine.generation())),
        Err(HostError::Call(CallError::Busy))
    );
    engine
        .finish_exception_detection(epoch, event, None)
        .unwrap();
    let record = engine.consume_exception_detection(epoch).unwrap();
    assert_eq!(record.event(), event);
    assert!(matches!(
        engine.consume_exception_detection(epoch),
        Err(HostError::Call(CallError::InvalidToken))
    ));
}

#[test]
fn changed_cr2_or_a_later_valid_access_cannot_replay_an_old_pf() {
    use crate::{
        cpu::x86::{EventOrigin32, ExceptionCause32},
        process::{CallError, cpu::events::detection::DetectedEvent32},
    };
    for seal in [false, true] {
        let mut engine = fixture();

        let epoch = engine
            .begin_exception_detection(PendingOwner::Replacement(engine.generation()))
            .unwrap();
        engine.cpu_read32(0x3005).unwrap();
        let fault = engine.certified_page_fault32(epoch).unwrap();
        stage_pf_exit(&mut engine, fault);
        let event = DetectedEvent32::Exception(EventOrigin32::Exception(
            ExceptionCause32::PageFault(fault),
        ));
        if seal {
            engine
                .finish_exception_detection(epoch, event, None)
                .unwrap();
            assert!(
                engine
                    .preflight_cpu_paging32(GuestAddress(0x3009), 4, PagingAccess32::Read)
                    .is_err()
            );
            assert!(matches!(
                engine.consume_exception_detection(epoch),
                Err(HostError::Call(CallError::StateChanged))
            ));
        } else {
            engine.cpu_read32(0x2000).unwrap();
            assert_eq!(engine.cr2(), Some(GuestAddress(0x3005)));
            assert_eq!(engine.certified_page_fault32(epoch), None);
            assert_eq!(
                engine.finish_exception_detection(epoch, event, None),
                Err(HostError::Call(CallError::InvalidStop))
            );
        }
    }
}
fn data(engine: &EngineInstance, page: u32) -> Vec<u8> {
    let mut bytes = vec![0; 4096];
    engine
        .retained_memory()
        .unwrap()
        .read(GuestAddress(page), &mut bytes)
        .unwrap();
    bytes
}
fn unchanged_outside_helper(engine: &EngineInstance, before: &[u8]) {
    assert_eq!(&engine.arena()[..HELPER_OFFSET], &before[..HELPER_OFFSET]);
    assert_eq!(
        &engine.arena()[HELPER_OFFSET + HELPER_SIZE..],
        &before[HELPER_OFFSET + HELPER_SIZE..]
    );
}

#[test]
fn actual_access_faults_update_cr2_while_success_and_alignment_faults_do_not() {
    let mut engine = fixture();
    assert_eq!(engine.cr2(), None);
    let fault = pf(engine
        .preflight_cpu_paging32(GuestAddress(0x3007), 4, PagingAccess32::Read)
        .unwrap_err());
    assert_eq!(
        (fault.linear(), fault.error_code()),
        (GuestAddress(0x3007), 4)
    );
    engine.cpu_read32(0x2000).unwrap();
    assert_eq!(engine.cr2(), Some(GuestAddress(0x3007)));
    engine.cpu_read(0x2001, 16, 16).unwrap();
    assert_eq!(engine.cr2(), Some(GuestAddress(0x3007)));
    engine.protect(0x2000, 1, 4).unwrap();
    let before = engine.arena().to_vec();
    engine.cpu_read32(0x2000).unwrap();
    unchanged_outside_helper(&engine, &before);
    assert_eq!(engine.cr2(), Some(GuestAddress(0x2000)));
    engine.close();
    assert_eq!(engine.cr2(), None);
}

#[test]
fn real_scalar_rmw_read_fault_is_write_and_seals_only_the_actual_active_witness() {
    let mut engine = fixture();
    engine.protect(0x2000, 1, 1).unwrap();
    let owner = PendingOwner::Replacement(engine.generation());
    let epoch = engine.begin_exception_detection(owner).unwrap();
    let before = engine.arena().to_vec();
    let ram = data(&engine, 0x2000);
    engine.cpu_read32(0x2000).unwrap();
    assert_eq!(engine.certified_page_fault32(epoch), None);
    assert_eq!(engine.cr2(), None);
    engine.cpu_read_modify32(0x2000).unwrap();
    let fault = engine.certified_page_fault32(epoch).unwrap();
    assert_eq!(
        (
            fault.linear(),
            fault.access(),
            fault.domain(),
            fault.error_code()
        ),
        (
            GuestAddress(0x2000),
            PagingAccess32::Write,
            PagingDomain32::User,
            7
        )
    );
    assert_eq!(engine.cr2(), Some(GuestAddress(0x2000)));
    unchanged_outside_helper(&engine, &before);
    assert_eq!(data(&engine, 0x2000), ram);
    engine.cpu_read32(0x2000).unwrap();
    assert_eq!(
        engine.certified_page_fault32(epoch),
        None,
        "a new actual access clears an old helper witness"
    );
    assert_eq!(
        engine.cr2(),
        Some(GuestAddress(0x2000)),
        "clearing a witness never rolls back cr2"
    );
}

#[test]
fn real_span_and_wrapping_write_faults_preflight_before_any_ram_or_version_effect() {
    let mut engine = fixture();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 16].fill(0xa5);
    let before = engine.arena().to_vec();
    let ram = data(&engine, 0x2000);
    let version = engine.memory().unwrap().code_validation_version();
    engine.cpu_write(0x2ffc, 8, 1).unwrap();
    unchanged_outside_helper(&engine, &before);
    assert_eq!(data(&engine, 0x2000), ram);
    assert_eq!(engine.memory().unwrap().code_validation_version(), version);
    assert_eq!(
        engine.cr2(),
        Some(GuestAddress(0x3000)),
        "unmapped application memory faults without extra configuration"
    );

    engine.map(0xfffff000, 1, 3).unwrap();

    let high = data(&engine, 0xfffff000);
    let version = engine.memory().unwrap().code_validation_version();
    engine.cpu_store32(0xfffffffe, 0x12345678).unwrap();
    assert_eq!(engine.cr2(), Some(GuestAddress(0)));
    assert_eq!(data(&engine, 0xfffff000), high);
    assert_eq!(engine.memory().unwrap().code_validation_version(), version);
}

#[test]
fn masked_disabled_bytes_skip_access_checks_and_fxsave_checks_its_full_span() {
    let mut engine = fixture();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 512].fill(0xa5);
    let version = engine.memory().unwrap().code_validation_version();
    engine.cpu_mask_write(0x2ffc, 8, 0).unwrap();
    assert_eq!(engine.memory().unwrap().code_validation_version(), version);
    assert_eq!(engine.cr2(), None);
    engine.cpu_mask_write(0x2ffc, 8, 0x0f).unwrap();
    let prefix = data(&engine, 0x2000);
    assert_eq!(&prefix[0xffc..], &[0xa5; 4]);
    let version = engine.memory().unwrap().code_validation_version();
    engine.cpu_mask_write(0x2ffc, 8, 0x80).unwrap();
    assert_eq!(data(&engine, 0x2000), prefix);
    assert_eq!(engine.memory().unwrap().code_validation_version(), version);
    assert_eq!(engine.cr2(), Some(GuestAddress(0x3003)));
    let version = engine.memory().unwrap().code_validation_version();
    engine.cpu_fxsave(0x2e10).unwrap();
    assert_eq!(engine.cr2(), Some(GuestAddress(0x3000)));
    assert_eq!(data(&engine, 0x2000), prefix);
    assert_eq!(engine.memory().unwrap().code_validation_version(), version);
}

#[test]
fn host_diagnostics_remain_memory_errors_without_recording_cpu_fault_occurrences() {
    let engine = fixture();
    let mut output = [0xa5; 4];
    assert_eq!(
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x3000), &mut output),
        Err(MemoryError::Fault(MemoryFault {
            address: GuestAddress(0x3000),
            access: Access::Read,
            reason: FaultReason::Unmapped,
        }))
    );
    assert_eq!(output, [0xa5; 4]);
    assert_eq!(engine.cr2(), None);
    assert_eq!(engine.cpu_page_fault_sequence32(), 0);
    assert_eq!(engine.memory().unwrap().capacity_pages(), 5);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 3);
}

#[test]
fn cpu_pf_sequence_wraps_for_identical_direct_faults_and_ignores_success_and_owner_errors() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();

    engine.map(0x2000, 1, 3).unwrap();

    engine.cpu_page_fault_sequence = u64::MAX - 1;
    for sequence in [u64::MAX, 0] {
        engine.cpu_read32(0x3005).unwrap();
        assert_eq!(engine.cpu_page_fault_sequence32(), sequence);
        assert_eq!(engine.cr2(), Some(GuestAddress(0x3005)));
    }
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.cpu_store_resident32(KEY + 1, 1, 0x2000, 0x12345678),
        Err(HostError::InvalidArtifact)
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.cpu_page_fault_sequence32(), 0);
    assert_eq!(engine.cr2(), Some(GuestAddress(0x3005)));
    engine.cpu_read32(0x2000).unwrap();
    assert_eq!(engine.cpu_page_fault_sequence32(), 0);
    assert_eq!(engine.cr2(), Some(GuestAddress(0x3005)));
}

#[test]
fn old_pf_does_not_turn_successful_zero_mask_gather_into_a_new_pf_occurrence() {
    let mut engine = fixture();
    let request = TRANSFER_OFFSET + 544;
    let arena = engine.arena_mut().unwrap();
    arena[request..request + 24].fill(0);
    arena[request..request + 12]
        .copy_from_slice(&[b'R', b'3', b'G', b'A', 1, 0, 1, 0, 24, 0, 0, 0]);
    arena[request + 16..request + 20].copy_from_slice(&3_u32.to_le_bytes());

    let epoch = engine
        .begin_exception_detection(PendingOwner::Replacement(engine.generation()))
        .unwrap();
    engine.cpu_read32(0x3005).unwrap();
    assert!(engine.certified_page_fault32(epoch).is_some());
    let before = engine.cpu_page_fault_sequence32();
    let before_fault = engine.certified_page_fault32(epoch);
    let before_ram = [0x1000, 0x2000, 0x7000].map(|page| data(&engine, page));
    let version = engine.memory().unwrap().code_validation_version();
    // default simd mask xmm2 is zero; the real gather performs no memory read.
    assert_eq!(
        engine.avx_execute(0x01001401, 0x1420, 0, 0, 0, 2),
        Ok(crate::process::StoreCompletion::Complete)
    );
    assert_eq!(engine.cpu_page_fault_sequence32(), before);
    assert_eq!(engine.certified_page_fault32(epoch), before_fault);
    assert_eq!(
        [0x1000, 0x2000, 0x7000].map(|page| data(&engine, page)),
        before_ram
    );
    assert_eq!(engine.memory().unwrap().code_validation_version(), version);
    assert_eq!(engine.cr2(), Some(GuestAddress(0x3005)));
    assert_eq!(&engine.arena()[HELPER_OFFSET..HELPER_OFFSET + 4], b"R3GH");
    assert_eq!(
        u16::from_le_bytes(engine.arena()[104..106].try_into().unwrap()),
        1
    );
    assert_eq!(
        u32::from_le_bytes(engine.arena()[116..120].try_into().unwrap()),
        0
    );
}

#[test]
fn material_replacement_preserves_active_and_ready_certified_pf_until_single_consume() {
    use crate::{
        cpu::x86::{EventOrigin32, ExceptionCause32},
        process::{CallError, cpu::events::detection::DetectedEvent32},
    };
    for ready in [false, true] {
        let mut engine = fixture();

        let owner = PendingOwner::Replacement(engine.generation());
        let epoch = engine.begin_exception_detection(owner).unwrap();
        engine.cpu_read32(0x3005).unwrap();
        let fault = engine.certified_page_fault32(epoch).unwrap();
        stage_pf_exit(&mut engine, fault);
        let event = DetectedEvent32::Exception(EventOrigin32::Exception(
            ExceptionCause32::PageFault(fault),
        ));
        if ready {
            engine
                .finish_exception_detection(epoch, event, None)
                .unwrap();
        }
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
            .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
        let before = engine.arena().to_vec();
        let source = data(&engine, 0x1000);
        let version = engine.memory().unwrap().code_validation_version();
        let generation = engine.generation();
        assert_eq!(
            engine.compile(1),
            Err(HostError::Call(CallError::Busy)),
            "replacement cannot invalidate an active occurrence or sealed owner"
        );
        assert_eq!(engine.generation(), generation);
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.memory().unwrap().code_validation_version(), version);
        assert_eq!(data(&engine, 0x1000), source);
        assert_eq!(engine.cr2(), Some(GuestAddress(0x3005)));
        if !ready {
            engine
                .finish_exception_detection(epoch, event, None)
                .unwrap();
        }
        let consumed = engine.consume_exception_detection(epoch).unwrap();
        assert_eq!(consumed.event(), event);
        assert!(matches!(
            engine.consume_exception_detection(epoch),
            Err(HostError::Call(CallError::InvalidToken))
        ));
    }
}

#[test]
fn engine_write_failures_do_not_mint_application_fault_occurrences() {
    let mut engine = fixture();
    engine.cpu_read32(0x3005).unwrap();
    let sequence = engine.cpu_page_fault_sequence32();
    let before_ram = data(&engine, 0x2000);
    engine.memory.as_mut().unwrap().exhaust_versions_for_test();
    let before = engine.arena().to_vec();
    engine.cpu_store32(0x2000, 0xaabbccdd).unwrap();
    unchanged_outside_helper(&engine, &before);
    assert_eq!(data(&engine, 0x2000), before_ram);
    assert_eq!(engine.cr2(), Some(GuestAddress(0x3005)));
    assert_eq!(engine.cpu_page_fault_sequence32(), sequence);
    assert_eq!(
        u16::from_le_bytes(
            engine.arena()[HELPER_OFFSET + 4..HELPER_OFFSET + 6]
                .try_into()
                .unwrap()
        ),
        10
    );
    assert_eq!(
        u32::from_le_bytes(
            engine.arena()[HELPER_OFFSET + 16..HELPER_OFFSET + 20]
                .try_into()
                .unwrap()
        ),
        2
    );
}
