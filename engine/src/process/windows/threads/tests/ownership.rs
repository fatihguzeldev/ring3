use super::scheduling::{SLEEP, engine};
use crate::{
    abi::{arena::*, x86::*},
    cpu::{ExecutionExit, ExitReason},
    memory::{GuestAddress, PageRange},
    process::{HostError, windows::threads::ThreadInvocationCompletion32},
    windows::{
        CallingConvention32, WindowsApi32,
        synchronization::{INFINITE, MessageWait32, WaitMode32, WaitOutcome32},
        thread::{ThreadApi32, ThreadSchedule32},
    },
};

const KEY: u64 = 0x5132;

fn compile(engine: &mut crate::process::EngineInstance, gate: bool) -> u32 {
    let words = if gate {
        [SLEEP, 2, SLEEP, WindowsApi32::Sleep.id()]
    } else {
        [0x70000, 4, 0, 0]
    };
    for (index, word) in words.iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    engine.compile_with_gates(1, u32::from(gate)).unwrap()
}

fn yield_call(engine: &mut crate::process::EngineInstance) {
    thread_call(engine, ThreadApi32::SwitchToThread, &[]);
}

fn thread_call(engine: &mut crate::process::EngineInstance, api: ThreadApi32, args: &[u32]) {
    let id = api.id();
    for (index, word) in [SLEEP, 2, SLEEP, id].iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    let generation = engine.compile_with_gates(1, 1).unwrap();
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = SLEEP;
    state.registers[4] = if engine.threads.current_id() == 1 {
        0x11e00
    } else {
        state.registers[4] - 64
    };
    for (index, word) in std::iter::once(0x70000u32)
        .chain(args.iter().copied())
        .enumerate()
    {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(
                GuestAddress(state.registers[4] + index as u32 * 4),
                &word.to_le_bytes(),
            )
            .unwrap();
    }
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = engine
        .capture_call(
            KEY,
            generation,
            CallingConvention32::Stdcall,
            args.len() as u32,
        )
        .unwrap()
        .token;
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
}

#[test]
fn self_suspend_blocks_cpu_until_child_resumes_the_exact_saved_caller_return() {
    let (mut engine, _) = engine();
    let parent = engine.open_windows_thread(1, 2, false).unwrap();
    engine
        .create_windows_thread_at_gate(&[0, 0, 0x70000, 0, 0, 0], 0x70010)
        .unwrap();
    thread_call(&mut engine, ThreadApi32::SuspendThread, &[u32::MAX - 1]);
    let generation = engine.generation();
    assert_eq!(engine.pending_windows_thread_schedule(), Some(1));
    assert_eq!(engine.windows_thread_info(1).unwrap().suspend_count, 1);
    assert!(engine.pending_call.is_none());
    assert!(matches!(
        engine.guard(KEY, generation),
        Err(HostError::Call(crate::process::CallError::Busy))
    ));
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    assert_eq!(
        engine.windows_thread_info(1).unwrap().state,
        crate::windows::thread::ThreadState32::Suspended
    );
    let saved = engine.threads.threads[0].saved.as_ref().unwrap();
    assert!(saved.pending.is_none());
    let returned = decode_state(&saved.arena[..STATE_SIZE]).unwrap();
    assert_eq!(returned.registers[0], 0);
    assert_eq!(returned.registers[4], 0x11e08);
    assert_eq!(returned.eip, 0x70000);
    thread_call(&mut engine, ThreadApi32::ResumeThread, &[parent]);
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        1
    );
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    assert_eq!(engine.generation(), generation);
    engine.guard(KEY, generation).unwrap();
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE]).unwrap(),
        returned
    );
}

#[test]
fn self_suspend_without_another_runnable_thread_stays_blocked_after_scheduler_wait() {
    let (mut engine, _) = engine();
    thread_call(&mut engine, ThreadApi32::SuspendThread, &[u32::MAX - 1]);
    let generation = engine.generation();
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Waiting { deadline_ns: None }
    );
    assert_eq!(engine.pending_windows_thread_schedule(), None);
    assert_eq!(
        engine.windows_thread_info(1).unwrap().state,
        crate::windows::thread::ThreadState32::Suspended
    );
    assert!(matches!(
        engine.guard_dispatch_entry(KEY),
        Err(HostError::Call(crate::process::CallError::Busy))
    ));
    assert!(matches!(
        engine.guard(KEY, generation),
        Err(HostError::Call(crate::process::CallError::Busy))
    ));
}

#[test]
fn switch_to_thread_publishes_exact_caller_return_and_requires_target_scheduling_before_execution()
{
    let (mut engine, _) = engine();
    let child = engine
        .create_windows_thread_at_gate(&[0, 0, 0x70000, 0, 0, 0], 0x70010)
        .unwrap();
    yield_call(&mut engine);
    assert_eq!(engine.threads.current_id(), 2);
    assert!(!engine.threads.threads[1].started);
    assert_eq!(engine.pending_windows_thread_schedule(), Some(2));
    assert!(matches!(
        engine.guard_dispatch_entry(KEY),
        Err(HostError::Call(crate::process::CallError::Busy))
    ));
    let parent = engine.threads.threads[0].saved.as_ref().unwrap();
    assert!(parent.pending.is_none());
    let returned = decode_state(&parent.arena[..STATE_SIZE]).unwrap();
    assert_eq!(returned.registers[0], 1);
    assert_eq!(returned.registers[4], 0x11e04);
    assert_eq!(returned.eip, 0x70000);
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    assert!(engine.threads.threads[1].started);
    assert_eq!(engine.pending_windows_thread_schedule(), None);
    engine.guard_dispatch_entry(KEY).unwrap();
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    assert_eq!(engine.change_thread_suspend(child, false).unwrap(), 0);
    yield_call(&mut engine);
    assert_eq!(engine.threads.current_id(), 1);
    assert_eq!(engine.pending_windows_thread_schedule(), None);
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        0
    );
}

#[test]
fn parked_replacement_wait_preserves_its_artifact_across_child_compilation_and_unique_generations()
{
    let (mut engine, _) = engine();
    engine
        .create_windows_thread_at_gate(&[0, 0, 0x70000, 0, 0, 0], 0x70010)
        .unwrap();
    let parent_generation = compile(&mut engine, true);
    let parent_artifact = engine.artifact_bytes().unwrap().to_vec();
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = SLEEP;
    state.registers[4] = 0x11e00;
    engine
        .memory
        .as_mut()
        .unwrap()
        .write_spans(&[
            (GuestAddress(0x11e00), &0x70000u32.to_le_bytes()),
            (GuestAddress(0x11e04), &1000u32.to_le_bytes()),
        ])
        .unwrap();
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate {
                id: WindowsApi32::Sleep.id(),
            },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let call = engine
        .capture_call(KEY, parent_generation, CallingConvention32::Stdcall, 1)
        .unwrap();
    engine
        .complete_windows_call(KEY, parent_generation, call.token)
        .unwrap();
    assert!(engine.pending_call.is_some());
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    assert_eq!(engine.generation(), 0);
    let child_generation = compile(&mut engine, false);
    assert!(child_generation > parent_generation);
    assert!(matches!(
        engine.complete_windows_call(KEY, parent_generation, call.token),
        Err(HostError::InvalidArtifact)
    ));
    engine.advance_cpu_clock(1_000_000_000).unwrap();
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    assert!(engine.pending_call.is_none());
    assert_eq!(engine.generation(), parent_generation);
    assert_eq!(engine.artifact_bytes().unwrap(), parent_artifact);
    assert!(matches!(
        engine.guard_artifact(KEY, child_generation),
        Err(HostError::InvalidArtifact)
    ));
    assert!(compile(&mut engine, false) > child_generation);
}

#[test]
fn sent_arrival_projects_only_target_queue_without_interrupting_an_unpumped_thread() {
    let (mut engine, _) = engine();
    engine
        .configure_user32(
            Default::default(),
            PageRange::new(GuestAddress(0x30000000), 8).unwrap(),
        )
        .unwrap();
    engine
        .create_windows_thread_at_gate(&[0, 0, 0x70000, 0, 0, 0], 0x70010)
        .unwrap();
    let sync = engine.synchronization_mut().unwrap();
    sync.set_current_thread(2).unwrap();
    let wait = sync
        .wait(
            &[],
            WaitMode32::Any,
            INFINITE,
            false,
            Some(MessageWait32 {
                mask: 0x40,
                input_available: false,
            }),
        )
        .unwrap();
    let WaitOutcome32::Pending(wait) = wait else {
        panic!("target queue starts empty");
    };
    sync.set_current_thread(1).unwrap();
    let revision = sync.revision();
    sync.set_revision_for_test(u64::MAX);
    assert!(
        engine
            .prepare_thread_invocation(
                2,
                GuestAddress(0x70000),
                &[1, 2, 3, 4],
                ThreadInvocationCompletion32::User32 {
                    sender: 1,
                    continuation: 9,
                },
            )
            .is_err()
    );
    assert!(engine.threads.invocations.is_empty());
    assert_eq!(
        engine.user32.as_ref().unwrap().wait_queue_status(2),
        (0, 0, 0)
    );
    engine
        .synchronization_mut()
        .unwrap()
        .set_revision_for_test(revision);
    let prepared = engine
        .prepare_thread_invocation(
            2,
            GuestAddress(0x70000),
            &[1, 2, 3, 4],
            ThreadInvocationCompletion32::User32 {
                sender: 1,
                continuation: 9,
            },
        )
        .unwrap();
    let invocation = engine.commit_thread_invocation(prepared);
    assert_eq!(
        engine.user32.as_ref().unwrap().wait_queue_status(2),
        (0x40, 0x40, 1)
    );
    assert_eq!(
        engine.user32.as_ref().unwrap().wait_queue_status(1),
        (0, 0, 1)
    );
    assert!(
        engine
            .synchronization()
            .unwrap()
            .wait_status(wait)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        engine.synchronization().unwrap().queue_state(2),
        (0x40, 0x40, 1)
    );
    assert_eq!(engine.synchronization().unwrap().queue_state(1), (0, 0, 0));
    assert_eq!(engine.threads.invocations[0].id, invocation);
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    assert!(!engine.begin_pending_thread_invocation().unwrap());
    assert_eq!(engine.threads.invocations[0].id, invocation);
    assert_eq!(
        engine.user32.as_ref().unwrap().wait_queue_status(2),
        (0x40, 0x40, 1)
    );
}
