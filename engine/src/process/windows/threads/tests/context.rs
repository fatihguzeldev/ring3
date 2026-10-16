use super::scheduling::{SLEEP, engine};
use crate::{
    abi::{arena::*, x86::*},
    cpu::{ExecutionExit, ExitReason},
    memory::GuestAddress,
    windows::{
        CallingConvention32, WindowsApi32,
        exceptions::context::{
            CONTEXT_CONTROL32, CONTEXT_INTEGER32, CONTEXT_SIZE32, Win32Context32,
        },
        thread::{ThreadApi32 as Api, ThreadSchedule32},
    },
};
const KEY: u64 = 0x5132;
fn write_context(
    engine: &mut crate::process::EngineInstance,
    flags: u32,
    eax: u32,
    eip: u32,
    esp: u32,
) {
    let mut state = engine.windows_context_state().unwrap();
    state.state.registers[0] = eax;
    state.state.eip = eip;
    state.state.registers[4] = esp;
    let context = Win32Context32::capture(flags, &state).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x11000), context.bytes())
        .unwrap();
}
#[test]
fn suspended_context_changes_requested_families_and_get_preserves_unselected_guest_bytes() {
    let (mut engine, _) = engine();
    let handle = engine
        .create_windows_thread_at_gate(&[0, 0, 0x70000, 0, 4, 0], 0x70010)
        .unwrap();
    write_context(&mut engine, CONTEXT_INTEGER32, 0x55, 0, 0);
    engine
        .execute_thread_context(Api::SetThreadContext, &[handle, 0x11000])
        .unwrap();
    let mut output = [0xaa; CONTEXT_SIZE32];
    output[..4].copy_from_slice(&CONTEXT_INTEGER32.to_le_bytes());
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x11500), &output)
        .unwrap();
    engine
        .execute_thread_context(Api::GetThreadContext, &[handle, 0x11500])
        .unwrap();
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x11500), &mut output)
        .unwrap();
    assert_eq!(
        u32::from_le_bytes(output[176..180].try_into().unwrap()),
        0x55
    );
    assert_eq!(&output[28..140], &[0xaa; 112]);
    assert_eq!(&output[180..204], &[0xaa; 24]);
    write_context(&mut engine, CONTEXT_CONTROL32, 0, 0x70000, 0x12345);
    let mut bad = [0; 4];
    bad.copy_from_slice(&0xdeadu32.to_le_bytes());
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x11000 + 188), &bad)
        .unwrap();
    assert!(
        engine
            .execute_thread_context(Api::SetThreadContext, &[handle, 0x11000])
            .is_err()
    );
    assert!(engine.threads.threads[1].context_override.is_none());
}
#[test]
fn blocked_thread_context_override_waits_for_exact_sync_completion_before_guest_reentry() {
    let (mut engine, unit) = engine();
    let handle = engine
        .create_windows_thread_at_gate(&[0, 0, 0x70000, 0, 0, 0], 0x70010)
        .unwrap();
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    let target_esp = state.registers[4];
    state.registers[4] -= 64;
    state.eip = SLEEP;
    engine
        .memory
        .as_mut()
        .unwrap()
        .write_spans(&[
            (GuestAddress(state.registers[4]), &0x70000u32.to_le_bytes()),
            (GuestAddress(state.registers[4] + 4), &1000u32.to_le_bytes()),
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
        .capture_resident_call(KEY, unit, CallingConvention32::Stdcall, 1)
        .unwrap();
    engine
        .complete_resident_windows_call(KEY, unit, call.token)
        .unwrap();
    assert!(engine.pending_call.is_some());
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    write_context(
        &mut engine,
        CONTEXT_CONTROL32 | CONTEXT_INTEGER32,
        0x77,
        0x70001,
        target_esp,
    );
    engine
        .execute_thread_context(Api::SetThreadContext, &[handle, 0x11000])
        .unwrap();
    assert!(engine.threads.threads[1].context_override.is_some());
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    engine.advance_cpu_clock(1_000_000_000).unwrap();
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    assert!(engine.pending_call.is_none());
    assert!(engine.threads.threads[1].context_override.is_none());
    let resumed = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    assert_eq!(resumed.eip, 0x70001);
    assert_eq!(resumed.registers[0], 0x77);
    assert_eq!(resumed.registers[4], target_esp);
}
