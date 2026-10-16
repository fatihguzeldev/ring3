use super::image::{Import, Symbol, image};
use ring3_engine::{
    abi::{
        arena::{EXIT_OFFSET, TRANSFER_OFFSET},
        x86::{decode_exit, decode_state, encode_exit_v3, encode_state, encode_state_v3},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    loader::ImportSymbol32,
    process::{
        EngineInstance, HostError, ModuleInitializer32, ModuleOperation32, ModuleRuntimeConfig32,
    },
    windows::WindowsApi32,
};

pub const MAIN: u32 = 0x0040_0000;
pub const LIBRARY: u32 = 0x0060_0000;
pub const NESTED: u32 = 0x0080_0000;
pub const KERNEL: u32 = 0x7000_0000;
pub const ESP: u32 = 0x7e00_0100;
pub const TEXT: u32 = MAIN + 0x2d00;
pub const KEY: u64 = 0xe353_aabb_7788_9900;

#[derive(Clone, Copy)]
pub enum Owner {
    Replacement,
    Resident,
}

pub struct Fixture {
    pub engine: EngineInstance,
    pub owner: Owner,
    pub generation: u32,
    pub resident: u64,
}

pub fn guest_library(base: u32, entry: bool) -> Vec<u8> {
    let mut bytes = image(
        true,
        base,
        &[],
        &[(1, Some("answer"), "@1000"), (2, Some("data"), "@2f00")],
    );
    if entry {
        bytes[0xa8..0xac].copy_from_slice(&0x1000_u32.to_le_bytes());
        bytes[0x200..0x208].copy_from_slice(&[0xb8, 1, 0, 0, 0, 0xc2, 12, 0]);
    }
    bytes
}

pub fn fixture(owner: Owner, forwarder: bool) -> Fixture {
    let mut engine = EngineInstance::new(64, KEY).unwrap();
    engine.register_kernel32_module(KERNEL).unwrap();
    let exports = if forwarder {
        vec![(1, Some("forward"), "helper.answer")]
    } else {
        vec![]
    };
    engine
        .register_guest_module(
            "main.exe",
            &image(
                false,
                MAIN,
                &[Import {
                    module: "kernel32.dll",
                    symbols: vec![Symbol::Name("LoadLibraryA")],
                }],
                &exports,
            ),
        )
        .unwrap();
    engine
        .register_guest_module("helper.dll", &guest_library(LIBRARY, true))
        .unwrap();
    engine
        .register_guest_module("nested.dll", &guest_library(NESTED, true))
        .unwrap();
    assert_eq!(
        engine
            .start_module_process("main.exe", None, ModuleRuntimeConfig32::default())
            .unwrap(),
        ModuleOperation32::Loaded { handle: MAIN }
    );
    let gate_count = describe_kernel_gates(&mut engine);
    let resident = engine
        .compile_resident_with_gates(gate_count, gate_count)
        .unwrap()
        .get();
    let generation = engine.compile_with_gates(gate_count, gate_count).unwrap();
    Fixture {
        engine,
        owner,
        generation,
        resident,
    }
}

pub fn compile_initializer_gates(f: &mut Fixture) {
    if matches!(f.owner, Owner::Replacement) {
        let count = describe_kernel_gates(&mut f.engine);
        f.generation = f.engine.compile_with_gates(count, count).unwrap();
    }
}

fn describe_kernel_gates(engine: &mut EngineInstance) -> u32 {
    let apis = [
        WindowsApi32::GetLastError,
        WindowsApi32::GetModuleHandleA,
        WindowsApi32::GetModuleHandleW,
        WindowsApi32::LoadLibraryA,
        WindowsApi32::LoadLibraryW,
        WindowsApi32::GetProcAddress,
        WindowsApi32::FreeLibrary,
        WindowsApi32::ExitProcess,
    ];
    let gates: Vec<_> = engine
        .module_gates()
        .unwrap()
        .into_iter()
        .filter(|gate| apis.iter().any(|api| api.id() == gate.id))
        .collect();
    assert_eq!(gates.len(), apis.len());
    for (index, value) in gates
        .iter()
        .flat_map(|gate| [gate.entry.0, 2])
        .chain(gates.iter().flat_map(|gate| [gate.entry.0, gate.id]))
        .enumerate()
    {
        let offset = TRANSFER_OFFSET + index * 4;
        engine.arena_mut().unwrap()[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    gates.len() as u32
}

pub fn text(f: &mut Fixture, address: u32, value: &str, wide: bool) {
    let bytes: Vec<u8> = if wide {
        value
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect()
    } else {
        value.bytes().chain([0]).collect()
    };
    f.engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(&bytes);
    f.engine.upload(address, bytes.len() as u32).unwrap();
}

pub fn capture(f: &mut Fixture, api: WindowsApi32, args: &[u32]) -> (u32, State32, Vec<u8>) {
    capture_at(f, api, args, ESP, MAIN + 0x1000)
}

pub fn capture_at(
    f: &mut Fixture,
    api: WindowsApi32,
    args: &[u32],
    esp: u32,
    return_pc: u32,
) -> (u32, State32, Vec<u8>) {
    assert_eq!(args.len() as u32, api.stack_words());
    f.engine.write32(esp, return_pc).unwrap();
    for (index, value) in args.iter().enumerate() {
        f.engine
            .write32(esp + 4 + index as u32 * 4, *value)
            .unwrap();
    }
    let gate = f
        .engine
        .module_export(KERNEL, &ImportSymbol32::Name(api.name().into()))
        .unwrap()
        .address()
        .0;
    let state = State32 {
        registers: [
            0x1357_9bdf,
            0x2468_ace0,
            0x3456_789a,
            0x4567_89ab,
            esp,
            0x5678_9abc,
            0x6789_abcd,
            0x789a_bcde,
        ],
        eip: gate,
        eflags: 0xcd7,
    };
    encode_state_v3(&state, &mut f.engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 7,
            reason: ExitReason::Gate { id: api.id() },
        },
        &mut f.engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + 40],
    )
    .unwrap();
    let record = match f.owner {
        Owner::Replacement => {
            f.engine
                .capture_call(KEY, f.generation, api.convention(), api.stack_words())
        }
        Owner::Resident => {
            f.engine
                .capture_resident_call(KEY, f.resident, api.convention(), api.stack_words())
        }
    }
    .unwrap();
    assert_eq!(record.arguments[..args.len()], *args);
    (record.token, state, f.engine.arena().to_vec())
}

pub fn complete(f: &mut Fixture, token: u32) -> Result<(), HostError> {
    match f.owner {
        Owner::Replacement => f.engine.complete_windows_call(KEY, f.generation, token),
        Owner::Resident => f
            .engine
            .complete_resident_windows_call(KEY, f.resident, token),
    }
}

pub fn expected_return(before: Vec<u8>, state: State32, api: WindowsApi32, result: u32) -> Vec<u8> {
    expected_return_to(before, state, api, result, MAIN + 0x1000)
}

pub fn expected_return_to(
    mut before: Vec<u8>,
    mut state: State32,
    api: WindowsApi32,
    result: u32,
    return_pc: u32,
) -> Vec<u8> {
    state.registers[0] = result;
    state.registers[4] += 4 + api.stack_words() * 4;
    state.eip = return_pc;
    encode_state(&state, &mut before[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::NeedCode,
        },
        &mut before[EXIT_OFFSET..EXIT_OFFSET + 40],
    )
    .unwrap();
    before
}

pub fn returned(f: &Fixture, before: Vec<u8>, state: State32, api: WindowsApi32, result: u32) {
    assert_eq!(
        f.engine.arena(),
        expected_return(before, state, api, result)
    );
    assert_eq!(
        decode_exit(&f.engine.arena()[EXIT_OFFSET..EXIT_OFFSET + 40])
            .unwrap()
            .reason,
        ExitReason::NeedCode
    );
}

pub fn return_initializer(f: &mut Fixture, init: ModuleInitializer32, result: u32) {
    // native lifecycle input; the separate wasm fixture executes the initializer body.
    let mut state = decode_state(&f.engine.arena()[..56]).unwrap();
    state.registers[0] = result;
    state.registers[4] = init.return_esp;
    state.eip = init.return_pc;
    encode_state_v3(&state, &mut f.engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::Gate { id: init.return_id },
        },
        &mut f.engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + 40],
    )
    .unwrap();
}
