use crate::{
    abi::{
        arena::{
            EXIT_OFFSET, SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET, SIMD_OFFSET, STATE_OFFSET,
            TRANSFER_OFFSET, X87_OFFSET, YMM_OFFSET,
        },
        callback::CALLBACK_RECORD_SIZE,
        x86::{
            EXIT_SIZE, SEGMENT_BASES_RECORD_SIZE, SEGMENTS_SIZE, SIMD_SIZE, STATE_SIZE, X87_SIZE,
            YMM_SIZE, encode_exit_v3, encode_state_current,
        },
    },
    cpu::{
        ExecutionExit, ExitReason,
        x86::{
            SegmentDescriptorRecord, State32,
            tasks::{TaskControlState32, TaskIncomingContext32, TaskRegister, TaskSave},
        },
    },
    memory::GuestAddress,
    process::{
        CallError, EngineInstance, HostError, TaskReturnTransition32, cpu::tasks::TaskReturnSources,
    },
    windows::CallingConvention32,
};

const KEY: u64 = 0x4342_5453;
const PAGES: [u32; 4] = [0x1000, 0x5000, 0x6000, 0x8000];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Authority {
    sources: Option<TaskReturnSources>,
    controls: Option<TaskControlState32>,
    revision: Option<u64>,
    incoming: Option<TaskIncomingContext32>,
    tr: Option<u16>,
    descriptors: [Option<SegmentDescriptorRecord>; 2],
}
fn authority(e: &EngineInstance) -> Authority {
    Authority {
        sources: e.task_return_sources,
        controls: e.task_control_state().unwrap(),
        revision: e.task_execution_revision(),
        incoming: e.task_incoming,
        tr: e.task_system_register_bank().and_then(|bank| bank.tr),
        descriptors: [0x40, 0x48]
            .map(|selector| e.segment_descriptors.raw_descriptor(selector).copied()),
    }
}
fn ram(e: &EngineInstance) -> [Vec<u8>; 4] {
    PAGES.map(|address| {
        let mut bytes = vec![0; 4096];
        e.memory()
            .unwrap()
            .read(GuestAddress(address), &mut bytes)
            .unwrap();
        bytes
    })
}
fn upload(e: &mut EngineInstance, address: u32, bytes: &[u8]) {
    e.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()].copy_from_slice(bytes);
    e.upload(address, bytes.len() as u32).unwrap();
}
fn tss(base: u32) -> SegmentDescriptorRecord {
    SegmentDescriptorRecord::from_words(
        (base << 16) | 0x67,
        ((base >> 16) & 0xff) | (base & 0xff000000) | 0x8b00,
    )
}
fn admitted_callback() -> (EngineInstance, u32, u32, Vec<u8>, Authority) {
    let mut e = EngineInstance::new(4, KEY).unwrap();
    for page in PAGES {
        e.map(page, 1, 7).unwrap();
    }
    e.configure_task_return_sources(TaskRegister::from_cached(0x43, tss(0x5000)).unwrap(), 0x57)
        .unwrap();
    e.configure_segment_descriptor_raw(0x40, tss(0x5000))
        .unwrap();
    e.configure_segment_descriptor_raw(0x48, tss(0x6000))
        .unwrap();
    e.configure_segment_descriptor_raw(0x50, SegmentDescriptorRecord::from_words(0, 0x8200))
        .unwrap();
    e.configure_task_controls(
        TaskControlState32::new(0x33, 0xdeadb123, 0, 0x400, Some(true)).unwrap(),
    )
    .unwrap();
    e.write32(0x5000, 0x4b).unwrap();
    let mut incoming = [0; 104];
    for (offset, value) in [
        (32, 0x1200_u32),
        (36, 2),
        (40, 0x0badf00d),
        (56, 0x8010),
        (96, 0x50),
    ] {
        incoming[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    for (index, selector) in [0x23_u16, 0x1b, 0x23, 0x23, 0x3b, 0]
        .into_iter()
        .enumerate()
    {
        incoming[72 + index * 4..74 + index * 4].copy_from_slice(&selector.to_le_bytes());
    }
    upload(&mut e, 0x6000, &incoming);
    for (address, code) in [
        (0x1000, &[0x0f, 0x0b][..]),
        (0x1100, &[0xcf][..]),
        (0x1200, &[0x0f, 0x0b][..]),
    ] {
        upload(&mut e, address, code);
    }
    for (index, value) in [0x1000_u32, 2, 0x1100, 1, 0x1200, 2, 0x1000, 17, 0x1200, 18]
        .into_iter()
        .enumerate()
    {
        e.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    let generation = e.compile_with_gates(3, 2).unwrap();
    e.write32(0x8010, 0x1300).unwrap();
    let state = State32 {
        registers: [0x11223344, 0, 0, 0, 0x8010, 0, 0, 0],
        eip: 0x1000,
        eflags: 0x4002,
    };
    encode_state_current(
        &state,
        &mut e.arena_mut().unwrap()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE],
    )
    .unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::Gate { id: 17 },
        },
        &mut e.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let outer = e
        .capture_call(KEY, generation, CallingConvention32::Cdecl, 0)
        .unwrap();
    let outer_image = e.arena().to_vec();
    let original_authority = authority(&e);
    let callback = e
        .begin_callback(KEY, generation, outer.token, 0x1100, 0x1200, 18, &[])
        .unwrap();
    assert_eq!(e.guard_material(KEY, generation), Ok(()));
    assert!(e.pending_call.is_none() && e.callback.is_some());
    (
        e,
        generation,
        callback.token,
        outer_image,
        original_authority,
    )
}
fn fixture() -> (EngineInstance, u32, u32, Vec<u8>, Authority) {
    let (mut e, generation, token, outer_image, original_authority) = admitted_callback();
    let state =
        crate::abi::x86::decode_state(&e.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]).unwrap();
    let outgoing = TaskSave {
        next_eip: 0x1101,
        flags: state.eflags,
        general: state.registers,
        selectors: e.segment_context().unwrap().state.selectors,
    };
    let TaskReturnTransition32::Committed(committed) = e.task_return_transition(outgoing).unwrap()
    else {
        panic!("admitted callback task return must commit")
    };
    assert_eq!(committed.context.fault(), None);
    assert_eq!(committed.context.loaded_mask(), 0x3f);
    assert_eq!(committed.context.state().eip, 0x1200);
    assert_eq!(committed.context.state().registers[4], 0x8010);
    let now = authority(&e);
    assert_eq!(now.tr, Some(0x4b));
    assert_eq!(now.revision, Some(2));
    assert_eq!(
        (now.controls.unwrap().cr0(), now.controls.unwrap().ldtr()),
        (0x3b, 0x50)
    );
    assert_ne!(now, original_authority);
    assert!(e.artifact_bytes().is_ok());
    assert_eq!(
        e.guard_material(KEY, generation),
        Err(HostError::InvalidArtifact)
    );
    (e, generation, token, outer_image, original_authority)
}
fn outer_restored(mut current: Vec<u8>, outer: &[u8]) -> Vec<u8> {
    for (offset, size) in [
        (STATE_OFFSET, STATE_SIZE),
        (EXIT_OFFSET, EXIT_SIZE),
        (X87_OFFSET, X87_SIZE),
        (SIMD_OFFSET, SIMD_SIZE),
        (YMM_OFFSET, YMM_SIZE),
        (SEGMENTS_OFFSET, SEGMENTS_SIZE),
        (SEGMENT_BASES_RECORD_OFFSET, SEGMENT_BASES_RECORD_SIZE),
    ] {
        current[offset..offset + size].copy_from_slice(&outer[offset..offset + size]);
    }
    current
}
#[allow(clippy::too_many_arguments)]
fn verify_restore(
    e: &EngineInstance,
    result: Result<(), HostError>,
    before: &[u8],
    original: &[u8],
    original_authority: Authority,
    current_authority: Authority,
    current_ram: [Vec<u8>; 4],
    completed: bool,
) {
    match result {
        Ok(()) => {
            let mut expected = outer_restored(before.to_vec(), original);
            if completed {
                let words = [
                    2_u32, 1, 2, 0, 0x1100, 0x800c, 0x1200, 18, 0, 0x0badf00d, 1, 0,
                ];
                let output = &mut expected[TRANSFER_OFFSET..TRANSFER_OFFSET + CALLBACK_RECORD_SIZE];
                output.fill(0);
                output[..16].copy_from_slice(&[
                    0x52, 0x33, 0x43, 0x42, 1, 0, 1, 0, 64, 0, 0, 0, 0, 0, 0, 0,
                ]);
                for (index, word) in words.into_iter().enumerate() {
                    output[16 + index * 4..20 + index * 4].copy_from_slice(&word.to_le_bytes());
                }
            }
            assert_eq!(e.arena(), expected);
            assert_eq!(
                authority(e),
                original_authority,
                "outer arena restoration must not retain a different task authority"
            );
        }
        Err(error) => {
            assert_eq!(error, HostError::Call(CallError::StateChanged));
            assert_eq!(e.arena(), before);
            assert_eq!(authority(e), current_authority);
            assert!(e.callback.is_some() && e.pending_call.is_none());
        }
    }
    assert_eq!(ram(e), current_ram);
}
#[test]
fn callback_abort_does_not_mix_outer_arena_with_committed_task_authority() {
    let (mut e, _, token, outer, original_authority) = fixture();
    let before = e.arena().to_vec();
    let current_authority = authority(&e);
    let memory = ram(&e);
    let result = e.abort_callback(KEY, token);
    verify_restore(
        &e,
        result,
        &before,
        &outer,
        original_authority,
        current_authority,
        memory,
        false,
    );
}
#[test]
fn callback_completion_does_not_mix_outer_arena_with_committed_task_authority() {
    let (mut e, generation, token, outer, original_authority) = fixture();
    // native fixtures supply a validated gate stop; guest execution is a separate actual proof.
    encode_exit_v3(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::Gate { id: 18 },
        },
        &mut e.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let before = e.arena().to_vec();
    let current_authority = authority(&e);
    let memory = ram(&e);
    let result = e.finish_callback(KEY, generation, token).map(|record| {
        assert_eq!((record.phase, record.result), (2, 0x0badf00d));
    });
    verify_restore(
        &e,
        result,
        &before,
        &outer,
        original_authority,
        current_authority,
        memory,
        true,
    );
}

#[test]
fn unchanged_task_callback_completion_and_stale_code_abort_remain_available() {
    for completed in [false, true] {
        let (mut e, generation, token, outer, saved_authority) = admitted_callback();
        if completed {
            encode_state_current(
                &State32 {
                    registers: [0x0badf00d, 0, 0, 0, 0x8010, 0, 0, 0],
                    eip: 0x1200,
                    eflags: 2,
                },
                &mut e.arena_mut().unwrap()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE],
            )
            .unwrap();
            encode_exit_v3(
                &ExecutionExit {
                    retired: 1,
                    reason: ExitReason::Gate { id: 18 },
                },
                &mut e.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
            )
            .unwrap();
        } else {
            upload(&mut e, 0x1100, &[0x90]);
            assert!(e.artifact_bytes().is_err());
        }
        let before = e.arena().to_vec();
        let memory = ram(&e);
        let result = if completed {
            e.finish_callback(KEY, generation, token).map(|_| ())
        } else {
            e.abort_callback(KEY, token)
        };
        assert!(result.is_ok());
        verify_restore(
            &e,
            result,
            &before,
            &outer,
            saved_authority,
            saved_authority,
            memory,
            completed,
        );
        assert!(e.pending_call.is_some() && e.callback.is_none());
    }
}
