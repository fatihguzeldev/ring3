use super::*;
use crate::{
    abi::arena::{HELPER_OFFSET, SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET, TRANSFER_OFFSET},
    cpu::x86::{
        DebugCause32, DetectedEntryResult32, EnteredEvent32, InterruptDescriptorTable32,
        SegmentDescriptorRecord, SelectorErrorCode32, TrapPoint32,
    },
    memory::{Access, FaultReason, GuestAddress},
    process::calls::{
        call::PendingCall,
        callback::{SuspendedCallback, SuspendedRecord},
    },
    windows::{CallFrame32, CallingConvention32},
};

const KEY: u64 = 0x4155_544f;
const FLAGS: u32 = 0x0024_0ed7;
const TARGET: u32 = 0x2300;

fn record(magic: &[u8; 4], size: usize, version: u16, fields: &[u32]) -> Vec<u8> {
    let mut bytes = vec![0; size];
    bytes[..4].copy_from_slice(magic);
    bytes[4..6].copy_from_slice(&version.to_le_bytes());
    bytes[6..8].copy_from_slice(&1_u16.to_le_bytes());
    bytes[8..12].copy_from_slice(&(size as u32).to_le_bytes());
    for (index, value) in fields.iter().enumerate() {
        bytes[16 + 4 * index..20 + 4 * index].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}
fn context(target: bool) -> (Vec<u8>, Vec<u8>) {
    let mut sg = record(b"R3SG", 64, 3, &[]);
    for (index, value) in [
        0x23_u16,
        if target { 0x4b } else { 0x1b },
        0x23,
        0x23,
        0x3b,
        0,
    ]
    .into_iter()
    .enumerate()
    {
        sg[16 + 2 * index..18 + 2 * index].copy_from_slice(&value.to_le_bytes());
    }
    for (offset, value) in [28, 32, 36, 40, 44, 56].into_iter().zip([
        0xf33f_ffff_u32,
        0xf3bf_ffff,
        0xf33f_ffff,
        0xf33f_ffff,
        0xf33f_ffff,
        0,
    ]) {
        sg[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    (
        sg,
        record(
            b"R3SB",
            40,
            1,
            &[0, if target { 0x4000 } else { 0 }, 0, 0, 0, 0],
        ),
    )
}
fn fixture(resident: bool, gates: &[(u8, u32, bool)]) -> (EngineInstance, PendingOwner) {
    let mut engine = EngineInstance::new(4, KEY).unwrap();
    let mut table = Box::new([SegmentDescriptorRecord::from_words(0, 0); 256]);
    for &(vector, dpl, present) in gates {
        table[usize::from(vector)] = SegmentDescriptorRecord::from_words(
            0x0048_2300,
            u32::from(present) << 15 | dpl << 13 | 15 << 8,
        );
    }
    engine
        .configure_interrupt_descriptor_table(InterruptDescriptorTable32::new(0x7ff, table))
        .unwrap();
    engine
        .configure_segment_descriptor_raw(
            0x48,
            SegmentDescriptorRecord::from_words(0x4000_ffff, 0x00cf_fa00),
        )
        .unwrap();
    for (page, permissions) in [(0x1000, 7), (0x5000, 3), (0x6000, 7), (0x7000, 3)] {
        engine.map(page, 1, permissions).unwrap();
    }
    let (sg, sb) = context(false);
    let arena = engine.arena_mut().unwrap();
    arena[SEGMENTS_OFFSET..SEGMENTS_OFFSET + 64].copy_from_slice(&sg);
    arena[SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + 40].copy_from_slice(&sb);
    arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 2].copy_from_slice(&[0xeb, 0xfe]);
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
fn stop(engine: &mut EngineInstance, exit: (u32, u32, u16, u32, u32), flags: u32, work: u32) {
    let (reason, detail, version, pc, retired) = exit;
    let arena = engine.arena_mut().unwrap();
    let regs = [
        0x1234_5678,
        0x2345_6789,
        0x3456_789a,
        0x4567_89ab,
        0x8000,
        0x5678_9abc,
        0x6789_abcd,
        0x789a_bcde,
    ];
    let mut fields = regs.to_vec();
    fields.extend([pc, flags]);
    arena[..STATE_SIZE].copy_from_slice(&record(b"R3ST", STATE_SIZE, 3, &fields));
    arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE].copy_from_slice(&record(
        b"R3EX",
        EXIT_SIZE,
        version,
        &[reason, retired, detail, 0, 0, 0],
    ));
    arena[WORK_OFFSET + 16..WORK_OFFSET + 20].copy_from_slice(&work.to_le_bytes());
}
fn ram(engine: &EngineInstance) -> [Vec<u8>; 4] {
    [0x1000, 0x5000, 0x6000, 0x7000].map(|page| {
        let mut bytes = vec![0; 4096];
        engine
            .memory
            .as_ref()
            .unwrap()
            .read(GuestAddress(page), &mut bytes)
            .unwrap();
        bytes
    })
}
fn entered_want(before: &[u8], retired: u32, error: bool, flags: u32) -> Vec<u8> {
    let mut want = before.to_vec();
    let (sg, sb) = context(true);
    want[SEGMENTS_OFFSET..SEGMENTS_OFFSET + 64].copy_from_slice(&sg);
    want[SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + 40].copy_from_slice(&sb);
    want[12..16].fill(0);
    for (offset, value) in [
        (32, if error { 0x7ff0_u32 } else { 0x7ff4 }),
        (48, TARGET),
        (52, flags),
    ] {
        want[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let version = u16::from_le_bytes(before[60..62].try_into().unwrap()).max(6);
    want[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE].copy_from_slice(&record(
        b"R3EX",
        EXIT_SIZE,
        version,
        &[3, retired, 0, 0, 0, 0],
    ));
    want[HELPER_OFFSET..HELPER_OFFSET + 40].copy_from_slice(&record(
        b"R3MH",
        40,
        12,
        &[0, TARGET, 0, 0, 0, if error { 0x204 } else { 4 }],
    ));
    want
}
fn with_frame(mut bytes: [Vec<u8>; 4], error: Option<u32>, pc: u32, flags: u32) -> [Vec<u8>; 4] {
    let mut words = Vec::new();
    if let Some(error) = error {
        words.push(error);
    }
    words.extend([pc, 0x1b, flags]);
    let start = 4096 - 4 * words.len();
    for (index, value) in words.into_iter().enumerate() {
        bytes[3][start + 4 * index..start + 4 * index + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}
fn event(cause: ExceptionCause32) -> DetectedEvent32 {
    DetectedEvent32::Exception(EventOrigin32::Exception(cause))
}

#[test]
fn sealed_faults_publish_one_complete_entry_and_replay_cannot_write_again() {
    for resident in [false, true] {
        for (cause, reason, version, error) in [
            (ExceptionCause32::InvalidOpcode, 20, 7, false),
            (ExceptionCause32::DivideError, 10, 5, false),
            (
                ExceptionCause32::GeneralProtection(SelectorErrorCode32::null(false)),
                11,
                9,
                true,
            ),
            (
                ExceptionCause32::InvalidTss(SelectorErrorCode32::null(false)),
                21,
                10,
                true,
            ),
        ] {
            let (mut engine, owner) = fixture(resident, &[(cause.vector(), 0, true)]);
            let epoch = engine.begin_exception_detection(owner).unwrap();
            stop(&mut engine, (reason, 0, version, 0x1000, 0), FLAGS, 0);
            let before = engine.arena().to_vec();
            let before_ram = ram(&engine);
            engine
                .finish_exception_detection(epoch, event(cause), None)
                .unwrap();
            assert_eq!(engine.arena(), before, "sealing does not publish a frame");
            let origin = EventOrigin32::Exception(cause);
            assert_eq!(
                engine.deliver_detected_exception(epoch),
                Ok(DetectedEntryResult32::Entered {
                    initial: origin,
                    origin,
                    entered: EnteredEvent32 {
                        target: TARGET,
                        esp: if error { 0x7ff0 } else { 0x7ff4 },
                        flags: FLAGS,
                        frame_info: if error { 0x204 } else { 4 },
                        code_invalidated: false,
                    },
                })
            );
            assert_eq!(engine.arena(), entered_want(&before, 0, error, FLAGS));
            assert_eq!(
                ram(&engine),
                with_frame(before_ram, error.then_some(0), 0x1000, FLAGS | 0x10000)
            );
            let after = engine.arena().to_vec();
            let after_ram = ram(&engine);
            assert_eq!(
                engine.deliver_detected_exception(epoch),
                Err(HostError::Call(CallError::InvalidToken))
            );
            assert_eq!(engine.arena(), after);
            assert_eq!(ram(&engine), after_ram);
        }
    }
}

#[test]
fn failed_instruction_traps_use_original_pc_retirement_work_and_processor_rf() {
    for resident in [false, true] {
        for (cause, reason, secondary, error) in [
            (ExceptionCause32::BreakpointInt3, 17, 13, 0x1a),
            (ExceptionCause32::OverflowInto, 18, 13, 0x22),
            (ExceptionCause32::Debug(DebugCause32::Int1), 16, 11, 0x0b),
        ] {
            let (mut engine, owner) = fixture(
                resident,
                &[(cause.vector(), 0, reason != 16), (secondary, 0, true)],
            );
            let epoch = engine.begin_exception_detection(owner).unwrap();
            stop(&mut engine, (reason, 0, 7, 0x1001, 1), FLAGS, 4);
            let before = engine.arena().to_vec();
            let before_ram = ram(&engine);
            engine
                .finish_exception_detection(
                    epoch,
                    event(cause),
                    Some(InstructionTrapRestart32 {
                        eip: 0x1000,
                        retired: 0,
                        flags: FLAGS | 0x10000,
                        ss_debug_shadow: true,
                        work_consumed: Some(3),
                    }),
                )
                .unwrap();
            let code = SelectorErrorCode32::from_bits(error).unwrap();
            let final_cause = if secondary == 13 {
                ExceptionCause32::GeneralProtection(code)
            } else {
                ExceptionCause32::SegmentNotPresent(code)
            };
            let result = engine.deliver_detected_exception(epoch).unwrap();
            assert!(
                matches!(result, DetectedEntryResult32::Entered { initial, origin, entered }
                if initial == EventOrigin32::Exception(cause)
                && origin == EventOrigin32::Exception(final_cause)
                && entered.esp == 0x7ff0 && entered.frame_info == 0x204)
            );
            let mut want = entered_want(&before, 0, true, FLAGS);
            want[WORK_OFFSET + 16..WORK_OFFSET + 20].copy_from_slice(&3_u32.to_le_bytes());
            assert_eq!(engine.arena(), want);
            assert_eq!(
                ram(&engine),
                with_frame(before_ram, Some(error), 0x1000, FLAGS | 0x10000)
            );
        }
    }
}

#[test]
fn partial_rep_progress_survives_physical_staleness_and_state_drift_is_not_consumed() {
    for resident in [false, true] {
        let (mut engine, owner) = fixture(resident, &[(1, 0, true)]);
        let epoch = engine.begin_exception_detection(owner).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .cpu_write(GuestAddress(0x1000), &[0x7a])
            .unwrap();
        stop(
            &mut engine,
            (16, 0, 7, 0x1000, 0),
            (FLAGS & !0x400) | 0x100,
            1,
        );
        let arena = engine.arena_mut().unwrap();
        for (offset, value) in [(20, 1_u32), (40, 0x5001), (44, 0x1001)] {
            arena[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        let cause =
            ExceptionCause32::Debug(DebugCause32::SingleStep(TrapPoint32::RepetitionInProgress));
        engine
            .finish_exception_detection(epoch, event(cause), None)
            .unwrap();
        let before = engine.arena().to_vec();
        let before_ram = ram(&engine);
        engine.arena_mut().unwrap()[20] ^= 1;
        let changed = engine.arena().to_vec();
        assert_eq!(
            engine.deliver_detected_exception(epoch),
            Err(HostError::Call(CallError::StateChanged))
        );
        assert_eq!(engine.arena(), changed);
        assert_eq!(ram(&engine), before_ram);
        engine.arena_mut().unwrap().copy_from_slice(&before);
        // scratch is not architectural state and may change without invalidating the sealed event.
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + 4] ^= 1;
        let before = engine.arena().to_vec();
        let result = engine.deliver_detected_exception(epoch).unwrap();
        assert!(
            matches!(result, DetectedEntryResult32::Entered { entered, .. } if entered.code_invalidated)
        );
        assert_eq!(
            engine.arena(),
            entered_want(&before, 0, false, FLAGS & !0x400)
        );
        assert_eq!(
            ram(&engine),
            with_frame(before_ram, None, 0x1000, (FLAGS & !0x400) | 0x10100)
        );
    }
}

#[test]
fn pending_call_and_callback_token_drift_preserve_the_same_ready_event() {
    use crate::abi::callback::CallbackRecord32;
    let (mut engine, owner) = fixture(false, &[(6, 0, true)]);
    let epoch = engine.begin_exception_detection(owner).unwrap();
    stop(&mut engine, (20, 0, 7, 0x1000, 0), FLAGS, 0);
    engine.arena_mut().unwrap()[32..36].copy_from_slice(&0x7800_u32.to_le_bytes());
    engine
        .memory
        .as_mut()
        .unwrap()
        .cpu_write(GuestAddress(0x7800), &0x1100_u32.to_le_bytes())
        .unwrap();
    engine
        .finish_exception_detection(epoch, event(ExceptionCause32::InvalidOpcode), None)
        .unwrap();
    let before = engine.arena().to_vec();
    let before_ram = ram(&engine);
    let frame = CallFrame32::capture(
        engine.memory.as_ref().unwrap(),
        decode_state(&before[..STATE_SIZE]).unwrap(),
        CallingConvention32::Cdecl,
        0,
    )
    .unwrap();
    engine.pending_call = Some(PendingCall {
        task_revision: engine.task_execution_revision(),
        token: 1,
        owner,
        frame,
        state: before[..STATE_SIZE].try_into().unwrap(),
        exit: before[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE]
            .try_into()
            .unwrap(),
        extended: ExtendedCpuState::capture(&before).unwrap(),
    });
    assert_eq!(
        engine.deliver_detected_exception(epoch),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(ram(&engine), before_ram);
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
    assert_eq!(
        engine.deliver_detected_exception(epoch),
        Err(HostError::Call(CallError::InvalidToken))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(ram(&engine), before_ram);
    engine.callback = None;
    let result = engine.deliver_detected_exception(epoch).unwrap();
    assert!(
        matches!(result, DetectedEntryResult32::Entered { entered, .. } if entered.esp == 0x77f4)
    );
    let mut want = entered_want(&before, 0, false, FLAGS);
    want[32..36].copy_from_slice(&0x77f4_u32.to_le_bytes());
    let mut expected_ram = before_ram;
    expected_ram[3][0x7f4..0x800]
        .copy_from_slice(&[0, 0x10, 0, 0, 0x1b, 0, 0, 0, 0xd7, 0x0e, 0x25, 0]);
    assert_eq!(engine.arena(), want);
    assert_eq!(ram(&engine), expected_ram);
}

#[test]
fn double_fault_keeps_abort_rf_and_failed_double_fault_latches_shutdown() {
    for shutdown in [false, true] {
        let (mut engine, owner) = fixture(true, &[(13, 0, false), (8, 0, !shutdown)]);
        let epoch = engine.begin_exception_detection(owner).unwrap();
        stop(&mut engine, (11, 0, 9, 0x1000, 0), FLAGS, 0);
        let before = engine.arena().to_vec();
        let before_ram = ram(&engine);
        let initial = EventOrigin32::Exception(ExceptionCause32::GeneralProtection(
            SelectorErrorCode32::null(false),
        ));
        engine
            .finish_exception_detection(epoch, DetectedEvent32::Exception(initial), None)
            .unwrap();
        let result = engine.deliver_detected_exception(epoch).unwrap();
        if shutdown {
            assert_eq!(
                result,
                DetectedEntryResult32::Shutdown {
                    initial,
                    secondary: ExceptionCause32::SegmentNotPresent(SelectorErrorCode32::idt(
                        8, true
                    )),
                }
            );
            let mut want = before.clone();
            want[HELPER_OFFSET..HELPER_OFFSET + 40].copy_from_slice(&record(
                b"R3MH",
                40,
                12,
                &[6, 0, 0x43, 0, 0, 0],
            ));
            assert_eq!(engine.arena(), want);
            assert_eq!(ram(&engine), before_ram);
            assert!(engine.is_exception_shutdown());
            assert_eq!(
                engine.begin_exception_detection(owner),
                Err(HostError::CpuShutdown)
            );
            assert_eq!(
                engine.discard_exception_detection(epoch),
                Err(HostError::CpuShutdown)
            );
            assert_eq!(engine.arena(), want);
            assert_eq!(ram(&engine), before_ram);
        } else {
            assert!(
                matches!(result, DetectedEntryResult32::Entered { initial: actual, origin, entered }
                if actual == initial && origin == EventOrigin32::Exception(ExceptionCause32::DoubleFault)
                && entered.esp == 0x7ff0 && entered.frame_info == 0x204)
            );
            assert_eq!(engine.arena(), entered_want(&before, 0, true, FLAGS));
            assert_eq!(ram(&engine), with_frame(before_ram, Some(0), 0x1000, FLAGS));
        }
    }
}

#[test]
fn raw_bus_memory_fault_never_manufactures_a_page_fault_or_changes_its_packet() {
    let (mut engine, owner) = fixture(false, &[(14, 0, true)]);
    let epoch = engine.begin_exception_detection(owner).unwrap();
    stop(&mut engine, (5, 1, 6, 0x1000, 0), FLAGS, 0);
    let fault = MemoryFault {
        address: GuestAddress(0xdead_0000),
        access: Access::Read,
        reason: FaultReason::Unmapped,
    };
    let arena = engine.arena_mut().unwrap();
    arena[84..88].copy_from_slice(&fault.address.0.to_le_bytes());
    arena[88..92].copy_from_slice(&1_u32.to_le_bytes());
    arena[92..96].copy_from_slice(&4_u32.to_le_bytes());
    arena[HELPER_OFFSET..HELPER_OFFSET + 40].copy_from_slice(&record(
        b"R3MH",
        40,
        10,
        &[1, 0, 1, 0xdead_0000, 1, 4],
    ));
    engine
        .finish_exception_detection(
            epoch,
            DetectedEvent32::NeedsPagingAuthority { fault, length: 4 },
            None,
        )
        .unwrap();
    let before = engine.arena().to_vec();
    let before_ram = ram(&engine);
    assert_eq!(
        engine.deliver_detected_exception(epoch),
        Ok(DetectedEntryResult32::NeedsPagingAuthority {
            initial: None,
            origin: None,
            fault,
            length: 4,
        })
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(ram(&engine), before_ram);
}
