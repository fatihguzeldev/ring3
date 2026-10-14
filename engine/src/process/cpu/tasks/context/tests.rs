use super::*;
use crate::{
    abi::{
        arena::{EXIT_OFFSET, HELPER_OFFSET},
        tasks::decode_task_context,
        x86::{encode_exit_v10, encode_state_v3},
    },
    cpu::{
        ExecutionExit, ExitReason,
        x86::{
            DetectedEntryResult32, EnteredEvent32, EventOrigin32, ExceptionCause32,
            InterruptDescriptorTable32, SegmentDescriptorRecord, SelectorErrorCode32, State32,
            tasks::{TaskControlState32, TaskRegister, TaskSave, TaskTransitionFault32},
        },
    },
    memory::GuestAddress,
    process::{
        CommittedTaskReturn32, TaskReturnTransition32, calls::call::PendingOwner,
        cpu::interrupts::detection::DetectedEvent32,
    },
};
const KEY: u64 = 0x54435243;
const PAGES: [u32; 8] = [
    0, 0x1000, 0x2000, 0x3000, 0x5000, 0x6000, 0x7000, 0xfffff000,
];
fn task(base: u32) -> SegmentDescriptorRecord {
    SegmentDescriptorRecord::from_words(
        base << 16 | 0x67,
        base & 0xff000000 | (base >> 16) & 0xff | 0x8b00,
    )
}
fn frame_packet() -> [u8; 40] {
    let mut bytes = [0; 40];
    bytes[..16].copy_from_slice(b"R3MH\x0c\0\x01\0\x28\0\0\0\0\0\0\0");
    for (offset, value) in [(20, 0x3000_u32), (36, 0x204)] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}
fn memory(engine: &EngineInstance) -> [Vec<u8>; 8] {
    PAGES.map(|page| {
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
fn write(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .cpu_write(GuestAddress(address), bytes)
        .unwrap();
}
fn setup(resident: bool, wrap: bool, ss_fault: bool) -> (EngineInstance, PendingOwner) {
    let mut engine = EngineInstance::new(8, KEY).unwrap();
    for page in PAGES {
        engine.map(page, 1, 7).unwrap();
        write(&mut engine, page, &[0xa5; 4096]);
    }
    let mut idt = Box::new([SegmentDescriptorRecord::from_words(0, 0); 256]);
    for vector in [8, 11, 12] {
        idt[vector] = SegmentDescriptorRecord::from_words(0x001b3000, 0x8f00);
    }
    engine
        .configure_interrupt_descriptor_table(InterruptDescriptorTable32::new(0xffff, idt))
        .unwrap();
    let current = TaskRegister::from_cached(0x43, task(0x5000)).unwrap();
    engine.configure_task_return_sources(current, 0x5f).unwrap();
    engine
        .configure_segment_descriptor_raw(0x40, task(0x5000))
        .unwrap();
    engine
        .configure_segment_descriptor_raw(0x48, task(0x6000))
        .unwrap();
    engine
        .configure_segment_descriptor_raw(
            0x50,
            SegmentDescriptorRecord::from_words(
                0xffff,
                if ss_fault { 0x00cf7300 } else { 0x00cf7b00 },
            ),
        )
        .unwrap();
    if wrap {
        engine
            .configure_segment_descriptor_raw(
                0x58,
                SegmentDescriptorRecord::from_words(0xfff000ff, 0xff00f3ff),
            )
            .unwrap();
    }
    engine
        .configure_task_controls(TaskControlState32::new(0x33, 0xdead0000, 0, 0x400, None).unwrap())
        .unwrap();
    write(&mut engine, 0x5000, &0x4b_u16.to_le_bytes());
    let mut tss = [0xa5; 104];
    for (offset, value) in [(28, 0xcccc0000_u32), (32, 0x2000), (36, 0x250fd7)] {
        tss[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    for index in 0..8 {
        let value = if index == 4 {
            if wrap { 0xdead0018_u32 } else { 0x7000 }
        } else {
            0x11223344 + index as u32
        };
        tss[40 + index * 4..44 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    let selectors = [
        0x23_u16,
        if ss_fault { 0x1b } else { 0x53 },
        if ss_fault {
            0x53
        } else if wrap {
            0x5b
        } else {
            0x23
        },
        0x23,
        0x3b,
        0,
    ];
    for (index, value) in selectors.into_iter().enumerate() {
        tss[72 + index * 4..74 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    tss[96..98].fill(0);
    tss[100..104].fill(0);
    write(&mut engine, 0x6000, &tss);
    for address in [0x2000, 0x3000] {
        write(&mut engine, address, &[0xeb, 0xfe]);
    }
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x20, 0, 0, 2, 0, 0, 0]);
    let owner = if resident {
        PendingOwner::Resident(engine.compile_resident(1).unwrap().get())
    } else {
        PendingOwner::Replacement(engine.compile(1).unwrap())
    };
    encode_state_v3(
        &State32 {
            registers: [0x77777777; 8],
            eip: 0x2000,
            eflags: 0x254fd7,
        },
        &mut engine.arena_mut().unwrap()[..56],
    )
    .unwrap();
    (engine, owner)
}
fn transition(engine: &mut EngineInstance, owner: PendingOwner) -> CommittedTaskReturn32 {
    let save = TaskSave {
        next_eip: 0x2001,
        flags: 0x254fd7,
        general: [0x77777777; 8],
        selectors: [0x23, 0x1b, 0x23, 0x23, 0x3b, 0],
    };
    let result = match owner {
        PendingOwner::Replacement(_) => engine.task_return_transition(save),
        PendingOwner::Resident(id) => engine.task_return_transition_resident(KEY, id, save),
        _ => unreachable!(),
    }
    .unwrap();
    let TaskReturnTransition32::Committed(value) = result else {
        panic!("{result:?}")
    };
    value
}
fn seal(engine: &mut EngineInstance, epoch: u64, fault: TaskTransitionFault32) -> EventOrigin32 {
    let (cause, reason) = match fault {
        TaskTransitionFault32::NotPresent(code) => (
            ExceptionCause32::SegmentNotPresent(SelectorErrorCode32::from_bits(code).unwrap()),
            ExitReason::SegmentNotPresent { error_code: code },
        ),
        TaskTransitionFault32::StackSegment(code) => (
            ExceptionCause32::StackSegment(SelectorErrorCode32::from_bits(code).unwrap()),
            ExitReason::StackSegment { error_code: code },
        ),
        _ => unreachable!(),
    };
    encode_exit_v10(
        &ExecutionExit { retired: 0, reason },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + 40],
    )
    .unwrap();
    let origin = EventOrigin32::Exception(cause);
    engine
        .finish_exception_detection(epoch, DetectedEvent32::Exception(origin), None)
        .unwrap();
    origin
}

#[test]
fn read_request_is_atomic_read_only_and_serializes_live_gprs_instead_of_old_phase_snapshot() {
    let (mut engine, owner) = setup(false, false, false);
    let result = transition(&mut engine, owner);
    assert_eq!(result.context.loaded_mask(), 4);
    engine.arena_mut().unwrap()[16..20].copy_from_slice(&0x13579bdf_u32.to_le_bytes());
    let before = engine.arena().to_vec();
    let ram = memory(&engine);
    engine.read_task_context().unwrap();
    let record =
        decode_task_context(&engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + TASK_CONTEXT_SIZE])
            .unwrap();
    let TaskContextRecord32::Loaded {
        revision,
        context,
        ss_debug_shadow,
    } = record
    else {
        panic!("loaded")
    };
    assert_eq!(revision, 2);
    assert!(!ss_debug_shadow);
    assert_eq!(context.state().registers[0], 0x13579bdf);
    assert_eq!(context.selectors()[1], 0x53);
    assert_eq!(context.loaded_mask(), 4);
    assert_eq!(
        context.fault(),
        Some(TaskTransitionFault32::NotPresent(0x50))
    );
    let mut expected = before;
    expected[TRANSFER_OFFSET..TRANSFER_OFFSET + TASK_CONTEXT_SIZE]
        .copy_from_slice(&engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + TASK_CONTEXT_SIZE]);
    assert_eq!(engine.arena(), expected);
    assert_eq!(memory(&engine), ram);
    let staging = engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + TASK_CONTEXT_SIZE].to_vec();
    engine.arena_mut().unwrap()[4..6].copy_from_slice(&4_u16.to_le_bytes());
    assert_eq!(engine.read_task_context(), Err(HostError::Infrastructure));
    assert_eq!(
        &engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + TASK_CONTEXT_SIZE],
        staging
    );
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.read_task_context().unwrap();
    assert_eq!(
        decode_task_context(&engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + TASK_CONTEXT_SIZE]),
        Ok(TaskContextRecord32::Absent)
    );
}

#[test]
fn detected_cs_fault_enters_on_new_qualified_ss_with_raw_cs_and_preserves_pending_data_context() {
    for resident in [false, true] {
        for wrap in [false, true] {
            let (mut engine, owner) = setup(resident, wrap, false);
            let epoch = engine.begin_exception_detection(owner).unwrap();
            let committed = transition(&mut engine, owner);
            let origin = seal(&mut engine, epoch, committed.context.fault().unwrap());
            let before = engine.arena().to_vec();
            let mut ram = memory(&engine);
            let version = engine.memory().unwrap().code_validation_version();
            let esp = if wrap { 0xdead0008 } else { 0x6ff0 };
            let entered = EnteredEvent32 {
                target: 0x3000,
                esp,
                flags: 0x240ed7,
                frame_info: 0x204,
                code_invalidated: false,
            };
            assert_eq!(
                engine.deliver_detected_exception(epoch),
                Ok(DetectedEntryResult32::Entered {
                    initial: origin,
                    origin,
                    entered
                })
            );
            let mut expected = before;
            for (offset, value) in [(32, esp), (48, 0x3000), (52, 0x240ed7)] {
                expected[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            }
            expected[12..16].fill(0);
            expected[EXIT_OFFSET..EXIT_OFFSET + 40].fill(0);
            expected[EXIT_OFFSET..EXIT_OFFSET + 16]
                .copy_from_slice(b"R3EX\x0a\0\x01\0\x28\0\0\0\0\0\0\0");
            expected[EXIT_OFFSET + 16..EXIT_OFFSET + 20].copy_from_slice(&3_u32.to_le_bytes());
            expected[HELPER_OFFSET..HELPER_OFFSET + 40].copy_from_slice(&frame_packet());
            assert_eq!(
                engine.arena(),
                expected,
                "complete arena keeps strict old R3SG unchanged while new context is explicit"
            );
            let addresses = if wrap {
                [4, 0, 0xffff_fffc, 0xffff_fff8]
            } else {
                [0x6ffc, 0x6ff8, 0x6ff4, 0x6ff0]
            };
            for (address, value) in addresses
                .into_iter()
                .zip([0x250fd7_u32, 0x53, 0x2000, 0x50])
            {
                let page = PAGES.iter().position(|p| *p == address & !4095).unwrap();
                ram[page][(address & 4095) as usize..(address & 4095) as usize + 4]
                    .copy_from_slice(&value.to_le_bytes());
            }
            assert_eq!(memory(&engine), ram);
            assert_eq!(
                engine.memory().unwrap().code_validation_version(),
                version + 1
            );
            let context = engine.task_incoming_context().unwrap().unwrap();
            assert_eq!(context.state().eip, 0x3000);
            assert_eq!(context.fault(), None);
            assert_eq!(context.loaded_mask(), 0x26);
            assert_eq!(context.selectors()[1], 0x1b);
            assert_eq!(context.selectors()[2], if wrap { 0x5b } else { 0x23 });
            assert_eq!(
                context.segment_image().state.bases[2],
                if wrap { 0xfffffff0 } else { 0 }
            );
            assert_eq!(engine.segment_context(), Err(HostError::InvalidRequest));
            assert!(engine.code_fetch_context().is_ok());
            match owner {
                PendingOwner::Replacement(generation) => {
                    engine.compile_stopped_entry(KEY, generation).unwrap();
                }
                PendingOwner::Resident(_) => {
                    engine.compile_stopped_resident_entry(KEY).unwrap();
                }
                _ => unreachable!(),
            }
        }
    }
}

#[test]
fn unavailable_new_ss_produces_secondary_ss_then_df_shutdown_without_any_frame_write() {
    for resident in [false, true] {
        let (mut engine, owner) = setup(resident, false, true);
        let epoch = engine.begin_exception_detection(owner).unwrap();
        let committed = transition(&mut engine, owner);
        assert_eq!(committed.context.loaded_mask(), 0);
        let origin = seal(&mut engine, epoch, committed.context.fault().unwrap());
        let before = engine.arena().to_vec();
        let ram = memory(&engine);
        let secondary = ExceptionCause32::StackSegment(SelectorErrorCode32::null(true));
        assert_eq!(
            engine.deliver_detected_exception(epoch),
            Ok(DetectedEntryResult32::Shutdown {
                initial: origin,
                secondary
            })
        );
        let mut expected = before;
        let mut packet = frame_packet();
        for (offset, value) in [(16, 7_u32), (20, 0), (24, 1)] {
            packet[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        expected[HELPER_OFFSET..HELPER_OFFSET + 40].copy_from_slice(&packet);
        assert_eq!(engine.arena(), expected);
        assert_eq!(memory(&engine), ram);
        assert!(engine.is_exception_shutdown());
        assert_eq!(engine.read_task_context(), Err(HostError::CpuShutdown));
    }
}

#[test]
fn sealed_detection_rejects_task_revision_or_raw_context_changes_before_consumption() {
    let (mut engine, owner) = setup(false, false, false);
    let epoch = engine.begin_exception_detection(owner).unwrap();
    let committed = transition(&mut engine, owner);
    seal(&mut engine, epoch, committed.context.fault().unwrap());
    let arena = engine.arena().to_vec();
    let ram = memory(&engine);
    engine.task_execution_revision += 1;
    assert_eq!(
        engine.deliver_detected_exception(epoch),
        Err(HostError::Call(crate::process::CallError::StateChanged))
    );
    engine.task_execution_revision -= 1;
    let original = engine.task_incoming;
    let mut state = original.unwrap().state();
    state.registers[0] ^= 1;
    engine.task_incoming = Some(original.unwrap().with_state(state));
    assert_eq!(
        engine.deliver_detected_exception(epoch),
        Err(HostError::Call(crate::process::CallError::StateChanged))
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(memory(&engine), ram);
    engine.task_incoming = original;
    assert!(matches!(
        engine.deliver_detected_exception(epoch),
        Ok(DetectedEntryResult32::Entered { .. })
    ));
}
