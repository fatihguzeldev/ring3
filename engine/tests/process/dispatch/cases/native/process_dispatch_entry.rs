use ring3_engine::{
    abi::x86::{encode_exit_v3, encode_state},
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError},
    windows::CallingConvention32,
};

const KEY: u64 = 0x1020_3040_5060_7080;
const GATE: u32 = 0x1000;
const ENTRY: u32 = 0x2000;
const RETURN: u32 = 0x3000;
const STACK: u32 = 0x9100;

#[derive(Debug, PartialEq, Eq)]
struct Observation {
    arena: Vec<u8>,
    arena_address: usize,
    generation: u32,
    legacy: Result<Vec<u8>, HostError>,
    legacy_pointer: Option<usize>,
    unit: Result<Vec<u8>, HostError>,
    unit_pointer: Option<usize>,
    legacy_guard: Result<(), HostError>,
    resident_guard: Result<(), HostError>,
    installed: Vec<Result<ring3_engine::process::ResidentInstallation, HostError>>,
    ram: Vec<Result<Vec<u8>, HostError>>,
}

fn observe(engine: &EngineInstance, id: u64) -> Observation {
    Observation {
        arena: engine.arena().to_vec(),
        arena_address: engine.arena_address(),
        generation: engine.generation(),
        legacy: engine.artifact_bytes().map(<[u8]>::to_vec),
        legacy_pointer: engine.artifact_bytes().ok().map(|b| b.as_ptr() as usize),
        unit: engine.resident_bytes(id).map(<[u8]>::to_vec),
        unit_pointer: engine.resident_bytes(id).ok().map(|b| b.as_ptr() as usize),
        legacy_guard: engine.guard(KEY, engine.generation()),
        resident_guard: engine.guard_resident(KEY, id),
        installed: [GATE, ENTRY, RETURN, u32::MAX]
            .map(|pc| engine.lookup_installed_resident(KEY, pc))
            .to_vec(),
        ram: [GATE, ENTRY, RETURN, 0x9000]
            .map(|pc| {
                let memory = engine.memory()?;
                let mut bytes = vec![0; 4096];
                memory
                    .read(GuestAddress(pc), &mut bytes)
                    .map_err(HostError::Memory)?;
                Ok(bytes)
            })
            .to_vec(),
    }
}

fn check(engine: &EngineInstance, id: u64, key: u64, expected: Result<(), HostError>) {
    let before = observe(engine, id);
    assert_eq!(engine.guard_dispatch_entry(key), expected);
    assert_eq!(observe(engine, id), before);
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn specs(engine: &mut EngineInstance) {
    let transfer = &mut engine.arena_mut().unwrap()[140..];
    for (index, (pc, value)) in [
        (GATE, 2_u32),
        (ENTRY, 3),
        (RETURN, 2),
        (GATE, 17),
        (RETURN, 18),
    ]
    .into_iter()
    .enumerate()
    {
        transfer[index * 8..index * 8 + 4].copy_from_slice(&pc.to_le_bytes());
        transfer[index * 8 + 4..index * 8 + 8].copy_from_slice(&value.to_le_bytes());
    }
}

fn fixture() -> (EngineInstance, u32, u64) {
    let mut engine = EngineInstance::new(4, KEY).unwrap();
    for page in [GATE, ENTRY, RETURN, 0x9000] {
        engine.map(page, 1, 7).unwrap();
    }
    upload(&mut engine, GATE, &[0x0f, 0x0b]);
    upload(&mut engine, ENTRY, &[0x90, 0xeb, 0]);
    upload(&mut engine, RETURN, &[0x0f, 0x0b]);
    engine.write32(STACK, ENTRY).unwrap();
    engine.write32(STACK + 4, 0x89abcdef).unwrap();
    specs(&mut engine);
    let generation = engine.compile_with_gates(3, 2).unwrap();
    specs(&mut engine);
    let id = engine.compile_resident_with_gates(3, 2).unwrap().get();
    engine
        .acknowledge_resident_installation(KEY, id, 0)
        .unwrap();
    (engine, generation, id)
}

fn typed_gate(engine: &mut EngineInstance) {
    let arena = engine.arena_mut().unwrap();
    encode_state(
        &State32 {
            registers: [1, 2, 3, 4, STACK, 6, 7, 8],
            eip: GATE,
            eflags: 0xcd7,
        },
        &mut arena[..56],
    )
    .unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: 17 },
        },
        &mut arena[56..96],
    )
    .unwrap();
    arena[96..100].fill(0);
}

#[test]
fn dispatch_entry_is_pure_process_authority_independent_of_code_and_cpu_buffers() {
    let mut empty = EngineInstance::new(1, KEY).unwrap();
    check(&empty, 0, KEY, Ok(()));
    for key in [0, KEY ^ 1, KEY ^ (1_u64 << 32), KEY as u32 as u64, u64::MAX] {
        check(&empty, 0, key, Err(HostError::InvalidArtifact));
    }
    empty.arena_mut().unwrap().fill(0xff);
    check(&empty, 0, KEY, Ok(()));
    let (mut engine, _, id) = fixture();
    check(&engine, id, KEY, Ok(()));
    engine.arena_mut().unwrap()[..100].fill(0xff);
    check(&engine, id, KEY, Ok(()));
    upload(&mut engine, GATE, &[0x0f, 0x0b]);
    assert_eq!(engine.artifact_bytes(), Err(HostError::CodeInvalidated));
    assert!(engine.guard_resident(KEY, id).is_err());
    check(&engine, id, KEY, Ok(()));
    engine.close();
    for key in [KEY, 0, KEY ^ (1_u64 << 32)] {
        check(&engine, id, key, Err(HostError::Closed));
    }
}

#[test]
fn pending_and_callback_entry_priorities_preserve_all_owned_state() {
    let (mut engine, generation, id) = fixture();
    // typed native stop controls; translated gate capture is covered by the wasm proof.
    typed_gate(&mut engine);
    let resident = engine
        .capture_resident_call(KEY, id, CallingConvention32::Cdecl, 0)
        .unwrap();
    check(&engine, id, KEY, Err(HostError::Call(CallError::Busy)));
    check(
        &engine,
        id,
        KEY ^ (1_u64 << 32),
        Err(HostError::InvalidArtifact),
    );
    engine.arena_mut().unwrap()[..100].fill(0xff);
    upload(&mut engine, ENTRY, &[0x90, 0xeb, 0]);
    check(&engine, id, KEY, Err(HostError::Call(CallError::Busy)));
    engine.abandon_call(KEY, resident.token).unwrap();
    check(&engine, id, KEY, Ok(()));
    specs(&mut engine);
    let fresh_generation = engine.compile_with_gates(3, 2).unwrap();
    assert_ne!(fresh_generation, generation);
    typed_gate(&mut engine);
    let outer = engine
        .capture_call(KEY, fresh_generation, CallingConvention32::Cdecl, 0)
        .unwrap();
    check(&engine, id, KEY, Err(HostError::Call(CallError::Busy)));
    let callback = engine
        .begin_callback(KEY, fresh_generation, outer.token, ENTRY, RETURN, 18, &[])
        .unwrap();
    check(&engine, id, KEY, Err(HostError::Call(CallError::Busy)));
    check(&engine, id, KEY ^ 1, Err(HostError::InvalidArtifact));
    engine.arena_mut().unwrap()[..100].fill(0xff);
    check(&engine, id, KEY, Err(HostError::Call(CallError::Busy)));
    engine.abort_callback(KEY, callback.token).unwrap();
    check(&engine, id, KEY, Err(HostError::Call(CallError::Busy)));
    engine.abandon_call(KEY, outer.token).unwrap();
    check(&engine, id, KEY, Ok(()));
    typed_gate(&mut engine);
    let _pending = engine
        .capture_call(KEY, fresh_generation, CallingConvention32::Cdecl, 0)
        .unwrap();
    engine.close();
    check(&engine, id, 0, Err(HostError::Closed));
    check(&engine, id, KEY, Err(HostError::Closed));
}
