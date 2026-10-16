use crate::{
    abi::{arena::*, x86::*},
    cpu::{ExecutionExit, ExitReason},
    memory::GuestAddress,
    process::{EngineInstance, ModuleRuntimeConfig32},
    windows::{CallingConvention32, thread::ThreadLimits32},
};
pub(super) const KEY: u64 = 197;

pub(super) fn engine(apcs: u32) -> EngineInstance {
    let mut engine = EngineInstance::new(256, KEY).unwrap();
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine
        .configure_synchronization(Default::default())
        .unwrap();
    engine
        .configure_threads(ThreadLimits32 {
            apcs,
            ..Default::default()
        })
        .unwrap();
    engine.register_kernel32_module(0x70000000).unwrap();
    engine.register_winmm_module(0x71000000).unwrap();
    engine
        .register_guest_module("timers.exe", &image())
        .unwrap();
    engine
        .start_module_process("timers.exe", None, ModuleRuntimeConfig32::default())
        .unwrap();
    engine.load_library_module("kernel32.dll", None).unwrap();
    engine.load_library_module("winmm.dll", None).unwrap();
    engine
}

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
    for (index, word) in [pc, 2, pc, id].into_iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    let generation = engine.compile_with_gates(1, 1).unwrap();
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = pc;
    if returned {
        state.registers[4] += 24;
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
pub(super) fn complete(engine: &mut EngineInstance, id: u32, args: &[u32], returned: bool) {
    let (generation, token) = capture(engine, id, args, returned);
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
}
pub(super) fn word(engine: &EngineInstance, address: u32) -> u32 {
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    u32::from_le_bytes(bytes)
}

pub(super) fn image() -> Vec<u8> {
    let mut bytes = vec![0; 1536];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    for (at, value) in [
        (0x84, 0x14cu16),
        (0x86, 2),
        (0x94, 224),
        (0x96, 0x103),
        (0x98, 0x10b),
        (0xdc, 3),
    ] {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    for (at, value) in [
        (0x3c, 0x80u32),
        (0xa8, 0x1000),
        (0xb4, 0x400000),
        (0xb8, 4096),
        (0xbc, 512),
        (0xd0, 0x4000),
        (0xd4, 512),
        (0xe0, 0x100000),
        (0xe4, 4096),
        (0xf4, 16),
    ] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    for (at, name, rva, raw, flags) in [
        (0x178, b".text\0\0\0", 0x1000u32, 512u32, 0x6000_0020u32),
        (0x1a0, b".data\0\0\0", 0x3000, 1024, 0xc000_0040),
    ] {
        bytes[at..at + 8].copy_from_slice(name);
        for (offset, value) in [(8, 512), (12, rva), (16, 512), (20, raw), (36, flags)] {
            bytes[at + offset..at + offset + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    bytes[512..516].copy_from_slice(&[0x90, 0xc2, 20, 0]);
    bytes
}
