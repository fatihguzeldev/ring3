use crate::{
    abi::{
        arena::{EXIT_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, decode_state, encode_exit_v3, encode_state_current},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::{GuestAddress, MemoryError},
    process::{CallError, EngineInstance, HostError},
    windows::{
        WindowsApi32,
        synchronization::{
            EVENT_ALL_ACCESS, INFINITE, SynchronizationError32 as Error, SynchronizationLimits32,
            WAIT_TIMEOUT,
        },
    },
};
const KEY: u64 = 0x5132;
const STACK: u32 = 0x11000;
const CODE: u32 = 0x20000;
const OUTPUT: u32 = 0x10000;

fn fixture() -> EngineInstance {
    let mut engine = EngineInstance::new(8, KEY).unwrap();
    engine.map(0x10000, 2, 3).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    let apis: Vec<_> = WindowsApi32::catalog()
        .filter(|api| api.id() >= 0x0001_0011)
        .collect();
    for (index, _) in apis.iter().enumerate() {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(CODE + index as u32 * 16), &[0x0f, 0x0b])
            .unwrap();
    }
    let words = [CODE, 2, CODE, WindowsApi32::CreateEventA.id()];
    for (index, word) in words.iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    engine.compile_with_gates(1, 1).unwrap();
    engine.configure_cpu_clock(1_000_000, 0).unwrap();
    engine
        .configure_synchronization(SynchronizationLimits32 {
            events: 8,
            handles: 16,
            bindings: 16,
            waits: 8,
        })
        .unwrap();
    engine
}
fn capture(engine: &mut EngineInstance, api: WindowsApi32, args: &[u32]) -> u32 {
    assert_eq!(args.len(), api.stack_words() as usize);
    let pc = CODE + (api.id() - 0x0001_0011) * 16;
    for (index, word) in [pc, 2, pc, api.id()].iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    engine.compile_with_gates(1, 1).unwrap();
    for (index, value) in std::iter::once(0x20900)
        .chain(args.iter().copied())
        .enumerate()
    {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(STACK + index as u32 * 4), &value.to_le_bytes())
            .unwrap();
    }
    let mut state = State32 {
        eip: CODE + (api.id() - 0x0001_0011) * 16,
        ..Default::default()
    };
    state.registers[4] = STACK;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: api.id() },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    engine
        .capture_windows_call(KEY, engine.generation)
        .unwrap()
        .token
}
fn complete(engine: &mut EngineInstance, token: u32) -> Result<(), HostError> {
    engine.complete_windows_call(KEY, engine.generation, token)
}
fn call(engine: &mut EngineInstance, api: WindowsApi32, args: &[u32]) -> u32 {
    let token = capture(engine, api, args);
    complete(engine, token).unwrap();
    assert!(engine.pending_call.is_none());
    decode_state(&engine.arena()[..STATE_SIZE])
        .unwrap()
        .registers[0]
}

mod calls;
mod critical;
