use super::*;
use crate::{
    abi::{
        arena::{
            HELPER_OFFSET, SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET, SIMD_OFFSET,
            TRANSFER_OFFSET, WORK_CONSUMED_OFFSET, X87_OFFSET, YMM_OFFSET,
        },
        x86::{encode_exit_v7, encode_exit_v10, encode_state_v3},
    },
    cpu::x86::{DebugCause32, SelectorErrorCode32, State32, TrapPoint32},
    memory::{Access, FaultReason, GuestAddress},
};

const KEY: u64 = 0x44455445;

#[path = "tests/need_code.rs"]
mod need_code;

fn fixture(resident: bool) -> (EngineInstance, PendingOwner) {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x7000, 1, 3).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    let owner = if resident {
        PendingOwner::Resident(engine.compile_resident(1).unwrap().get())
    } else {
        engine.compile(1).unwrap();
        PendingOwner::Replacement(engine.generation())
    };
    // production root entry adapter supplies this same validated material authority before begin.
    match owner {
        PendingOwner::Replacement(generation) => engine.guard_material(KEY, generation).unwrap(),
        PendingOwner::Resident(id) => engine.guard_material_resident(KEY, id).unwrap(),
        _ => unreachable!(),
    }
    (engine, owner)
}
fn stop(engine: &mut EngineInstance, reason: ExitReason, pc: u32, retired: u32, work: u32) {
    let mut state = State32 {
        eip: pc,
        eflags: 0x240ed7,
        ..State32::default()
    };
    state.registers[1] = 1;
    state.registers[4] = 0x7800;
    state.registers[6] = 0x5001;
    state.registers[7] = 0x5101;
    let arena = engine.arena_mut().unwrap();
    encode_state_v3(&state, &mut arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]).unwrap();
    encode_exit_v7(
        &ExecutionExit { reason, retired },
        &mut arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    arena[WORK_CONSUMED_OFFSET..WORK_CONSUMED_OFFSET + 4].copy_from_slice(&work.to_le_bytes());
}
fn ud() -> DetectedEvent32 {
    DetectedEvent32::Exception(EventOrigin32::Exception(ExceptionCause32::InvalidOpcode))
}

#[test]
fn detector_origin_keeps_actual_continuation_partial_rep_and_f1_distinct() {
    for resident in [false, true] {
        for (cause, pc, retired, work) in [
            (
                DebugCause32::SingleStep(TrapPoint32::CompletedInstruction),
                0x1200,
                1,
                0,
            ),
            (
                DebugCause32::SingleStep(TrapPoint32::RepetitionInProgress),
                0x1000,
                0,
                1,
            ),
            (DebugCause32::Int1, 0x1001, 1, 0),
        ] {
            let (mut engine, owner) = fixture(resident);
            let epoch = engine.begin_exception_detection(owner).unwrap();
            stop(&mut engine, ExitReason::DebugTrap, pc, retired, work);
            let before = engine.arena().to_vec();
            let event = DetectedEvent32::Exception(EventOrigin32::Exception(
                ExceptionCause32::Debug(cause),
            ));
            let restart = if cause == DebugCause32::Int1 {
                Some(InstructionTrapRestart32 {
                    eip: 0x1000,
                    retired: 0,
                    flags: 0x250ed7,
                    ss_debug_shadow: true,
                    work_consumed: None,
                })
            } else {
                None
            };
            engine
                .finish_exception_detection(epoch, event, restart)
                .unwrap();
            assert_eq!(
                engine.arena(),
                before,
                "detection captures without CPU/frame/packet publication"
            );
            let record = engine.consume_exception_detection(epoch).unwrap();
            assert_eq!(
                (
                    record.epoch(),
                    record.owner(),
                    record.event(),
                    record.retired()
                ),
                (epoch, owner, event, retired)
            );
            assert_eq!(
                record.input(),
                FrameInput32 {
                    eip: pc,
                    esp: 0x7800,
                    flags: 0x240ed7,
                    cs: 0x1b
                }
            );
            assert_eq!(record.state(), &before[..STATE_SIZE]);
            assert_eq!(record.exit(), &before[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE]);
            assert_eq!(engine.arena(), before);
        }
    }
}

#[test]
fn sealed_detection_is_consumed_once_and_unfinished_non_event_lease_does_not_lock_reentry() {
    let (mut engine, owner) = fixture(false);
    let first = engine.begin_exception_detection(owner).unwrap();
    stop(&mut engine, ExitReason::Budget, 0x1000, 0, 0);
    // no terminal event import on Budget; the next valid entry explicitly replaces only Active.
    let next = engine.begin_exception_detection(owner).unwrap();
    assert_eq!(next, first + 1);
    stop(&mut engine, ExitReason::InvalidOpcode, 0x1000, 0, 0);
    assert_eq!(
        engine.finish_exception_detection(first, ud(), None),
        Err(HostError::Call(CallError::InvalidToken))
    );
    engine.finish_exception_detection(next, ud(), None).unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.finish_exception_detection(next, ud(), None),
        Err(HostError::Call(CallError::InvalidToken))
    );
    assert_eq!(
        engine.begin_exception_detection(owner),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.discard_exception_detection(first),
        Err(HostError::Call(CallError::InvalidToken))
    );
    assert!(engine.has_ready_exception_detection());
    let _record = engine.consume_exception_detection(next).unwrap();
    assert!(matches!(
        engine.consume_exception_detection(next),
        Err(HostError::Call(CallError::InvalidToken))
    ));
    assert_eq!(engine.arena(), before);
    let third = engine.begin_exception_detection(owner).unwrap();
    engine.discard_exception_detection(third).unwrap();
    assert_eq!(
        engine.discard_exception_detection(third),
        Err(HostError::Call(CallError::InvalidToken))
    );
}

#[test]
fn committed_source_smc_does_not_destroy_precise_partial_rep_detection() {
    for resident in [false, true] {
        let (mut engine, owner) = fixture(resident);
        let epoch = engine.begin_exception_detection(owner).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .cpu_write(GuestAddress(0x1000), &[0x7a])
            .unwrap();
        let current = match owner {
            PendingOwner::Replacement(generation) => engine.guard_material(KEY, generation),
            PendingOwner::Resident(id) => engine.guard_material_resident(KEY, id),
            _ => unreachable!(),
        };
        assert!(current.is_err(), "ordinary currentness guards stay strict");
        stop(&mut engine, ExitReason::DebugTrap, 0x1000, 0, 1);
        let event = DetectedEvent32::Exception(EventOrigin32::Exception(ExceptionCause32::Debug(
            DebugCause32::SingleStep(TrapPoint32::RepetitionInProgress),
        )));
        let before = engine.arena().to_vec();
        let version = engine.memory().unwrap().code_validation_version();
        engine
            .finish_exception_detection(epoch, event, None)
            .unwrap();
        let record = engine.consume_exception_detection(epoch).unwrap();
        assert_eq!(
            (record.input().eip, record.retired(), record.event()),
            (0x1000, 0, event)
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.memory().unwrap().code_validation_version(), version);
        let mut byte = [0];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x1000), &mut byte)
            .unwrap();
        assert_eq!(byte, [0x7a], "committed element/write is not rolled back");
    }
}

#[test]
fn raw_memory_fault_and_inconsistent_detector_never_acquire_verified_pf_authority() {
    let (mut engine, owner) = fixture(false);
    let epoch = engine.begin_exception_detection(owner).unwrap();
    let fault = MemoryFault {
        address: GuestAddress(0x6000),
        access: Access::Write,
        reason: FaultReason::Unmapped,
    };
    stop(
        &mut engine,
        ExitReason::MemoryFault { fault, length: 4 },
        0x1000,
        0,
        0,
    );
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.finish_exception_detection(epoch, ud(), None),
        Err(HostError::Call(CallError::InvalidStop))
    );
    let event = DetectedEvent32::MemoryFailure { fault, length: 4 };
    engine
        .finish_exception_detection(epoch, event, None)
        .unwrap();
    let record = engine.consume_exception_detection(epoch).unwrap();
    assert_eq!(record.event(), event);
    assert_eq!(engine.arena(), before);
}

#[test]
fn full_architectural_snapshot_and_work_are_bound_but_helper_scratch_is_not_context() {
    for offset in [
        16,
        EXIT_OFFSET + 20,
        STATE_OFFSET + 12,
        X87_OFFSET + 16,
        SIMD_OFFSET + 16,
        YMM_OFFSET + 16,
        SEGMENTS_OFFSET + 16,
        SEGMENT_BASES_RECORD_OFFSET + 28,
        WORK_CONSUMED_OFFSET,
    ] {
        let (mut engine, owner) = fixture(false);
        engine
            .set_segment_base(SegmentRegister::Ds, 0x4000)
            .unwrap();
        let epoch = engine.begin_exception_detection(owner).unwrap();
        stop(&mut engine, ExitReason::InvalidOpcode, 0x1000, 0, 0);
        engine
            .finish_exception_detection(epoch, ud(), None)
            .unwrap();
        let before = engine.arena().to_vec();
        engine.arena_mut().unwrap()[offset] ^= 1;
        assert!(matches!(
            engine.consume_exception_detection(epoch),
            Err(HostError::Call(CallError::StateChanged))
        ));
        assert!(
            engine.has_ready_exception_detection(),
            "rejection does not silently discard the event"
        );
        engine.arena_mut().unwrap().copy_from_slice(&before);
        engine.arena_mut().unwrap()[HELPER_OFFSET + 20] ^= 1;
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + 4] ^= 1;
        let _record = engine.consume_exception_detection(epoch).unwrap();
    }
}

#[test]
fn retained_owner_identity_is_required_after_smc_and_epochs_exhaust_before_mutation() {
    let (mut engine, owner) = fixture(true);
    let PendingOwner::Resident(id) = owner else {
        unreachable!()
    };
    let epoch = engine.begin_exception_detection(owner).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .cpu_write(GuestAddress(0x1000), &[0x90])
        .unwrap();
    stop(&mut engine, ExitReason::InvalidOpcode, 0x1000, 0, 0);
    engine
        .finish_exception_detection(epoch, ud(), None)
        .unwrap();
    // simulate owner disposal; no second registry or cloned CompiledRegion can outlive this ID.
    let memory = engine.memory.as_ref().unwrap();
    engine
        .resident
        .as_mut()
        .unwrap()
        .retire_stale_raw(memory, id)
        .unwrap();
    assert!(matches!(
        engine.consume_exception_detection(epoch),
        Err(HostError::Resident(_))
    ));
    engine.discard_exception_detection(epoch).unwrap();
    let (mut engine, owner) = fixture(false);
    let active = engine.begin_exception_detection(owner).unwrap();
    engine.detection_epoch = i64::MAX as u64;
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.begin_exception_detection(owner),
        Err(HostError::GenerationExhausted)
    );
    assert_eq!(engine.arena(), before);
    engine.discard_exception_detection(active).unwrap();
    engine.close();
    assert!(engine.exception_detection.is_none());
    assert_eq!(
        engine.begin_exception_detection(owner),
        Err(HostError::Closed)
    );
}

#[test]
fn nt_return_fault_keeps_null_selector_without_system_task_state() {
    let (mut engine, owner) = fixture(false);
    let epoch = engine.begin_exception_detection(owner).unwrap();
    let code = SelectorErrorCode32::from_bits(0).unwrap();
    let origin = EventOrigin32::Exception(ExceptionCause32::InvalidTss(code));
    assert_eq!((origin.vector(), origin.error_code()), (10, Some(0)));
    let mut state = State32 {
        eip: 0x1000,
        eflags: 2,
        ..State32::default()
    };
    state.registers[4] = 0x7800;
    let arena = engine.arena_mut().unwrap();
    encode_state_v3(&state, &mut arena[..STATE_SIZE]).unwrap();
    encode_exit_v10(
        &ExecutionExit {
            reason: ExitReason::InvalidTss { error_code: 0 },
            retired: 0,
        },
        &mut arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    engine
        .finish_exception_detection(epoch, DetectedEvent32::Exception(origin), None)
        .unwrap();
    assert_eq!(
        engine.consume_exception_detection(epoch).unwrap().event(),
        DetectedEvent32::Exception(origin)
    );
}

#[test]
fn instruction_trap_snapshot_keeps_continuation_and_precise_pre_instruction_restart() {
    for (cause, reason) in [
        (ExceptionCause32::BreakpointInt3, ExitReason::Breakpoint),
        (ExceptionCause32::OverflowInto, ExitReason::Overflow),
        (
            ExceptionCause32::Debug(DebugCause32::Int1),
            ExitReason::DebugTrap,
        ),
    ] {
        let (mut engine, owner) = fixture(false);
        let epoch = engine.begin_exception_detection(owner).unwrap();
        // supplied CPU-site provenance; this is not an encoding-validity or length-decoder test.
        stop(&mut engine, reason, 0x1003, 8, 12);
        let before = engine.arena().to_vec();
        let restart = InstructionTrapRestart32 {
            eip: 0x1000,
            retired: 7,
            flags: 0x250ed7,
            ss_debug_shadow: true,
            work_consumed: Some(11),
        };
        let event = DetectedEvent32::Exception(EventOrigin32::Exception(cause));
        assert_eq!(
            engine.finish_exception_detection(epoch, event, None),
            Err(HostError::Call(CallError::InvalidStop))
        );
        assert!(!engine.has_ready_exception_detection());
        engine
            .finish_exception_detection(epoch, event, Some(restart))
            .unwrap();
        let record = engine.consume_exception_detection(epoch).unwrap();
        assert_eq!(record.input().eip, 0x1003);
        assert_eq!(record.retired(), 8);
        assert_eq!(record.instruction_restart(), Some(restart));
        assert_eq!(
            (
                restart.retired,
                restart.work_consumed,
                restart.ss_debug_shadow
            ),
            (7, Some(11), true)
        );
        assert_eq!(
            engine.arena(),
            before,
            "record capture/consume does not publish rollback or frame effects"
        );
    }
}

#[test]
fn pending_call_and_callback_token_drift_do_not_consume_the_ready_application_event() {
    use crate::{
        abi::callback::CallbackRecord32,
        process::calls::{
            call::PendingCall,
            callback::{SuspendedCallback, SuspendedRecord},
        },
        windows::{CallFrame32, CallingConvention32},
    };

    let (mut engine, owner) = fixture(false);
    engine.write32(0x7800, 0x1100).unwrap();
    let epoch = engine.begin_exception_detection(owner).unwrap();
    stop(&mut engine, ExitReason::InvalidOpcode, 0x1000, 0, 0);
    engine
        .finish_exception_detection(epoch, ud(), None)
        .unwrap();
    let before = engine.arena().to_vec();
    let version = engine.memory().unwrap().code_validation_version();
    let frame = CallFrame32::capture(
        engine.memory().unwrap(),
        decode_state(&before[..STATE_SIZE]).unwrap(),
        CallingConvention32::Cdecl,
        0,
    )
    .unwrap();
    engine.pending_call = Some(PendingCall {
        token: 1,
        owner,
        frame,
        state: before[..STATE_SIZE].try_into().unwrap(),
        exit: before[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE]
            .try_into()
            .unwrap(),
        extended: ExtendedCpuState::capture(&before).unwrap(),
    });
    assert!(matches!(
        engine.consume_exception_detection(epoch),
        Err(HostError::Call(CallError::Busy))
    ));
    let outer = engine.pending_call.take().unwrap();
    engine.callback = Some(SuspendedCallback {
        record: SuspendedRecord::Replacement(CallbackRecord32 {
            token: 2,
            outer_token: 1,
            phase: 1,
            outcome: 0,
            entry_pc: 0x1200,
            entry_esp: 0x7800,
            return_pc: 0x1300,
            return_id: 18,
            stack_words: 0,
            result: 0,
            generation: engine.generation(),
        }),
        outer,
    });
    assert!(matches!(
        engine.consume_exception_detection(epoch),
        Err(HostError::Call(CallError::InvalidToken))
    ));
    assert!(engine.has_ready_exception_detection());
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().code_validation_version(), version);
    engine.callback = None;
    assert_eq!(
        engine.consume_exception_detection(epoch).unwrap().event(),
        ud()
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().code_validation_version(), version);
}

#[test]
fn application_page_fault_requires_the_actual_helper_occurrence_and_unchanged_cr2() {
    use crate::{abi::x86::encode_exit_v12, cpu::x86::PagingAccess32};

    for resident in [false, true] {
        let (mut engine, owner) = fixture(resident);
        let epoch = engine.begin_exception_detection(owner).unwrap();
        let fault =
            VerifiedPageFault32::application(GuestAddress(0x9005), PagingAccess32::Read, false);
        let event = DetectedEvent32::Exception(EventOrigin32::Exception(
            ExceptionCause32::PageFault(fault),
        ));
        stop(&mut engine, ExitReason::InvalidOpcode, 0x1000, 0, 0);
        let exit = ExecutionExit {
            retired: 0,
            reason: ExitReason::PageFault {
                error_code: 4,
                linear: fault.linear(),
                access: Access::Read,
                length: 4,
            },
        };
        encode_exit_v12(
            &exit,
            &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
        )
        .unwrap();
        assert_eq!(
            engine.finish_exception_detection(epoch, event, None),
            Err(HostError::Call(CallError::InvalidStop))
        );
        assert!(!engine.has_ready_exception_detection());
        engine.cpu_read32(0x9005).unwrap();
        assert_eq!(engine.cr2(), Some(fault.linear()));
        assert_eq!(
            engine.begin_exception_detection(owner),
            Err(HostError::Call(CallError::Busy))
        );
        engine
            .finish_exception_detection(epoch, event, None)
            .unwrap();
        let before = engine.arena().to_vec();
        engine.cr2 = Some(GuestAddress(0x9006));
        assert!(matches!(
            engine.consume_exception_detection(epoch),
            Err(HostError::Call(CallError::StateChanged))
        ));
        assert!(engine.has_ready_exception_detection());
        engine.cr2 = Some(fault.linear());
        assert_eq!(
            engine.consume_exception_detection(epoch).unwrap().event(),
            event
        );
        assert!(matches!(
            engine.consume_exception_detection(epoch),
            Err(HostError::Call(CallError::InvalidToken))
        ));
        assert_eq!(engine.arena(), before);
    }
}

#[test]
fn public_cpu_event_preserves_trap_origin_full_context_and_single_take() {
    let (mut engine, owner) = fixture(false);
    assert!(engine.take_cpu_event32().unwrap().is_none());
    let epoch = engine.begin_exception_detection(owner).unwrap();
    stop(&mut engine, ExitReason::Breakpoint, 0x1003, 8, 12);
    let before = engine.arena().to_vec();
    let restart = InstructionTrapRestart32 {
        eip: 0x1000,
        retired: 7,
        flags: 0x250ed7,
        ss_debug_shadow: true,
        work_consumed: Some(11),
    };
    engine
        .finish_exception_detection(
            epoch,
            DetectedEvent32::Exception(EventOrigin32::Exception(ExceptionCause32::BreakpointInt3)),
            Some(restart),
        )
        .unwrap();
    engine.cpu_event = Some(
        engine
            .consume_exception_detection(epoch)
            .unwrap()
            .into_event(),
    );
    assert!(
        engine
            .cpu_event
            .as_ref()
            .unwrap()
            .matches_context(engine.arena())
    );
    engine.arena_mut().unwrap()[crate::abi::x86::EIP_OFFSET] ^= 1;
    assert!(
        !engine
            .cpu_event
            .as_ref()
            .unwrap()
            .matches_context(engine.arena())
    );
    engine.arena_mut().unwrap().copy_from_slice(&before);
    let event = engine.take_cpu_event32().unwrap().unwrap();
    assert!(engine.take_cpu_event32().unwrap().is_none());
    assert_eq!(event.epoch, epoch);
    assert_eq!(event.state.eip, 0x1003);
    assert_eq!(
        event.exit,
        ExecutionExit {
            reason: ExitReason::Breakpoint,
            retired: 8
        }
    );
    assert_eq!(event.instruction.unwrap().eip, 0x1000);
    assert_eq!(event.instruction.unwrap().flags, 0x250ed7);
    assert_eq!(event.instruction.unwrap().work_consumed, Some(11));
    assert_eq!(event.work_consumed, 12);
    assert_eq!(
        event.x87,
        crate::abi::x86::decode_x87(&before[X87_OFFSET..X87_OFFSET + crate::abi::x86::X87_SIZE])
            .unwrap()
    );
    assert_eq!(
        event.simd,
        crate::abi::x86::decode_simd(
            &before[SIMD_OFFSET..SIMD_OFFSET + crate::abi::x86::SIMD_SIZE]
        )
        .unwrap()
    );
    assert_eq!(
        event.ymm,
        crate::abi::x86::decode_ymm(&before[YMM_OFFSET..YMM_OFFSET + crate::abi::x86::YMM_SIZE])
            .unwrap()
    );
    assert_eq!(event.segments, engine.segment_context().unwrap());
    let mut packet = [0; crate::abi::x86::CPU_EVENT_SITE_SIZE];
    crate::abi::x86::encode_cpu_event_site32(&event, &mut packet).unwrap();
    assert_eq!(&packet[..4], b"R3CE");
    assert_eq!(
        (0..8)
            .map(|i| header::read_u32(&packet, 16 + i * 4))
            .collect::<Vec<_>>(),
        [3, 0x1000, 0x1003, 7, 0x250ed7, 1, 11, 8]
    );
    let mut short = [0xa5; 47];
    assert_eq!(
        crate::abi::x86::encode_cpu_event_site32(&event, &mut short),
        Err(crate::abi::AbiError::Length)
    );
    assert_eq!(short, [0xa5; 47]);
    assert_eq!(engine.arena(), before);
    engine.close();
    assert!(matches!(engine.take_cpu_event32(), Err(HostError::Closed)));
}

#[test]
fn event_lifetime_survives_failed_entry_and_expires_on_valid_entry() {
    let (mut engine, owner) = fixture(false);
    let epoch = engine.begin_exception_detection(owner).unwrap();
    stop(&mut engine, ExitReason::InvalidOpcode, 0x1000, 0, 0);
    engine
        .finish_exception_detection(epoch, ud(), None)
        .unwrap();
    engine.cpu_event = Some(
        engine
            .consume_exception_detection(epoch)
            .unwrap()
            .into_event(),
    );
    assert!(
        engine
            .begin_exception_detection(PendingOwner::Replacement(engine.generation() + 1))
            .is_err()
    );
    assert_eq!(engine.cpu_event.as_ref().unwrap().epoch, epoch);
    engine.detection_epoch = i64::MAX as u64;
    assert!(matches!(
        engine.begin_exception_detection(owner),
        Err(HostError::GenerationExhausted)
    ));
    assert_eq!(engine.cpu_event.as_ref().unwrap().epoch, epoch);
    engine.detection_epoch = epoch;
    engine.begin_exception_detection(owner).unwrap();
    assert!(engine.take_cpu_event32().unwrap().is_none());
}
