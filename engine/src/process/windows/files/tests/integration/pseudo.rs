use super::*;
use crate::{
    process::{HostError, windows::files::tests::support},
    windows::{
        WindowsApi32 as Api,
        memory::MemoryApi32,
        process::ProcessApi32,
        synchronization::{SynchronizationError32, WAIT_TIMEOUT, WaitMode32, WaitOutcome32},
        thread::{ThreadApi32, ThreadSchedule32, ThreadState32},
    },
};

fn duplicate(
    engine: &mut EngineInstance,
    source: u32,
    access: u32,
    inherit: u32,
    flags: u32,
) -> u32 {
    assert_eq!(
        complete(
            engine,
            Api::DuplicateHandle.id(),
            &[u32::MAX, source, u32::MAX, 0x1200, access, inherit, flags]
        ),
        1
    );
    support::dword(engine.memory().unwrap(), 0x1200)
}

fn thread_engine() -> EngineInstance {
    let mut engine = EngineInstance::new(128, KEY).unwrap();
    engine.register_kernel32_module(0x70000000).unwrap();
    engine
        .register_guest_module(
            "main.exe",
            &crate::process::windows::process::test_image_for_startup(),
        )
        .unwrap();
    engine
        .start_module_process("main.exe", None, Default::default())
        .unwrap();
    engine.load_library_module("kernel32.dll", None).unwrap();
    engine.map(0x1000, 4, 3).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.configure_cpu_clock(1_000_000, 0).unwrap();
    engine
        .configure_synchronization(SynchronizationLimits32 {
            events: 16,
            handles: 32,
            bindings: 32,
            waits: 8,
        })
        .unwrap();
    engine.configure_files(Default::default()).unwrap();
    engine
}

fn park_child_resident_wait(engine: &mut EngineInstance) -> u64 {
    assert_ne!(
        complete(
            engine,
            ThreadApi32::CreateThread.id(),
            &[0, 0, CODE, 0, 0, 0]
        ),
        0
    );
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    const WAIT: u32 = 0x9000;
    engine.map(WAIT, 1, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(WAIT), &[0x0f, 0x0b])
        .unwrap();
    for (index, word) in [WAIT, 2, WAIT, Api::Sleep.id()].iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    let unit = engine.compile_resident_with_gates(1, 1).unwrap().get();
    let child = engine.windows_thread_info(2).unwrap();
    let stack = child.stack_base + child.stack_pages * 4096 - 64;
    engine
        .memory
        .as_mut()
        .unwrap()
        .write_spans(&[
            (GuestAddress(stack), &(CODE + 0x100).to_le_bytes()),
            (GuestAddress(stack + 4), &u32::MAX.to_le_bytes()),
        ])
        .unwrap();
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = WAIT;
    state.registers[4] = stack;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate {
                id: Api::Sleep.id(),
            },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = engine
        .capture_resident_call(KEY, unit, CallingConvention32::Stdcall, 1)
        .unwrap()
        .token;
    engine
        .complete_resident_windows_call(KEY, unit, token)
        .unwrap();
    assert!(engine.pending_call.is_some());
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    assert!(engine.threads.execution_pins().any(|pin| pin == unit));
    unit
}

#[test]
fn current_thread_pseudo_duplicate_is_a_real_shared_sync_object_with_files_configured() {
    let mut engine = thread_engine();
    let pseudo = complete(&mut engine, ProcessApi32::GetCurrentThread.id(), &[]);
    assert_eq!(pseudo, u32::MAX - 1);
    let alias = duplicate(&mut engine, pseudo, 0, 1, 2);
    assert!(alias < 0x80000000);
    assert_eq!(
        engine
            .synchronization()
            .unwrap()
            .validate_thread_access(alias, 0x40)
            .unwrap(),
        1
    );
    assert!(
        engine
            .synchronization()
            .unwrap()
            .handle_inheritable(alias)
            .unwrap()
    );
    assert_eq!(
        complete(&mut engine, Api::WaitForSingleObject.id(), &[alias, 0]),
        WAIT_TIMEOUT
    );
    assert_ne!(
        complete(
            &mut engine,
            ThreadApi32::CreateThread.id(),
            &[0, 0, CODE, 0, 0, 0]
        ),
        0
    );
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    assert_eq!(engine.resolve_thread_handle(pseudo, 0x40).unwrap(), 2);
    assert_eq!(
        complete(
            &mut engine,
            ThreadApi32::GetExitCodeThread.id(),
            &[alias, 0x1300]
        ),
        1
    );
    assert_eq!(support::dword(engine.memory().unwrap(), 0x1300), 259);
    let revision = engine.synchronization().unwrap().revision();
    assert_eq!(complete(&mut engine, Api::CloseHandle.id(), &[pseudo]), 1);
    assert_eq!(engine.synchronization().unwrap().revision(), revision);
    assert_eq!(complete(&mut engine, Api::CloseHandle.id(), &[alias]), 1);
    assert_eq!(
        complete(&mut engine, Api::WaitForSingleObject.id(), &[alias, 0]),
        u32::MAX
    );
    assert_eq!(engine.windows_thread.last_error(), 6);
}

#[test]
fn pseudo_duplicate_output_failure_publishes_no_primary_thread_object_or_handle() {
    let mut engine = engine();
    let token = capture(
        &mut engine,
        Api::DuplicateHandle.id(),
        &[u32::MAX, u32::MAX - 1, u32::MAX, 0x5555, 0, 0, 2],
        None,
    );
    let revision = engine.synchronization().unwrap().revision();
    assert!(matches!(
        engine.complete_windows_call(KEY, engine.generation, token),
        Err(HostError::Memory(_))
    ));
    assert_eq!(engine.synchronization().unwrap().revision(), revision);
    assert!(!engine.synchronization().unwrap().has_thread_object(1));
    assert!(engine.pending_call.is_some());
    engine.abandon_call(KEY, token).unwrap();
    let alias = duplicate(&mut engine, u32::MAX - 1, 0x40, 0, 0);
    assert_eq!(
        complete(&mut engine, Api::WaitForSingleObject.id(), &[alias, 0]),
        u32::MAX
    );
    assert_eq!(engine.windows_thread.last_error(), 5);
    assert_eq!(
        complete(
            &mut engine,
            ThreadApi32::GetExitCodeThread.id(),
            &[alias, 0x1300]
        ),
        1
    );
}

#[test]
fn real_current_process_handle_preserves_rights_across_memory_file_duplication_and_exit() {
    let mut engine = thread_engine();
    let process = duplicate(&mut engine, u32::MAX, 0x0010_0448, 1, 0);
    assert_eq!(
        engine
            .synchronization()
            .unwrap()
            .validate_process_access(process, 0x40)
            .unwrap(),
        engine.current_windows_process_id()
    );
    let file = file(&mut engine, false);
    assert_eq!(
        complete(
            &mut engine,
            Api::DuplicateHandle.id(),
            &[process, file, process, 0x1200, 0, 0, 2]
        ),
        1
    );
    let alias = support::dword(engine.memory().unwrap(), 0x1200);
    assert_eq!(
        engine.files.as_ref().unwrap().handles.file(alias).unwrap(),
        engine.files.as_ref().unwrap().handles.file(file).unwrap()
    );
    let address = complete(
        &mut engine,
        MemoryApi32::VirtualAllocEx.id(),
        &[process, 0, 4096, 0x3000, 4],
    );
    assert_ne!(address, 0);
    assert_eq!(
        complete(
            &mut engine,
            MemoryApi32::VirtualQueryEx.id(),
            &[process, address, 0x1300, 28]
        ),
        28
    );
    assert_eq!(
        complete(
            &mut engine,
            ProcessApi32::CheckRemoteDebuggerPresent.id(),
            &[process, 0x1300]
        ),
        1
    );
    assert_eq!(support::dword(engine.memory().unwrap(), 0x1300), 0);
    let narrow = duplicate(&mut engine, u32::MAX, 0x0010_0000, 0, 0);
    assert_eq!(
        complete(
            &mut engine,
            MemoryApi32::VirtualAllocEx.id(),
            &[narrow, 0, 4096, 0x3000, 4]
        ),
        0
    );
    assert_eq!(engine.windows_thread.last_error(), 5);
    assert_eq!(
        complete(&mut engine, Api::WaitForSingleObject.id(), &[process, 0]),
        WAIT_TIMEOUT
    );
    let revision = engine.synchronization().unwrap().revision();
    assert_eq!(complete(&mut engine, Api::CloseHandle.id(), &[u32::MAX]), 1);
    assert_eq!(engine.synchronization().unwrap().revision(), revision);
    let parked = park_child_resident_wait(&mut engine);
    let token = capture(&mut engine, Api::ExitProcess.id(), &[17], None);
    let revision = engine.synchronization().unwrap().revision();
    engine
        .synchronization_mut()
        .unwrap()
        .set_revision_for_test(u64::MAX);
    assert_eq!(
        engine.complete_windows_call(KEY, engine.generation, token),
        Err(HostError::Synchronization(
            SynchronizationError32::Exhausted
        ))
    );
    assert_eq!(engine.exit_code, None);
    assert_eq!(
        engine.threads.info(2).unwrap().state,
        ThreadState32::Waiting
    );
    assert!(engine.threads.execution_pins().any(|pin| pin == parked));
    assert!(engine.files.as_ref().unwrap().handles.file(file).is_ok());
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    engine
        .synchronization_mut()
        .unwrap()
        .set_revision_for_test(revision);
    engine
        .complete_windows_call(KEY, engine.generation, token)
        .unwrap();
    assert_eq!(engine.exit_code, Some(17));
    for thread in [1, 2] {
        let info = engine.threads.info(thread).unwrap();
        assert_eq!(info.state, ThreadState32::Terminated);
        assert_eq!(info.exit_code, 17);
    }
    assert!(!engine.threads.execution_pins().any(|pin| pin == parked));
    for alias in [process, narrow] {
        let result = engine
            .synchronization
            .as_mut()
            .unwrap()
            .context_mut()
            .wait(&[alias], WaitMode32::Any, 0, false, None)
            .unwrap();
        let WaitOutcome32::Ready(result) = result else {
            panic!("process aliases must remain signaled after exit");
        };
        assert_eq!(result.value, 0);
    }
}
