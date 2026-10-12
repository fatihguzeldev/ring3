use ring3_engine::{
    abi::x86::{encode_exit_v3, encode_state},
    cpu::{ExecutionExit, ExitReason, dbt::RegistryError, x86::State32},
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError, ResidentInstallation},
    windows::CallingConvention32,
};

const KEY: u64 = 0x1020_3040_5060_7080;
const GATE: u32 = 0x1000;
const ENTRY: u32 = 0x2000;
const RETURN: u32 = 0x3000;
const STACK: u32 = 0x9100;

struct RetainedModule {
    bytes: Vec<u8>,
    pointer: usize,
}

fn retain(engine: &EngineInstance) -> RetainedModule {
    let bytes = engine.dispatcher_bytes(KEY).unwrap();
    assert!(bytes.len() > 8);
    assert_eq!(&bytes[..8], b"\0asm\x01\0\0\0");
    RetainedModule {
        bytes: bytes.to_vec(),
        pointer: bytes.as_ptr() as usize,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Observation {
    arena: Vec<u8>,
    arena_address: usize,
    generation: u32,
    dispatcher: Result<Vec<u8>, HostError>,
    dispatcher_pointer: Option<usize>,
    legacy: Result<Vec<u8>, HostError>,
    legacy_pointer: Option<usize>,
    resident: Result<Vec<u8>, HostError>,
    resident_pointer: Option<usize>,
    entry_guard: Result<(), HostError>,
    legacy_guard: Result<(), HostError>,
    resident_guard: Result<(), HostError>,
    installed: Vec<Result<ResidentInstallation, HostError>>,
    ram: Vec<Result<Vec<u8>, HostError>>,
}

fn observe(engine: &EngineInstance, id: u64) -> Observation {
    Observation {
        arena: engine.arena().to_vec(),
        arena_address: engine.arena_address(),
        generation: engine.generation(),
        dispatcher: engine.dispatcher_bytes(KEY).map(<[u8]>::to_vec),
        dispatcher_pointer: engine
            .dispatcher_bytes(KEY)
            .ok()
            .map(|b| b.as_ptr() as usize),
        legacy: engine.artifact_bytes().map(<[u8]>::to_vec),
        legacy_pointer: engine.artifact_bytes().ok().map(|b| b.as_ptr() as usize),
        resident: engine.resident_bytes(id).map(<[u8]>::to_vec),
        resident_pointer: engine.resident_bytes(id).ok().map(|b| b.as_ptr() as usize),
        entry_guard: engine.guard_dispatch_entry(KEY),
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

fn check(
    engine: &EngineInstance,
    id: u64,
    key: u64,
    expected: Result<(), HostError>,
    retained: &RetainedModule,
) {
    let before = observe(engine, id);
    match expected {
        Ok(()) => {
            let bytes = engine.dispatcher_bytes(key).unwrap();
            assert_eq!(bytes, retained.bytes);
            assert_eq!(bytes.as_ptr() as usize, retained.pointer);
        }
        Err(error) => assert_eq!(engine.dispatcher_bytes(key), Err(error)),
    }
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

fn fixture() -> (EngineInstance, RetainedModule) {
    let mut engine = EngineInstance::new(4, KEY).unwrap();
    let retained = retain(&engine);
    for page in [GATE, ENTRY, RETURN, 0x9000] {
        engine.map(page, 1, 7).unwrap();
    }
    upload(&mut engine, GATE, &[0x0f, 0x0b]);
    upload(&mut engine, ENTRY, &[0x90, 0xeb, 0]);
    upload(&mut engine, RETURN, &[0x0f, 0x0b]);
    engine.write32(STACK, ENTRY).unwrap();
    engine.write32(STACK + 4, 0x89ab_cdef).unwrap();
    (engine, retained)
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
fn dispatcher_module_is_pure_stable_process_storage_independent_of_code_and_buffers() {
    let mut empty = EngineInstance::new(1, KEY).unwrap();
    let retained = retain(&empty);
    check(&empty, 0, KEY, Ok(()), &retained);
    for key in [0, KEY ^ 1, KEY ^ (1_u64 << 32), KEY as u32 as u64, u64::MAX] {
        check(&empty, 0, key, Err(HostError::InvalidArtifact), &retained);
    }
    empty.arena_mut().unwrap().fill(0xff);
    check(&empty, 0, KEY, Ok(()), &retained);
    empty.close();
    check(&empty, 0, 0, Err(HostError::Closed), &retained);

    let (mut engine, retained) = fixture();
    check(&engine, 0, KEY, Ok(()), &retained);
    specs(&mut engine);
    let generation = engine.compile_with_gates(3, 2).unwrap();
    check(&engine, 0, KEY, Ok(()), &retained);
    specs(&mut engine);
    let id = engine.compile_resident_with_gates(3, 2).unwrap().get();
    check(&engine, id, KEY, Ok(()), &retained);
    let installed = engine
        .acknowledge_resident_installation(KEY, id, 0)
        .unwrap();
    assert_eq!(
        installed,
        ResidentInstallation {
            unit_id: id,
            slot: 0
        }
    );
    check(&engine, id, KEY, Ok(()), &retained);
    engine.arena_mut().unwrap()[..100].fill(0xff);
    check(&engine, id, KEY, Ok(()), &retained);
    upload(&mut engine, ENTRY, &[0x90, 0xeb, 0]);
    assert_eq!(engine.artifact_bytes(), Err(HostError::CodeInvalidated));
    assert_eq!(
        engine.resident_bytes(id),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    check(&engine, id, KEY, Ok(()), &retained);
    specs(&mut engine);
    let fresh_generation = engine.compile_with_gates(3, 2).unwrap();
    assert!(fresh_generation > generation);
    check(&engine, id, KEY, Ok(()), &retained);
    engine.close();
    for key in [KEY, 0, KEY ^ (1_u64 << 32)] {
        check(&engine, id, key, Err(HostError::Closed), &retained);
    }
}

#[test]
fn pending_and_callback_inspection_keeps_the_frozen_module_and_all_owned_state() {
    let (mut engine, retained) = fixture();
    specs(&mut engine);
    let generation = engine.compile_with_gates(3, 2).unwrap();
    specs(&mut engine);
    let id = engine.compile_resident_with_gates(3, 2).unwrap().get();
    engine
        .acknowledge_resident_installation(KEY, id, 0)
        .unwrap();
    // typed native stop controls; translated gates belong to the wasm proof.
    typed_gate(&mut engine);
    let resident = engine
        .capture_resident_call(KEY, id, CallingConvention32::Cdecl, 0)
        .unwrap();
    assert_eq!(
        engine.guard_dispatch_entry(KEY),
        Err(HostError::Call(CallError::Busy))
    );
    check(&engine, id, KEY, Ok(()), &retained);
    check(
        &engine,
        id,
        KEY ^ (1_u64 << 32),
        Err(HostError::InvalidArtifact),
        &retained,
    );
    engine.arena_mut().unwrap()[..100].fill(0xff);
    upload(&mut engine, ENTRY, &[0x90, 0xeb, 0]);
    check(&engine, id, KEY, Ok(()), &retained);
    engine.abandon_call(KEY, resident.token).unwrap();
    check(&engine, id, KEY, Ok(()), &retained);
    specs(&mut engine);
    let fresh_generation = engine.compile_with_gates(3, 2).unwrap();
    assert!(fresh_generation > generation);
    typed_gate(&mut engine);
    let outer = engine
        .capture_call(KEY, fresh_generation, CallingConvention32::Cdecl, 0)
        .unwrap();
    check(&engine, id, KEY, Ok(()), &retained);
    let callback = engine
        .begin_callback(KEY, fresh_generation, outer.token, ENTRY, RETURN, 18, &[])
        .unwrap();
    assert_eq!(
        engine.guard_dispatch_entry(KEY),
        Err(HostError::Call(CallError::Busy))
    );
    check(&engine, id, KEY, Ok(()), &retained);
    check(
        &engine,
        id,
        KEY ^ 1,
        Err(HostError::InvalidArtifact),
        &retained,
    );
    engine.arena_mut().unwrap()[..100].fill(0xff);
    check(&engine, id, KEY, Ok(()), &retained);
    engine.abort_callback(KEY, callback.token).unwrap();
    assert_eq!(
        engine.guard_dispatch_entry(KEY),
        Err(HostError::Call(CallError::Busy))
    );
    check(&engine, id, KEY, Ok(()), &retained);
    engine.abandon_call(KEY, outer.token).unwrap();
    check(&engine, id, KEY, Ok(()), &retained);
    typed_gate(&mut engine);
    let _pending = engine
        .capture_call(KEY, fresh_generation, CallingConvention32::Cdecl, 0)
        .unwrap();
    engine.close();
    for key in [0, KEY, KEY ^ (1_u64 << 32)] {
        check(&engine, id, key, Err(HostError::Closed), &retained);
    }
}
