use crate::{
    abi::{arena::*, x86::*},
    cpu::{ExecutionExit, ExitReason},
    memory::GuestAddress,
    process::{EngineInstance, ModuleRuntimeConfig32},
    windows::{
        CallingConvention32, WindowsApi32, synchronization::SynchronizationApi32 as Api,
        thread::ThreadSchedule32,
    },
};

const KEY: u64 = 83;
pub(super) fn capture(
    engine: &mut EngineInstance,
    id: u32,
    args: &[u32],
    returned: bool,
) -> (u32, u32) {
    let pc = engine
        .module_gates()
        .unwrap()
        .into_iter()
        .find(|gate| gate.id == id)
        .unwrap()
        .entry
        .0;
    for (index, word) in [pc, 2, pc, id].iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    let generation = engine.compile_with_gates(1, 1).unwrap();
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = pc;
    if returned {
        state.registers[4] = engine.threads.threads[0]
            .active_invocations
            .last()
            .unwrap()
            .return_esp;
    } else {
        let stack = state.registers[4] - 64;
        for (index, word) in std::iter::once(0x401000)
            .chain(args.iter().copied())
            .enumerate()
        {
            engine
                .memory
                .as_mut()
                .unwrap()
                .write(GuestAddress(stack + index as u32 * 4), &word.to_le_bytes())
                .unwrap();
        }
        state.registers[4] = stack;
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
    (generation, token)
}
pub(super) fn call(engine: &mut EngineInstance, id: u32, args: &[u32]) -> u32 {
    let (generation, token) = capture(engine, id, args, false);
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
    decode_state(&engine.arena()[..STATE_SIZE])
        .unwrap()
        .registers[0]
}

#[test]
fn cancellation_inside_running_timer_apc_preserves_alertable_wait_completion_and_outer_artifact() {
    let mut engine = EngineInstance::new(256, KEY).unwrap();
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine
        .configure_synchronization(Default::default())
        .unwrap();
    engine.register_kernel32_module(0x70000000).unwrap();
    let mut image = super::static_tls::image(0x400000, false, false);
    image[512..516].copy_from_slice(&[0x90, 0xc2, 12, 0]);
    engine.register_guest_module("timer.exe", &image).unwrap();
    engine
        .start_module_process("timer.exe", None, ModuleRuntimeConfig32::default())
        .unwrap();
    engine.load_library_module("kernel32.dll", None).unwrap();
    let timer = call(&mut engine, Api::CreateWaitableTimerW.id(), &[0, 0, 0]);
    assert_ne!(timer, 0);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x403000), &(-10_000i64).to_le_bytes())
        .unwrap();
    assert_eq!(
        call(
            &mut engine,
            Api::SetWaitableTimer.id(),
            &[timer, 0x403000, 0, 0x401000, 0xabcdef, 0]
        ),
        1
    );
    let (outer_generation, token) =
        capture(&mut engine, WindowsApi32::SleepEx.id(), &[1000, 1], false);
    engine
        .complete_windows_call(KEY, outer_generation, token)
        .unwrap();
    assert!(engine.pending_call.is_some());
    engine.advance_cpu_clock(1_000_000).unwrap();
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    assert!(engine.pending_call.is_none());
    assert_eq!(engine.threads.threads[0].active_invocations.len(), 1);
    assert_eq!(
        call(&mut engine, Api::CancelWaitableTimer.id(), &[timer]),
        1
    );
    let (generation, token) = capture(&mut engine, super::super::INVOCATION_RETURN_ID, &[], true);
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
    assert_eq!(engine.generation(), outer_generation);
    assert!(engine.pending_call.is_some());
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    assert!(engine.pending_call.is_none());
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        0xc0
    );
    assert!(engine.threads.threads[0].active_invocations.is_empty());
    assert_eq!(engine.cpu_clock.monotonic_bounds().unwrap().1, 1_000_000);
}
