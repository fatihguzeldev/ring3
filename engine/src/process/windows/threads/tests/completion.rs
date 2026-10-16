use super::timers::{call, capture};
use crate::{
    abi::x86::*,
    memory::GuestAddress,
    process::{EngineInstance, ModuleRuntimeConfig32},
    windows::{
        files::FileApi32 as File, synchronization::SynchronizationApi32 as Sync,
        thread::ThreadSchedule32,
    },
};
const KEY: u64 = 83;
#[test]
fn alertable_completion_port_wait_runs_real_apc_then_restores_exact_file_reply_without_sync_wait() {
    let mut engine = EngineInstance::new(256, KEY).unwrap();
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine
        .configure_synchronization(Default::default())
        .unwrap();
    engine.configure_files(Default::default()).unwrap();
    engine.register_kernel32_module(0x70000000).unwrap();
    engine
        .register_guest_module(
            "iocp.exe",
            &super::static_tls::image(0x400000, false, false),
        )
        .unwrap();
    engine
        .start_module_process("iocp.exe", None, ModuleRuntimeConfig32::default())
        .unwrap();
    engine.load_library_module("kernel32.dll", None).unwrap();
    let port = call(
        &mut engine,
        File::CreateIoCompletionPort.id(),
        &[u32::MAX, 0, 0, 1],
    );
    assert_ne!(port, 0);
    assert_eq!(
        call(
            &mut engine,
            Sync::QueueUserAPC.id(),
            &[0x401000, 0xfffffffe, 0xfeed]
        ),
        1
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x403000), &[0xa5; 16])
        .unwrap();
    let (generation, token) = capture(
        &mut engine,
        File::GetQueuedCompletionStatusEx.id(),
        &[port, 0x403000, 1, 0x403100, u32::MAX, 1],
        false,
    );
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
    assert!(engine.pending_call.is_some());
    assert!(engine.pending_windows_wait().is_none());
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    assert!(engine.pending_call.is_none());
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE]).unwrap().eip,
        0x401000
    );
    let state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    let mut value = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(state.registers[4] + 4), &mut value)
        .unwrap();
    assert_eq!(u32::from_le_bytes(value), 0xfeed);
    let (callback_generation, callback_token) =
        capture(&mut engine, super::super::INVOCATION_RETURN_ID, &[], true);
    engine
        .complete_windows_call(KEY, callback_generation, callback_token)
        .unwrap();
    assert_eq!(engine.generation(), generation);
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    assert!(engine.pending_call.is_none());
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        0
    );
    assert_eq!(engine.current_windows_thread().unwrap().last_error(), 192);
    let mut entries = [0; 16];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x403000), &mut entries)
        .unwrap();
    assert_eq!(entries, [0xa5; 16]);
    assert!(engine.pending_windows_wait().is_none());
    assert!(engine.threads.threads[0].active_invocations.is_empty());
}
