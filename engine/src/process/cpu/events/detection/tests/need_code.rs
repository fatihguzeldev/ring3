use super::*;
use crate::{cpu::InfrastructureFailure, cpu::x86::PagingAccess32};

fn observed_fault(engine: &mut EngineInstance) {
    assert!(matches!(
        engine.preflight_cpu_paging32(GuestAddress(0x6000), 4, PagingAccess32::Read),
        Err(crate::cpu::x86::CpuPagingError32::PageFault(fault))
            if fault.linear() == GuestAddress(0x6000) && fault.error_code() == 4
    ));
}

#[test]
fn verified_need_code_retires_only_the_attempt_and_preserves_observed_fault_metadata() {
    for resident in [false, true] {
        let (mut engine, owner) = fixture(resident);
        let epoch = engine.begin_exception_detection(owner).unwrap();
        observed_fault(&mut engine);
        stop(&mut engine, ExitReason::NeedCode, 0x1000, 2, 3);
        let before = engine.arena().to_vec();
        let occurrence = engine.cpu_page_fault_sequence32();
        let version = engine.memory().unwrap().code_validation_version();
        let mut ram = [0; 4096];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x7000), &mut ram)
            .unwrap();
        assert!(engine.has_retained_detection());
        assert_eq!(
            engine.begin_exception_detection(owner),
            Err(HostError::Call(CallError::Busy))
        );

        engine
            .finish_generated_detection(epoch, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
            .unwrap();
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.cr2(), Some(GuestAddress(0x6000)));
        assert_eq!(engine.cpu_page_fault_sequence32(), occurrence);
        assert_eq!(engine.memory().unwrap().code_validation_version(), version);
        let mut after = [0; 4096];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x7000), &mut after)
            .unwrap();
        assert_eq!(after, ram);
        assert!(!engine.has_retained_detection());
        assert!(engine.cpu_event.is_none());
        assert_eq!(
            engine.finish_generated_detection(epoch, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            Err(HostError::Call(CallError::InvalidToken))
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.begin_exception_detection(owner), Ok(epoch + 1));
        assert!(engine.validated_extended_state.is_some());
        engine.close();
        assert!(engine.validated_extended_state.is_none());
    }
}

#[test]
fn need_code_cleanup_refuses_nonzero_fields_bad_context_wrong_epoch_and_downgraded_faults() {
    let (mut engine, owner) = fixture(false);
    let warm = engine.begin_exception_detection(owner).unwrap();
    stop(&mut engine, ExitReason::NeedCode, 0x1000, 0, 0);
    engine
        .finish_generated_detection(warm, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        .unwrap();
    assert!(engine.validated_extended_state.is_some());
    let epoch = engine.begin_exception_detection(owner).unwrap();
    observed_fault(&mut engine);
    stop(&mut engine, ExitReason::NeedCode, 0x1000, 0, 0);
    let valid = engine.arena().to_vec();
    let occurrence = engine.cpu_page_fault_sequence32();
    for field in 1..11 {
        let mut fields = [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        fields[field] = 1;
        assert_eq!(
            engine.finish_generated_detection(epoch, fields),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), valid);
        assert!(engine.has_retained_detection());
    }
    assert_eq!(
        engine.finish_generated_detection(epoch + 1, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        Err(HostError::Call(CallError::InvalidToken))
    );
    for offset in [
        0,
        X87_OFFSET,
        SIMD_OFFSET,
        YMM_OFFSET,
        SEGMENTS_OFFSET,
        WORK_OFFSET,
        WORK_OFFSET + 20,
        EXIT_OFFSET + 24,
    ] {
        engine.arena_mut().unwrap()[offset] ^= 1;
        let changed = engine.arena().to_vec();
        assert_eq!(
            engine.finish_generated_detection(epoch, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            Err(HostError::Call(CallError::InvalidStop))
        );
        assert_eq!(engine.arena(), changed);
        assert!(engine.has_retained_detection());
        engine.arena_mut().unwrap().copy_from_slice(&valid);
    }
    stop(
        &mut engine,
        ExitReason::Infrastructure(InfrastructureFailure::HelperProtocol),
        0x1000,
        0,
        0,
    );
    let downgraded = engine.arena().to_vec();
    assert_eq!(
        engine.finish_generated_detection(epoch, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        Err(HostError::Call(CallError::InvalidStop))
    );
    assert_eq!(engine.arena(), downgraded);
    assert_eq!(engine.cr2(), Some(GuestAddress(0x6000)));
    assert_eq!(engine.cpu_page_fault_sequence32(), occurrence);
    assert!(engine.has_retained_detection());
}

#[test]
fn need_code_cleanup_cannot_consume_ready_events_or_disposed_owners() {
    let (mut engine, owner) = fixture(false);
    let epoch = engine.begin_exception_detection(owner).unwrap();
    stop(&mut engine, ExitReason::InvalidOpcode, 0x1000, 0, 0);
    engine
        .finish_exception_detection(epoch, ud(), None)
        .unwrap();
    let ready = engine.arena().to_vec();
    assert_eq!(
        engine.finish_generated_detection(epoch, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        Err(HostError::Call(CallError::InvalidToken))
    );
    assert_eq!(engine.arena(), ready);
    assert!(engine.has_ready_exception_detection());
    assert_eq!(
        engine.consume_exception_detection(epoch).unwrap().event(),
        ud()
    );

    let epoch = engine.begin_exception_detection(owner).unwrap();
    stop(&mut engine, ExitReason::NeedCode, 0x1000, 0, 0);
    engine.generation += 1;
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.finish_generated_detection(epoch, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        Err(HostError::InvalidArtifact)
    );
    assert_eq!(engine.arena(), before);
}
