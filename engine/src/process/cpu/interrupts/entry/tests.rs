use super::*;
use crate::{
    abi::{
        arena::{
            EXIT_OFFSET, HELPER_OFFSET, SEGMENTS_OFFSET, STATE_OFFSET, TRANSFER_OFFSET,
            WORK_CONSUMED_OFFSET,
        },
        memory_helper::HELPER_SIZE,
        x86::{EXIT_SIZE, STATE_SIZE, encode_exit_v10, encode_state_v3},
    },
    cpu::{
        ExecutionExit, ExitReason,
        x86::{
            DebugCause32, EnteredEvent32, EventAuthorityKind32, ExceptionClass32,
            InterruptDescriptorTable32, SegmentDescriptorRecord, SelectorErrorCode32, State32,
            TrapPoint32,
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryError, MemoryFault},
    process::{
        CallError, calls::call::PendingOwner, cpu::interrupts::detection::InstructionTrapRestart32,
    },
};

const KEY: u64 = 0x524f5554;
fn gate(kind: u8, dpl: u8, present: bool) -> SegmentDescriptorRecord {
    SegmentDescriptorRecord::from_words(
        0x00402345,
        (u32::from(kind) << 8) | (u32::from(dpl) << 13) | if present { 0x8000 } else { 0 },
    )
}
fn fixture(
    resident: bool,
    overrides: &[(usize, SegmentDescriptorRecord)],
) -> (EngineInstance, PendingOwner) {
    let mut engine = EngineInstance::new(4, KEY).unwrap();
    let mut records = Box::new([SegmentDescriptorRecord::from_words(0, 0); 256]);
    for vector in [0, 1, 3, 4, 6, 8, 10, 11, 12, 13] {
        records[vector] = gate(15, 3, true);
    }
    for &(vector, record) in overrides {
        records[vector] = record;
    }
    engine
        .configure_interrupt_descriptor_table(InterruptDescriptorTable32::new(0xffff, records))
        .unwrap();
    engine
        .configure_segment_descriptor_raw(
            0x40,
            SegmentDescriptorRecord::from_words(0xffff, 0x00cffa00),
        )
        .unwrap();
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
    match owner {
        PendingOwner::Replacement(generation) => engine.guard_material(KEY, generation).unwrap(),
        PendingOwner::Resident(id) => engine.guard_material_resident(KEY, id).unwrap(),
        _ => unreachable!(),
    }
    (engine, owner)
}
#[allow(clippy::too_many_arguments)]
fn seal(
    engine: &mut EngineInstance,
    owner: PendingOwner,
    event: DetectedEvent32,
    reason: ExitReason,
    pc: u32,
    retired: u32,
    esp: u32,
    work: u32,
    restart: Option<InstructionTrapRestart32>,
) -> u64 {
    let epoch = engine.begin_exception_detection(owner).unwrap();
    let mut state = State32 {
        registers: [
            0x11223344, 1, 0x55667788, 0x99aabbcc, esp, 0x12345678, 0x5001, 0x1001,
        ],
        eip: pc,
        eflags: 0x302,
    };
    // these are trusted detector output images, not a claim of guest execution in native tests.
    if let DetectedEvent32::NeedsPagingAuthority { .. } = event {
        state.eflags = 0x202;
    }
    let arena = engine.arena_mut().unwrap();
    encode_state_v3(&state, &mut arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]).unwrap();
    encode_exit_v10(
        &ExecutionExit { reason, retired },
        &mut arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    arena[WORK_CONSUMED_OFFSET..WORK_CONSUMED_OFFSET + 4].copy_from_slice(&work.to_le_bytes());
    engine
        .finish_exception_detection(epoch, event, restart)
        .unwrap();
    epoch
}
fn event(cause: ExceptionCause32) -> DetectedEvent32 {
    DetectedEvent32::Exception(EventOrigin32::Exception(cause))
}
fn bytes(engine: &EngineInstance, page: u32) -> Vec<u8> {
    let mut output = vec![0; 4096];
    engine
        .retained_memory()
        .unwrap()
        .read(GuestAddress(page), &mut output)
        .unwrap();
    output
}
fn packet(fields: [u32; 6]) -> [u8; HELPER_SIZE] {
    let mut output = [0; HELPER_SIZE];
    output[..16].copy_from_slice(b"R3MH\x0c\0\x01\0\x28\0\0\0\0\0\0\0");
    for (index, value) in fields.into_iter().enumerate() {
        output[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    output
}
fn expect_entered(
    before: &[u8],
    target: u32,
    esp: u32,
    flags: u32,
    retired: u32,
    work: u32,
    info: u32,
) -> Vec<u8> {
    let mut want = before.to_vec();
    for (offset, value) in [(32, esp), (48, target), (52, flags)] {
        want[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    want[12..16].fill(0);
    want[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE].copy_from_slice(
        b"R3EX\x0a\0\x01\0\x28\0\0\0\0\0\0\0\x03\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
    );
    want[EXIT_OFFSET + 20..EXIT_OFFSET + 24].copy_from_slice(&retired.to_le_bytes());
    want[WORK_CONSUMED_OFFSET..WORK_CONSUMED_OFFSET + 4].copy_from_slice(&work.to_le_bytes());
    want[SEGMENTS_OFFSET + 18..SEGMENTS_OFFSET + 20].copy_from_slice(&0x43_u16.to_le_bytes());
    want[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]
        .copy_from_slice(&packet([0, target, 0, 0, 0, info]));
    want
}
fn entered(
    initial: EventOrigin32,
    origin: EventOrigin32,
    esp: u32,
    flags: u32,
    info: u32,
    stale: bool,
) -> DetectedEntryResult32 {
    DetectedEntryResult32::Entered {
        initial,
        origin,
        entered: EnteredEvent32 {
            target: 0x2345,
            esp,
            flags,
            frame_info: info,
            code_invalidated: stale,
        },
    }
}
fn restart() -> InstructionTrapRestart32 {
    InstructionTrapRestart32 {
        eip: 0x1000,
        retired: 7,
        flags: 0x10302,
        ss_debug_shadow: true,
        work_consumed: Some(4),
    }
}

#[test]
fn detected_fault_tf_and_unfinished_rep_keep_distinct_eip_retirement_rf_and_full_state() {
    for resident in [false, true] {
        for (cause, reason, pc, retired, work, saved_flags) in [
            (
                ExceptionCause32::InvalidOpcode,
                ExitReason::InvalidOpcode,
                0x1000,
                0,
                0,
                0x10302,
            ),
            (
                ExceptionCause32::Debug(DebugCause32::SingleStep(
                    TrapPoint32::CompletedInstruction,
                )),
                ExitReason::DebugTrap,
                0x1200,
                1,
                0,
                0x302,
            ),
            (
                ExceptionCause32::Debug(DebugCause32::SingleStep(
                    TrapPoint32::RepetitionInProgress,
                )),
                ExitReason::DebugTrap,
                0x1000,
                0,
                1,
                0x10302,
            ),
        ] {
            let (mut engine, owner) = fixture(resident, &[]);
            let epoch = seal(
                &mut engine,
                owner,
                event(cause),
                reason,
                pc,
                retired,
                0x7800,
                work,
                None,
            );
            let before = engine.arena().to_vec();
            let code = bytes(&engine, 0x1000);
            let mut stack = bytes(&engine, 0x7000);
            for (offset, value) in [(0x7f4, pc), (0x7f8, 0x1b), (0x7fc, saved_flags)] {
                stack[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            }
            let origin = EventOrigin32::Exception(cause);
            assert_eq!(
                engine.deliver_detected_exception(epoch),
                Ok(entered(origin, origin, 0x77f4, 0x202, 4, false))
            );
            assert_eq!(
                engine.arena(),
                expect_entered(&before, 0x2345, 0x77f4, 0x202, retired, work, 4)
            );
            assert_eq!(bytes(&engine, 0x7000), stack);
            assert_eq!(bytes(&engine, 0x1000), code);
            assert_eq!(
                engine.deliver_detected_exception(epoch),
                Err(HostError::Call(CallError::InvalidToken))
            );
        }
    }
}

#[test]
fn instruction_trap_delivery_faults_use_trusted_restart_and_preserve_f1_external_origin() {
    for (cause, reason, vector, bad, secondary, error) in [
        (
            ExceptionCause32::BreakpointInt3,
            ExitReason::Breakpoint,
            3,
            gate(15, 0, true),
            ExceptionCause32::GeneralProtection(SelectorErrorCode32::idt(3, false)),
            0x1a_u32,
        ),
        (
            ExceptionCause32::OverflowInto,
            ExitReason::Overflow,
            4,
            gate(15, 0, true),
            ExceptionCause32::GeneralProtection(SelectorErrorCode32::idt(4, false)),
            0x22,
        ),
        (
            ExceptionCause32::Debug(DebugCause32::Int1),
            ExitReason::DebugTrap,
            1,
            gate(15, 0, false),
            ExceptionCause32::SegmentNotPresent(SelectorErrorCode32::idt(1, true)),
            0x0b,
        ),
    ] {
        let (mut engine, owner) = fixture(false, &[(vector, bad)]);
        let epoch = seal(
            &mut engine,
            owner,
            event(cause),
            reason,
            0x1001,
            8,
            0x7800,
            5,
            Some(restart()),
        );
        let before = engine.arena().to_vec();
        let mut stack = bytes(&engine, 0x7000);
        for (offset, value) in [
            (0x7f0, error),
            (0x7f4, 0x1000),
            (0x7f8, 0x1b),
            (0x7fc, 0x10302),
        ] {
            stack[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        let initial = EventOrigin32::Exception(cause);
        let origin = EventOrigin32::Exception(secondary);
        assert_eq!(
            engine.deliver_detected_exception(epoch),
            Ok(entered(initial, origin, 0x77f0, 0x202, 0x204, false))
        );
        assert_eq!(
            engine.arena(),
            expect_entered(&before, 0x2345, 0x77f0, 0x202, 7, 4, 0x204)
        );
        assert_eq!(bytes(&engine, 0x7000), stack);
    }
    // F1 ignores gate DPL even though it is an instruction-origin trap with trusted restart data.
    let (mut engine, owner) = fixture(false, &[(1, gate(15, 0, true))]);
    let cause = ExceptionCause32::Debug(DebugCause32::Int1);
    let epoch = seal(
        &mut engine,
        owner,
        event(cause),
        ExitReason::DebugTrap,
        0x1001,
        8,
        0x7800,
        5,
        Some(restart()),
    );
    let origin = EventOrigin32::Exception(cause);
    assert_eq!(
        engine.deliver_detected_exception(epoch),
        Ok(entered(origin, origin, 0x77f4, 0x202, 4, false))
    );
    assert_eq!(
        &bytes(&engine, 0x7000)[0x7f4..0x800],
        &[1, 0x10, 0, 0, 0x1b, 0, 0, 0, 2, 3, 0, 0]
    );
}

#[test]
fn double_fault_is_an_abort_with_zero_error_and_diagnostic_rf_then_shutdown_latches() {
    let absent = SegmentDescriptorRecord::from_words(0, 0);
    let (mut engine, owner) = fixture(false, &[(0, absent)]);
    let epoch = seal(
        &mut engine,
        owner,
        event(ExceptionCause32::DivideError),
        ExitReason::DivideError,
        0x1000,
        0,
        0x7800,
        0,
        None,
    );
    let before = engine.arena().to_vec();
    let initial = EventOrigin32::Exception(ExceptionCause32::DivideError);
    let df = EventOrigin32::Exception(ExceptionCause32::DoubleFault);
    assert_eq!(
        ExceptionCause32::DoubleFault.class(),
        ExceptionClass32::Abort
    );
    assert_eq!(df.error_code(), Some(0));
    assert_eq!(
        df.saved_flags(0x302),
        0x302,
        "abort does not inherit ordinary fault rf forcing"
    );
    assert_eq!(df.saved_flags(0x10302), 0x10302);
    assert_eq!(
        engine.deliver_detected_exception(epoch),
        Ok(entered(initial, df, 0x77f0, 0x202, 0x204, false))
    );
    assert_eq!(
        engine.arena(),
        expect_entered(&before, 0x2345, 0x77f0, 0x202, 0, 0, 0x204)
    );
    assert_eq!(
        &bytes(&engine, 0x7000)[0x7f0..0x800],
        &[0, 0, 0, 0, 0, 0x10, 0, 0, 0x1b, 0, 0, 0, 2, 3, 0, 0]
    );

    let (mut engine, owner) = fixture(false, &[(6, absent), (13, absent), (8, absent)]);
    let epoch = seal(
        &mut engine,
        owner,
        event(ExceptionCause32::InvalidOpcode),
        ExitReason::InvalidOpcode,
        0x1000,
        0,
        0x7800,
        0,
        None,
    );
    let before = engine.arena().to_vec();
    let stack = bytes(&engine, 0x7000);
    let initial = EventOrigin32::Exception(ExceptionCause32::InvalidOpcode);
    let secondary = ExceptionCause32::GeneralProtection(SelectorErrorCode32::idt(8, true));
    assert_eq!(
        engine.deliver_detected_exception(epoch),
        Ok(DetectedEntryResult32::Shutdown { initial, secondary })
    );
    let mut want = before;
    want[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]
        .copy_from_slice(&packet([3, 0, 0x43, 0, 0, 0]));
    assert_eq!(
        engine.arena(),
        want,
        "only the final failed-df packet is externally published"
    );
    assert_eq!(bytes(&engine, 0x7000), stack);
    assert!(engine.is_exception_shutdown());
    assert_eq!(
        engine.deliver_detected_exception(epoch),
        Err(HostError::CpuShutdown)
    );
    assert_eq!(
        engine.begin_exception_detection(owner),
        Err(HostError::CpuShutdown)
    );
    assert_eq!(
        engine.discard_exception_detection(epoch),
        Err(HostError::CpuShutdown)
    );
}

#[test]
fn authority_absence_restores_unattempted_trap_but_not_an_already_faulted_attempt() {
    for faulted in [false, true] {
        let mut overrides = vec![(3, gate(5, 3, true))];
        if faulted {
            overrides = vec![(3, gate(15, 0, true)), (13, gate(5, 0, true))];
        }
        let (mut engine, owner) = fixture(false, &overrides);
        let cause = ExceptionCause32::BreakpointInt3;
        let epoch = seal(
            &mut engine,
            owner,
            event(cause),
            ExitReason::Breakpoint,
            0x1001,
            8,
            0x7800,
            5,
            Some(restart()),
        );
        let before = engine.arena().to_vec();
        let stack = bytes(&engine, 0x7000);
        let initial = EventOrigin32::Exception(cause);
        let origin = if faulted {
            EventOrigin32::Exception(ExceptionCause32::GeneralProtection(
                SelectorErrorCode32::idt(3, false),
            ))
        } else {
            initial
        };
        assert_eq!(
            engine.deliver_detected_exception(epoch),
            Ok(DetectedEntryResult32::NeedsTaskOrPrivilegeAuthority {
                initial,
                origin,
                kind: EventAuthorityKind32::Task,
                selector: 0x40,
            })
        );
        let mut want = before;
        want[48..52].copy_from_slice(&0x1000_u32.to_le_bytes());
        want[52..56].copy_from_slice(&(if faulted { 0x302_u32 } else { 0x10302 }).to_le_bytes());
        want[12..16].copy_from_slice(&u32::from(!faulted).to_le_bytes());
        want[EXIT_OFFSET + 20..EXIT_OFFSET + 24].copy_from_slice(&7_u32.to_le_bytes());
        want[WORK_CONSUMED_OFFSET..WORK_CONSUMED_OFFSET + 4].copy_from_slice(&4_u32.to_le_bytes());
        want[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]
            .copy_from_slice(&packet([8, 0, 1, 0x40, 0, 0]));
        assert_eq!(engine.arena(), want);
        assert_eq!(bytes(&engine, 0x7000), stack);
    }
}

#[test]
fn unknown_source_and_delivery_page_faults_never_manufacture_a_pf_or_partial_frame() {
    let fault = MemoryFault {
        address: GuestAddress(0x6000),
        access: Access::Read,
        reason: FaultReason::Unmapped,
    };
    let (mut engine, owner) = fixture(false, &[]);
    let source_event = DetectedEvent32::NeedsPagingAuthority { fault, length: 4 };
    let epoch = seal(
        &mut engine,
        owner,
        source_event,
        ExitReason::MemoryFault { fault, length: 4 },
        0x1000,
        0,
        0x7800,
        0,
        None,
    );
    let before = engine.arena().to_vec();
    let stack = bytes(&engine, 0x7000);
    assert_eq!(
        engine.deliver_detected_exception(epoch),
        Ok(DetectedEntryResult32::NeedsPagingAuthority {
            initial: None,
            origin: None,
            fault,
            length: 4
        })
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(bytes(&engine, 0x7000), stack);

    let cause = ExceptionCause32::GeneralProtection(SelectorErrorCode32::null(false));
    let (mut engine, owner) = fixture(false, &[]);
    let epoch = seal(
        &mut engine,
        owner,
        event(cause),
        ExitReason::GeneralProtection { error_code: 0 },
        0x1000,
        2,
        0x700c,
        0,
        None,
    );
    let before = engine.arena().to_vec();
    let stack = bytes(&engine, 0x7000);
    let version = engine.memory().unwrap().code_validation_version();
    let fault = MemoryFault {
        address: GuestAddress(0x6ffc),
        access: Access::Write,
        reason: FaultReason::Unmapped,
    };
    let origin = EventOrigin32::Exception(cause);
    assert_eq!(
        engine.deliver_detected_exception(epoch),
        Ok(DetectedEntryResult32::NeedsPagingAuthority {
            initial: Some(origin),
            origin: Some(origin),
            fault,
            length: 4
        })
    );
    let mut want = before;
    want[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]
        .copy_from_slice(&packet([1, 0, 1, 0x6ffc, 2, 0x204]));
    assert_eq!(engine.arena(), want);
    assert_eq!(bytes(&engine, 0x7000), stack);
    assert_eq!(engine.memory().unwrap().code_validation_version(), version);
    assert_eq!(
        engine
            .segment_descriptors
            .raw_descriptor(0x40)
            .unwrap()
            .descriptor_type(),
        10
    );
}

#[test]
fn detected_stale_material_retains_identity_and_rep_progress_while_supplied_api_stays_strict() {
    for resident in [false, true] {
        let (mut engine, owner) = fixture(resident, &[]);
        let cause =
            ExceptionCause32::Debug(DebugCause32::SingleStep(TrapPoint32::RepetitionInProgress));
        let epoch = seal(
            &mut engine,
            owner,
            event(cause),
            ExitReason::DebugTrap,
            0x1000,
            0,
            0x7800,
            1,
            None,
        );
        engine
            .memory
            .as_mut()
            .unwrap()
            .cpu_write(GuestAddress(0x1000), &[0x7a])
            .unwrap();
        let before = engine.arena().to_vec();
        let source = bytes(&engine, 0x1000);
        let origin = EventOrigin32::Exception(cause);
        let input = crate::cpu::x86::FrameInput32 {
            eip: 0x1000,
            esp: 0x7800,
            flags: 0x302,
            cs: 0x1b,
        };
        let ordinary = match owner {
            PendingOwner::Resident(id) => engine.deliver_exception_resident(KEY, id, origin, input),
            PendingOwner::Replacement(_) => engine.deliver_exception(origin, input),
            _ => unreachable!(),
        };
        assert!(
            ordinary.is_err(),
            "ordinary supplied-event guard still requires current material"
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(
            engine.deliver_detected_exception(epoch),
            Ok(entered(origin, origin, 0x77f4, 0x202, 4, true))
        );
        assert_eq!(
            engine.arena(),
            expect_entered(&before, 0x2345, 0x77f4, 0x202, 0, 1, 4)
        );
        assert_eq!(bytes(&engine, 0x1000), source);
        assert_eq!(&bytes(&engine, 0x7000)[0x7fc..0x800], &[2, 3, 1, 0]);
    }
}

#[test]
fn version_failure_publishes_only_the_final_infrastructure_packet_and_no_accessed_bit() {
    let (mut engine, owner) = fixture(false, &[]);
    let cause = ExceptionCause32::InvalidOpcode;
    let epoch = seal(
        &mut engine,
        owner,
        event(cause),
        ExitReason::InvalidOpcode,
        0x1000,
        0,
        0x7800,
        0,
        None,
    );
    engine.memory.as_mut().unwrap().exhaust_versions_for_test();
    let before = engine.arena().to_vec();
    let stack = bytes(&engine, 0x7000);
    let version = engine.memory().unwrap().code_validation_version();
    let origin = EventOrigin32::Exception(cause);
    assert_eq!(
        engine.deliver_detected_exception(epoch),
        Ok(DetectedEntryResult32::Infrastructure {
            initial: origin,
            origin,
            error: MemoryError::VersionExhausted
        })
    );
    let mut want = before;
    want[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE].copy_from_slice(&packet([2, 0, 1, 0, 0, 4]));
    assert_eq!(engine.arena(), want);
    assert_eq!(bytes(&engine, 0x7000), stack);
    assert_eq!(engine.memory().unwrap().code_validation_version(), version);
    assert_eq!(
        engine
            .segment_descriptors
            .raw_descriptor(0x40)
            .unwrap()
            .descriptor_type(),
        10
    );
}
