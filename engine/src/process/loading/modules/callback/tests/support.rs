use crate::{
    abi::{arena::*, x86::*},
    cpu::{ExecutionExit, ExitReason},
    memory::{GuestAddress, PageRange},
    process::{EngineInstance, ModuleInitializer32, ModuleOperation32, ModuleRuntimeConfig32},
    windows::{
        WindowsApi32,
        user32::{User32Api32, User32Limits32},
    },
};

pub(super) const KEY: u64 = 77;
pub(super) const EXE: u32 = 0x400000;
pub(super) const DLL: u32 = 0x600000;
pub(super) const WNDPROC: u32 = EXE + 0x1010;
pub(super) const RETURN_PC: u32 = EXE + 0x1020;
pub(super) const RETURN_ID: u32 = 0x3210;
pub(super) const DATA: u32 = 0x7e101000;

fn image(base: u32, dll: bool) -> Vec<u8> {
    let mut image = vec![0; 1024];
    image[..2].copy_from_slice(b"MZ");
    let put16 = |b: &mut [u8], at: usize, v: u16| b[at..at + 2].copy_from_slice(&v.to_le_bytes());
    let put32 = |b: &mut [u8], at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put32(&mut image, 0x3c, 0x80);
    image[0x80..0x84].copy_from_slice(b"PE\0\0");
    for (at, v) in [
        (0x84, 0x14c),
        (0x86, 1),
        (0x94, 224),
        (0x96, if dll { 0x2103 } else { 0x103 }),
        (0x98, 0x10b),
        (0xdc, 3),
    ] {
        put16(&mut image, at, v);
    }
    for (at, v) in [
        (0xa8, 0x1000),
        (0xb4, base),
        (0xb8, 4096),
        (0xbc, 512),
        (0xd0, 0x2000),
        (0xd4, 512),
        (0xf4, 16),
    ] {
        put32(&mut image, at, v);
    }
    image[0x178..0x180].copy_from_slice(b".text\0\0\0");
    for (at, v) in [
        (0x180, 512),
        (0x184, 4096),
        (0x188, 512),
        (0x18c, 512),
        (0x19c, 0x60000020),
    ] {
        put32(&mut image, at, v);
    }
    image[512..].fill(0x90);
    image[512..520].copy_from_slice(&[0xb8, 1, 0, 0, 0, 0xc2, 12, 0]);
    image[528..531].copy_from_slice(&[0xc2, 16, 0]);
    image[544..546].copy_from_slice(&[0x0f, 0x0b]);
    image
}
pub(super) fn engine() -> (EngineInstance, u32) {
    configured_engine(None)
}
pub(super) fn engine_with_cache() -> (EngineInstance, u32) {
    configured_engine(Some(crate::process::execution::cache::ExecutionConfig32 {
        units: 2,
        wasm_bytes: 131072,
        promotion_hits: 8,
    }))
}
fn configured_engine(
    execution: Option<crate::process::execution::cache::ExecutionConfig32>,
) -> (EngineInstance, u32) {
    let mut engine = EngineInstance::new(128, KEY).unwrap();
    engine.register_kernel32_module(0x70000000).unwrap();
    engine.register_user32_module(0x71000000).unwrap();
    engine
        .register_guest_module("callback.exe", &image(EXE, false))
        .unwrap();
    engine
        .register_guest_module("helper.dll", &image(DLL, true))
        .unwrap();
    engine
        .start_module_process(
            "callback.exe",
            None,
            ModuleRuntimeConfig32 {
                stack_base: 0x7e100000,
                stack_pages: 8,
                runtime_base: 0x7d000000,
            },
        )
        .unwrap();
    engine.load_library_module("kernel32.dll", None).unwrap();
    engine.load_library_module("user32.dll", None).unwrap();
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine
        .configure_user32(
            User32Limits32 {
                classes: 4,
                windows: 8,
                messages: 8,
                continuations: 4,
            },
            PageRange::new(GuestAddress(0x60000000), 8).unwrap(),
        )
        .unwrap();
    if let Some(execution) = execution {
        engine.configure_execution(execution).unwrap();
    }
    string(&mut engine, DATA + 128, "callback-class");
    string(&mut engine, DATA + 256, "helper.dll");
    let mut class = [0; 40];
    class[4..8].copy_from_slice(&WNDPROC.to_le_bytes());
    class[16..20].copy_from_slice(&EXE.to_le_bytes());
    class[36..40].copy_from_slice(&(DATA + 128).to_le_bytes());
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(DATA), &class)
        .unwrap();
    material(&mut engine, &[User32Api32::RegisterClassW as u32]);
    call(&mut engine, User32Api32::RegisterClassW as u32, &[DATA]);
    let atom = decode_state(&engine.arena()[..STATE_SIZE])
        .unwrap()
        .registers[0];
    assert_ne!(atom, 0);
    (engine, atom)
}
pub(super) fn string(engine: &mut EngineInstance, address: u32, text: &str) {
    let bytes: Vec<u8> = text
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(address), &bytes)
        .unwrap();
}
pub(super) fn material(engine: &mut EngineInstance, apis: &[u32]) {
    let mut entries = vec![(WNDPROC, 3), (RETURN_PC, 2)];
    let mut gates = vec![(RETURN_PC, RETURN_ID)];
    let available = engine.module_gates().unwrap();
    for &id in apis {
        let gate = available.iter().find(|gate| gate.id == id).unwrap();
        entries.push((gate.entry.0, 2));
        gates.push((gate.entry.0, id));
    }
    compile(engine, &entries, &gates);
}
pub(super) fn compile(engine: &mut EngineInstance, entries: &[(u32, u32)], gates: &[(u32, u32)]) {
    let words: Vec<u32> = entries
        .iter()
        .chain(gates)
        .flat_map(|&(a, b)| [a, b])
        .collect();
    for (index, word) in words.into_iter().enumerate() {
        engine.arena.as_mut().get_mut()
            [TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    engine
        .compile_with_gates(entries.len() as u32, gates.len() as u32)
        .unwrap();
}
pub(super) fn call(engine: &mut EngineInstance, id: u32, args: &[u32]) {
    let gate = engine
        .module_gates()
        .unwrap()
        .iter()
        .find(|gate| gate.id == id)
        .unwrap()
        .entry
        .0;
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = gate;
    state.registers[4] -= 128;
    let bytes: Vec<u8> = std::iter::once(WNDPROC)
        .chain(args.iter().copied())
        .flat_map(u32::to_le_bytes)
        .collect();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(state.registers[4]), &bytes)
        .unwrap();
    encode_state_current(&state, &mut engine.arena.as_mut().get_mut()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id },
        },
        &mut engine.arena.as_mut().get_mut()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = engine
        .capture_windows_call(KEY, engine.generation)
        .unwrap()
        .token;
    engine
        .complete_windows_call(KEY, engine.generation, token)
        .unwrap();
}
pub(super) fn begin_create(
    engine: &mut EngineInstance,
    atom: u32,
    apis: &[u32],
) -> crate::abi::callback::CallbackRecord32 {
    let mut all = vec![User32Api32::CreateWindowExW as u32];
    all.extend_from_slice(apis);
    material(engine, &all);
    call(
        engine,
        User32Api32::CreateWindowExW as u32,
        &[0, atom, 0, 0, 0, 0, 64, 64, 0, 0, EXE, 0],
    );
    assert_eq!(
        engine.pending_windows_callback().unwrap().arguments[1],
        0x81
    );
    engine
        .begin_windows_callback(KEY, engine.generation, RETURN_PC, RETURN_ID)
        .unwrap()
}
pub(super) fn finish_callback(
    engine: &mut EngineInstance,
    record: crate::abi::callback::CallbackRecord32,
    value: u32,
) {
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = record.return_pc;
    state.registers[0] = value;
    state.registers[4] = engine
        .callback
        .as_ref()
        .unwrap()
        .outer
        .frame
        .state()
        .registers[4];
    encode_state_current(&state, &mut engine.arena.as_mut().get_mut()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate {
                id: record.return_id,
            },
        },
        &mut engine.arena.as_mut().get_mut()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    engine
        .finish_windows_callback(KEY, record.generation, record.token)
        .unwrap();
}
pub(super) fn finish_creation(
    engine: &mut EngineInstance,
    first: crate::abi::callback::CallbackRecord32,
) -> u32 {
    finish_callback(engine, first, 1);
    while let Some(request) = engine.pending_windows_callback() {
        let record = engine
            .begin_windows_callback(KEY, engine.generation, RETURN_PC, RETURN_ID)
            .unwrap();
        finish_callback(engine, record, u32::from(request.arguments[1] == 0x81));
    }
    assert!(
        engine.callback.is_none()
            && engine.pending_call.is_none()
            && engine.windows_replies.is_empty()
    );
    decode_state(&engine.arena()[..STATE_SIZE])
        .unwrap()
        .registers[0]
}
pub(super) fn return_initializer(
    engine: &mut EngineInstance,
    init: ModuleInitializer32,
    result: u32,
) -> ModuleOperation32 {
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = init.return_pc;
    state.registers[4] = init.return_esp;
    state.registers[0] = result;
    encode_state_current(&state, &mut engine.arena.as_mut().get_mut()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: init.return_id },
        },
        &mut engine.arena.as_mut().get_mut()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    engine.finish_module_initializer(init.token).unwrap()
}
pub(super) fn loader_material(
    engine: &mut EngineInstance,
    init: ModuleInitializer32,
    nested: bool,
) {
    let mut entries = vec![(init.entry, 8), (init.return_pc, 2)];
    let mut gates = vec![(init.return_pc, init.return_id)];
    if nested {
        let send = engine
            .module_gates()
            .unwrap()
            .iter()
            .find(|gate| gate.id == User32Api32::SendMessageW as u32)
            .unwrap()
            .entry
            .0;
        entries.extend([(WNDPROC, 3), (RETURN_PC, 2), (send, 2)]);
        gates.extend([
            (RETURN_PC, RETURN_ID),
            (send, User32Api32::SendMessageW as u32),
        ]);
    }
    compile(engine, &entries, &gates);
}
pub(super) fn load(engine: &mut EngineInstance) {
    call(engine, WindowsApi32::LoadLibraryW.id(), &[DATA + 256]);
}
