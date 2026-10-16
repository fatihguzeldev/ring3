use crate::{
    abi::{arena::*, x86::*},
    cpu::{ExecutionExit, ExitReason},
    memory::{GuestAddress, PageRange, Permissions},
    process::{EngineInstance, ModuleRuntimeConfig32},
};

pub(super) const KEY: u64 = 77;
pub(super) const DATA: u32 = 0x2100_0000;

pub(super) fn engine() -> EngineInstance {
    let mut image = vec![0; 1024];
    image[..2].copy_from_slice(b"MZ");
    let put16 = |bytes: &mut [u8], at: usize, value: u16| {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes())
    };
    let put32 = |bytes: &mut [u8], at: usize, value: u32| {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes())
    };
    put32(&mut image, 0x3c, 0x80);
    image[0x80..0x84].copy_from_slice(b"PE\0\0");
    for (at, value) in [
        (0x84, 0x14c),
        (0x86, 1),
        (0x94, 224),
        (0x96, 0x103),
        (0x98, 0x10b),
    ] {
        put16(&mut image, at, value);
    }
    for (at, value) in [
        (0xa8, 0x1000),
        (0xb4, 0x400000),
        (0xb8, 4096),
        (0xbc, 512),
        (0xd0, 0x2000),
        (0xd4, 512),
        (0xf4, 16),
    ] {
        put32(&mut image, at, value);
    }
    put16(&mut image, 0xdc, 3);
    let section = 0x178;
    image[section..section + 8].copy_from_slice(b".text\0\0\0");
    for (at, value) in [
        (section + 8, 512),
        (section + 12, 4096),
        (section + 16, 512),
        (section + 20, 512),
        (section + 36, 0x60000020),
    ] {
        put32(&mut image, at, value);
    }
    image[512] = 0xc3;
    let mut engine = EngineInstance::new(256, KEY).unwrap();
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine.configure_files(Default::default()).unwrap();
    engine.configure_registry(Default::default()).unwrap();
    engine
        .configure_ole32(crate::process::Ole32Config32 {
            objects: PageRange::new(GuestAddress(0x3100_0000), 8).unwrap(),
            max_objects: 8,
            max_threads: 8,
        })
        .unwrap();
    engine.register_kernel32_module(0x7000_0000).unwrap();
    engine.register_advapi32_module(0x7100_0000).unwrap();
    engine.register_ole32_module(0x7200_0000).unwrap();
    engine.register_winmm_module(0x7300_0000).unwrap();
    engine
        .register_guest_module("guard-service.exe", &image)
        .unwrap();
    engine
        .start_module_process(
            "guard-service.exe",
            None,
            ModuleRuntimeConfig32 {
                stack_base: 0x7f00_0000,
                stack_pages: 4,
                runtime_base: 0x7e00_0000,
            },
        )
        .unwrap();
    for name in ["kernel32.dll", "advapi32.dll", "ole32.dll", "winmm.dll"] {
        engine.load_library_module(name, None).unwrap();
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .map_zeroed(range(DATA, 4), Permissions::READ_WRITE)
        .unwrap();
    let thread = engine
        .current_windows_thread()
        .unwrap()
        .with_last_error(0x1234_5678);
    engine.publish_windows_thread(thread).unwrap();
    engine
}

pub(super) fn range(address: u32, count: u32) -> PageRange {
    PageRange::new(GuestAddress(address), count).unwrap()
}
pub(super) fn put(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(address), bytes)
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
pub(super) fn guard(engine: &mut EngineInstance, address: u32, count: u32) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(range(address, count), Permissions::READ_WRITE.with_guard())
        .unwrap();
}
pub(super) fn capture(engine: &mut EngineInstance, id: u32, args: &[u32]) -> (u32, u32) {
    let gate = *engine
        .module_gates()
        .unwrap()
        .iter()
        .find(|gate| gate.id == id)
        .unwrap();
    for (index, word) in [gate.entry.0, 2, gate.entry.0, id].into_iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    let generation = engine.compile_with_gates(1, 1).unwrap();
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = gate.entry.0;
    state.registers[4] -= 64;
    for (index, value) in std::iter::once(0x401000u32)
        .chain(args.iter().copied())
        .enumerate()
    {
        put(
            engine,
            state.registers[4] + index as u32 * 4,
            &value.to_le_bytes(),
        );
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
    let token = engine.capture_windows_call(KEY, generation).unwrap().token;
    (generation, token)
}
pub(super) fn result(engine: &EngineInstance) -> u32 {
    decode_state(&engine.arena()[..STATE_SIZE])
        .unwrap()
        .registers[0]
}
pub(super) fn call(engine: &mut EngineInstance, id: u32, args: &[u32]) -> u32 {
    let (generation, token) = capture(engine, id, args);
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
    assert!(engine.windows_service.is_none());
    assert!(engine.memory().unwrap().guard_service_fault().is_none());
    result(engine)
}
