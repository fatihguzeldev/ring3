mod cancellation;
mod cleanup;
mod handles;
mod memory;
mod startup;
mod termination;

use crate::{
    abi::{
        arena::{EXIT_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, decode_state, encode_exit_v3, encode_state_current},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::GuestAddress,
    process::EngineInstance,
    windows::{CallingConvention32, synchronization::SynchronizationLimits32},
};
const KEY: u64 = 0x4649_4c45;
const CODE: u32 = 0x8000;
const STACK: u32 = 0x4000;
fn engine() -> EngineInstance {
    engine_with_events(16)
}
fn engine_with_events(events: u32) -> EngineInstance {
    let mut engine = EngineInstance::new(128, KEY).unwrap();
    engine.map(0x1000, 4, 3).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.configure_cpu_clock(1_000_000, 0).unwrap();
    engine
        .configure_synchronization(SynchronizationLimits32 {
            events,
            handles: 32,
            bindings: 32,
            waits: 8,
        })
        .unwrap();
    engine
        .configure_files(crate::process::FileConfig32::default())
        .unwrap();
    engine
}
fn capture(engine: &mut EngineInstance, id: u32, args: &[u32], raw_words: Option<u32>) -> u32 {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(CODE), &[0x0f, 0x0b])
        .unwrap();
    for (index, word) in [CODE, 2, CODE, id].iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    engine.compile_with_gates(1, 1).unwrap();
    for (index, word) in std::iter::once(CODE + 0x100)
        .chain(args.iter().copied())
        .enumerate()
    {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(STACK + index as u32 * 4), &word.to_le_bytes())
            .unwrap();
    }
    let mut state = State32 {
        eip: CODE,
        ..State32::default()
    };
    state.registers[4] = STACK;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    match raw_words {
        Some(words) => {
            engine.capture_call(KEY, engine.generation, CallingConvention32::Stdcall, words)
        }
        None => engine.capture_windows_call(KEY, engine.generation),
    }
    .unwrap()
    .token
}
fn complete(engine: &mut EngineInstance, id: u32, args: &[u32]) -> u32 {
    let token = capture(engine, id, args, None);
    engine
        .complete_windows_call(KEY, engine.generation, token)
        .unwrap();
    assert!(engine.pending_call.is_none());
    decode_state(&engine.arena()[..STATE_SIZE])
        .unwrap()
        .registers[0]
}
fn file(engine: &mut EngineInstance, overlapped: bool) -> u32 {
    super::support::path(
        engine.memory.as_mut().unwrap(),
        0x1000,
        "C:\\integration.bin",
    );
    complete(
        engine,
        crate::windows::files::FileApi32::CreateFileW.id(),
        &[
            0x1000,
            0xc000_0000,
            7,
            0,
            4,
            0x80 | if overlapped { 0x4000_0000 } else { 0 },
            0,
        ],
    )
}
