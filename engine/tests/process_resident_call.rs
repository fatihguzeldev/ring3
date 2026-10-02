use ring3_engine::{
    abi::{
        arena::{ARENA_SIZE, TRANSFER_OFFSET},
        call_frame::{CALL_FRAME_SIZE, CallRecord32, encode_call_frame},
        x86::{encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, dbt::RegistryError, x86::State32},
    memory::{Access, FaultReason, GuestAddress, MemoryError, MemoryFault},
    process::{CallError, EngineInstance, HostError},
    windows::CallingConvention32,
};

const KEY: u64 = 0x9456_789a_b000_0001;
const GATE: u32 = 0x1000;
const WHOLE: u32 = 0x2000;
const KEEP: u32 = 0x3000;
const LEGACY: u32 = 0x4000;
const CALLBACK_RETURN: u32 = 0x4100;
const STACK: u32 = 0x8000;
const RETURN: u32 = 0x6000;
const GATE_ID: u32 = 17;

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn describe(engine: &mut EngineInstance, blocks: &[(u32, u32)], gates: &[(u32, u32)]) {
    for (index, &(pc, value)) in blocks.iter().chain(gates).enumerate() {
        let start = TRANSFER_OFFSET + index * 8;
        engine.arena_mut().unwrap()[start..start + 4].copy_from_slice(&pc.to_le_bytes());
        engine.arena_mut().unwrap()[start + 4..start + 8].copy_from_slice(&value.to_le_bytes());
    }
}

fn compile(engine: &mut EngineInstance, blocks: &[(u32, u32)], gates: &[(u32, u32)]) -> u64 {
    describe(engine, blocks, gates);
    let arena = engine.arena().to_vec();
    let id = engine
        .compile_resident_with_gates(blocks.len() as u32, gates.len() as u32)
        .unwrap()
        .get();
    assert_eq!(engine.arena(), arena);
    id
}

fn stop(engine: &mut EngineInstance, pc: u32, id: u32, esp: u32, retired: u32) -> State32 {
    // typed native input; the separate actual target supplies translated Gate stops.
    let state = State32 {
        registers: [0x89ab_cdef, 0x1357_9bdf, 3, 4, esp, 6, 7, 8],
        eip: pc,
        eflags: 0xcd7,
    };
    let arena = engine.arena_mut().unwrap();
    encode_state(&state, &mut arena[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired,
            reason: ExitReason::Gate { id },
        },
        &mut arena[56..96],
    )
    .unwrap();
    arena[96..100].fill(0);
    arena[100..140].fill(0x5a);
    arena[TRANSFER_OFFSET..].fill(0xcc);
    state
}

fn fixture() -> (EngineInstance, u64, u64, State32) {
    let mut engine = EngineInstance::new(10, KEY).unwrap();
    for pc in [GATE, WHOLE, KEEP] {
        engine.map(pc, 1, 7).unwrap();
    }
    engine.map(STACK, 1, 3).unwrap();
    upload(&mut engine, GATE, &[0x0f, 0x0b]);
    upload(&mut engine, WHOLE, &[0x90, 0xeb, 0]);
    upload(&mut engine, KEEP, &[0x0f, 0x0b]);
    let id = compile(&mut engine, &[(GATE, 2), (WHOLE, 3)], &[(GATE, GATE_ID)]);
    let keep = compile(&mut engine, &[(KEEP, 2)], &[(KEEP, GATE_ID)]);
    engine.write32(STACK, RETURN).unwrap();
    for index in 0..16 {
        engine
            .write32(STACK + 4 + index * 4, 0x1122_3300 + index)
            .unwrap();
    }
    let state = stop(&mut engine, GATE, GATE_ID, STACK, 0);
    (engine, id, keep, state)
}

fn call(error: CallError) -> HostError {
    HostError::Call(error)
}

fn stale() -> HostError {
    HostError::Resident(RegistryError::CodeInvalidated)
}

fn word(engine: &EngineInstance, pc: u32) -> u32 {
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(pc), &mut bytes)
        .unwrap();
    u32::from_le_bytes(bytes)
}

#[derive(Debug, PartialEq, Eq)]
struct SavedUnit {
    bytes: Result<Vec<u8>, HostError>,
    pointer: Option<usize>,
}

#[derive(Debug, PartialEq, Eq)]
struct Saved {
    arena: Vec<u8>,
    arena_pointer: usize,
    generation: u32,
    legacy: Result<Vec<u8>, HostError>,
    units: Vec<SavedUnit>,
    pages: Vec<Result<Vec<u8>, MemoryError>>,
}

fn saved(engine: &EngineInstance, ids: &[u64]) -> Saved {
    Saved {
        arena: engine.arena().to_vec(),
        arena_pointer: engine.arena_address(),
        generation: engine.generation(),
        legacy: engine.artifact_bytes().map(|bytes| bytes.to_vec()),
        units: ids
            .iter()
            .map(|&id| SavedUnit {
                bytes: engine.resident_bytes(id).map(|bytes| bytes.to_vec()),
                pointer: engine
                    .resident_bytes(id)
                    .ok()
                    .map(|bytes| bytes.as_ptr() as usize),
            })
            .collect(),
        pages: [
            0,
            GATE,
            WHOLE,
            KEEP,
            LEGACY,
            0x7000,
            STACK,
            0x9000,
            0xffff_f000,
        ]
        .into_iter()
        .map(|pc| {
            let mut page = vec![0; 4096];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(pc), &mut page)
                .map(|()| page)
        })
        .collect(),
    }
}

fn reject<T: std::fmt::Debug + PartialEq>(
    engine: &mut EngineInstance,
    ids: &[u64],
    error: HostError,
    operation: impl FnOnce(&mut EngineInstance) -> Result<T, HostError>,
) {
    let before = saved(engine, ids);
    assert_eq!(operation(engine), Err(error));
    assert_eq!(saved(engine, ids), before);
}

fn expected_record(state: State32, token: u32, id: u32, tag: u32, count: u32) -> CallRecord32 {
    CallRecord32 {
        token,
        id,
        convention: tag,
        stack_words: count,
        gate_pc: state.eip,
        entry_esp: state.registers[4],
        return_pc: RETURN,
        this_pointer: if tag == 3 { state.registers[1] } else { 0 },
        arguments: std::array::from_fn(|index| {
            if index < count as usize {
                0x1122_3300 + index as u32
            } else {
                0
            }
        }),
    }
}

fn capture(engine: &mut EngineInstance, id: u64, expected: CallRecord32) {
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.capture_resident_call_raw(KEY, id, expected.convention, expected.stack_words),
        Ok(expected)
    );
    let mut wanted = before;
    encode_call_frame(
        &expected,
        &mut wanted[TRANSFER_OFFSET..TRANSFER_OFFSET + CALL_FRAME_SIZE],
    )
    .unwrap();
    assert_eq!(engine.arena(), wanted);
}

fn completed(before: &[u8], mut state: State32, esp: u32, return_pc: u32, result: u32) -> Vec<u8> {
    state.registers[0] = result;
    state.registers[4] = esp;
    state.eip = return_pc;
    let mut wanted = before.to_vec();
    encode_state(&state, &mut wanted[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::NeedCode,
        },
        &mut wanted[56..96],
    )
    .unwrap();
    wanted
}

fn install_legacy(engine: &mut EngineInstance) -> u32 {
    engine.map(LEGACY, 1, 7).unwrap();
    upload(engine, LEGACY, &[0x90, 0xc3]);
    upload(engine, CALLBACK_RETURN, &[0x0f, 0x0b]);
    describe(
        engine,
        &[(GATE, 2), (LEGACY, 2), (CALLBACK_RETURN, 2)],
        &[(GATE, GATE_ID), (CALLBACK_RETURN, 18)],
    );
    engine.compile_with_gates(3, 2).unwrap()
}

#[test]
fn resident_gate_capture_publishes_frame_without_legacy_artifact() {
    for (gate_id, count) in [(GATE_ID, 2), (0x8000_0001, 0), (u32::MAX, 16)] {
        let (mut engine, _, keep, _) = fixture();
        engine.protect(GATE, 1, 4).unwrap();
        let id = compile(&mut engine, &[(GATE, 2), (WHOLE, 3)], &[(GATE, gate_id)]);
        let state = stop(&mut engine, GATE, gate_id, STACK, 7);
        assert_eq!(engine.generation(), 0);
        assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
        let bytes = engine.resident_bytes(id).unwrap().to_vec();
        let pointer = engine.resident_bytes(id).unwrap().as_ptr();
        let before = engine.arena().to_vec();
        let expected = expected_record(state, 1, gate_id, 1, count);
        assert_eq!(
            engine.capture_resident_call(KEY, id, CallingConvention32::Cdecl, count),
            Ok(expected)
        );
        let mut wanted = before;
        encode_call_frame(
            &expected,
            &mut wanted[TRANSFER_OFFSET..TRANSFER_OFFSET + CALL_FRAME_SIZE],
        )
        .unwrap();
        assert_eq!(engine.arena(), wanted);
        assert_eq!(engine.resident_bytes(id).unwrap(), bytes);
        assert_eq!(engine.resident_bytes(id).unwrap().as_ptr(), pointer);
        assert_eq!(word(&engine, STACK), RETURN);
        assert!(engine.resident_bytes(keep).is_ok());
        let before = engine.arena().to_vec();
        engine.abandon_call(KEY, 1).unwrap();
        assert_eq!(engine.arena(), before);
        capture(
            &mut engine,
            id,
            expected_record(state, 2, gate_id, 1, count),
        );
    }
}

#[test]
fn identity_request_stop_and_busy_priorities_preserve_all_outputs_and_units() {
    let (mut engine, id, keep, _) = fixture();
    assert_eq!(install_legacy(&mut engine), 1);
    stop(&mut engine, GATE, GATE_ID, STACK, 0);
    let (foreign, foreign_id, _, _) = fixture();
    assert_ne!(foreign_id, id);
    for (key, unit, error) in [
        (KEY ^ 1, 0, HostError::InvalidArtifact),
        (KEY, 0, HostError::Resident(RegistryError::InvalidUnit)),
        (
            KEY,
            id ^ (1_u64 << 32),
            HostError::Resident(RegistryError::InvalidUnit),
        ),
        (
            KEY,
            foreign_id,
            HostError::Resident(RegistryError::InvalidUnit),
        ),
    ] {
        reject(&mut engine, &[id, keep], error, |engine| {
            engine.capture_resident_call_raw(key, unit, u32::MAX, u32::MAX)
        });
    }
    drop(foreign);
    for count in [17, u32::MAX] {
        reject(
            &mut engine,
            &[id, keep],
            call(CallError::InvalidRequest),
            |engine| engine.capture_resident_call_raw(KEY, id, 0, count),
        );
    }
    for tag in [0, 4, u32::MAX] {
        reject(
            &mut engine,
            &[id, keep],
            call(CallError::InvalidRequest),
            |engine| engine.capture_resident_call_raw(KEY, id, tag, 0),
        );
    }
    for offset in [0, 12, 52, 56, 60, 68, 80, 84] {
        stop(&mut engine, GATE, GATE_ID, STACK, 0);
        engine.arena_mut().unwrap()[offset] ^= if offset == 52 { 2 } else { 1 };
        reject(
            &mut engine,
            &[id, keep],
            call(CallError::InvalidStop),
            |engine| engine.capture_resident_call(KEY, id, CallingConvention32::Cdecl, 0),
        );
    }
    for (pc, gate_id, owner) in [
        (GATE + 1, GATE_ID, id),
        (GATE, 18, id),
        (GATE, GATE_ID, keep),
    ] {
        stop(&mut engine, pc, gate_id, STACK, 0);
        reject(
            &mut engine,
            &[id, keep],
            call(CallError::InvalidStop),
            |engine| engine.capture_resident_call(KEY, owner, CallingConvention32::Cdecl, 0),
        );
    }
    stop(&mut engine, GATE, GATE_ID, 0x7000, 0);
    encode_exit_v3(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::Budget,
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
    reject(
        &mut engine,
        &[id, keep],
        call(CallError::InvalidStop),
        |engine| engine.capture_resident_call(KEY, id, CallingConvention32::Cdecl, 0),
    );
    stop(&mut engine, GATE, GATE_ID, 0x7000, 0);
    engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    reject(
        &mut engine,
        &[id, keep],
        call(CallError::Cancelled),
        |engine| engine.capture_resident_call(KEY, id, CallingConvention32::Cdecl, 0),
    );
    let state = stop(&mut engine, GATE, GATE_ID, STACK, 0);
    capture(&mut engine, id, expected_record(state, 1, GATE_ID, 1, 2));
    for unit in [id, keep] {
        reject(&mut engine, &[id, keep], call(CallError::Busy), |engine| {
            engine.capture_resident_call_raw(KEY, unit, u32::MAX, u32::MAX)
        });
    }
    reject(
        &mut engine,
        &[id, keep],
        HostError::InvalidArtifact,
        |engine| engine.capture_resident_call_raw(KEY ^ 1, id, 0, 17),
    );
    reject(
        &mut engine,
        &[id, keep],
        HostError::Resident(RegistryError::InvalidUnit),
        |engine| engine.capture_resident_call_raw(KEY, 0, 0, 17),
    );
    reject(&mut engine, &[id, keep], call(CallError::Busy), |engine| {
        engine.compile(0)
    });
    reject(&mut engine, &[id, keep], call(CallError::Busy), |engine| {
        engine.compile_resident_with_gates(0, u32::MAX)
    });
    assert_eq!(engine.lookup_resident(GATE).unwrap().get(), id);
    assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
    assert!(engine.resident_bytes(id).is_ok());
}

#[test]
fn all_conventions_complete_only_frozen_cpu_values_and_consume_pending_once() {
    for (convention, tag, count, esp) in [
        (CallingConvention32::Cdecl, 1, 2, STACK + 4),
        (CallingConvention32::Stdcall, 2, 2, STACK + 12),
        (CallingConvention32::Thiscall, 3, 16, STACK + 68),
    ] {
        let (mut engine, id, keep, state) = fixture();
        let expected = expected_record(state, 1, GATE_ID, tag, count);
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.capture_resident_call(KEY, id, convention, count),
            Ok(expected)
        );
        let mut wanted = before;
        encode_call_frame(
            &expected,
            &mut wanted[TRANSFER_OFFSET..TRANSFER_OFFSET + CALL_FRAME_SIZE],
        )
        .unwrap();
        assert_eq!(engine.arena(), wanted);
        for (key, unit, token, error) in [
            (KEY ^ 1, id, 1, HostError::InvalidArtifact),
            (KEY, 0, 1, HostError::Resident(RegistryError::InvalidUnit)),
            (KEY, keep, 1, call(CallError::InvalidToken)),
            (KEY, id, 0, call(CallError::InvalidToken)),
            (KEY, id, u32::MAX, call(CallError::InvalidToken)),
        ] {
            reject(&mut engine, &[id, keep], error, |engine| {
                engine.complete_resident_call(key, unit, token, 55)
            });
        }
        engine.write32(STACK, 0xdead_beef).unwrap();
        for index in 0..count {
            engine
                .write32(STACK + 4 + index * 4, 0xf000_0000 + index)
                .unwrap();
        }
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + CALL_FRAME_SIZE].fill(0xaa);
        let before = saved(&engine, &[id, keep]);
        engine
            .complete_resident_call(KEY, id, 1, 0xa1b2_c3d4)
            .unwrap();
        let after = saved(&engine, &[id, keep]);
        assert_eq!(
            after.arena,
            completed(&before.arena, state, esp, RETURN, 0xa1b2_c3d4)
        );
        assert_eq!(after.units, before.units);
        assert_eq!(after.pages, before.pages);
        assert_eq!(after.generation, before.generation);
        assert_eq!(after.legacy, before.legacy);
        reject(
            &mut engine,
            &[id, keep],
            call(CallError::InvalidToken),
            |engine| engine.complete_resident_call(KEY, id, 1, 66),
        );
        assert_eq!(engine.guard_resident(KEY, id), Ok(()));
        assert_eq!(word(&engine, STACK), 0xdead_beef);
        assert_eq!(
            engine.lookup_resident(RETURN),
            Err(HostError::Resident(RegistryError::NotFound {
                pc: GuestAddress(RETURN)
            }))
        );
    }
}

#[test]
fn stack_read_faults_keep_helper_and_token_while_repair_and_wrap_capture_current_words() {
    for (esp, count, fault_pc, reason) in [
        (0x7000, 0, 0x7000, FaultReason::Unmapped),
        (STACK, 0, STACK, FaultReason::Permission),
        (0x8ff8, 2, 0x9000, FaultReason::Unmapped),
        (0x8ff8, 2, 0x9000, FaultReason::Permission),
        (0xffff_fffd, 0, 0xffff_fffd, FaultReason::AddressOverflow),
    ] {
        let (mut engine, id, keep, _) = fixture();
        if esp == STACK {
            engine.protect(STACK, 1, 2).unwrap();
        }
        if esp == 0x8ff8 {
            engine.write32(esp, RETURN).unwrap();
            engine.write32(esp + 4, 0x1122_3300).unwrap();
            if reason == FaultReason::Permission {
                engine.map(0x9000, 1, 2).unwrap();
            }
        }
        let state = stop(&mut engine, GATE, GATE_ID, esp, 0);
        reject(
            &mut engine,
            &[id, keep],
            call(CallError::Memory(MemoryError::Fault(MemoryFault {
                address: GuestAddress(fault_pc),
                access: Access::Read,
                reason,
            }))),
            |engine| engine.capture_resident_call(KEY, id, CallingConvention32::Cdecl, count),
        );
        assert_eq!(engine.guard_resident(KEY, id), Ok(()));
        if reason == FaultReason::AddressOverflow {
            continue;
        }
        if esp == 0x7000 {
            engine.map(0x7000, 1, 3).unwrap();
        }
        if esp == STACK {
            engine.protect(STACK, 1, 3).unwrap();
        }
        if esp == 0x8ff8 {
            if reason == FaultReason::Unmapped {
                engine.map(0x9000, 1, 3).unwrap();
            } else {
                engine.protect(0x9000, 1, 3).unwrap();
            }
        }
        engine.write32(esp, RETURN + 3).unwrap();
        for index in 0..count {
            engine
                .write32(esp + 4 + index * 4, 0x6677_8800 + index)
                .unwrap();
        }
        let mut expected = expected_record(state, 1, GATE_ID, 1, count);
        expected.return_pc = RETURN + 3;
        for index in 0..count as usize {
            expected.arguments[index] = 0x6677_8800 + index as u32;
        }
        capture(&mut engine, id, expected);
    }
    let (mut engine, id, keep, _) = fixture();
    engine.map(0xffff_f000, 1, 3).unwrap();
    engine.map(0, 1, 3).unwrap();
    for (pc, value) in [(0xffff_fffc, RETURN), (0, 0x1122_3300), (4, 0x1122_3301)] {
        engine.write32(pc, value).unwrap();
    }
    let state = stop(&mut engine, GATE, GATE_ID, 0xffff_fffc, 0);
    capture(&mut engine, id, expected_record(state, 1, GATE_ID, 1, 2));
    let before = saved(&engine, &[id, keep]);
    engine.complete_resident_call(KEY, id, 1, 77).unwrap();
    assert_eq!(
        engine.arena(),
        completed(&before.arena, state, 0, RETURN, 77)
    );
    assert_eq!(saved(&engine, &[id, keep]).pages, before.pages);
}

#[test]
fn snapshot_state_cancel_abandon_and_close_keep_exact_pending_owner_authority() {
    for mutation in [
        "same-byte",
        "protect",
        "remap",
        "whole",
        "cross-first",
        "cross-second",
    ] {
        let (mut engine, mut id, keep, _) = fixture();
        let pc = if mutation.starts_with("cross") {
            upload(&mut engine, 0x1fff, &[0x0f, 0x0b]);
            id = compile(&mut engine, &[(0x1fff, 2)], &[(0x1fff, GATE_ID)]);
            0x1fff
        } else {
            GATE
        };
        let state = stop(&mut engine, pc, GATE_ID, STACK, 0);
        capture(&mut engine, id, expected_record(state, 1, GATE_ID, 1, 2));
        match mutation {
            "same-byte" => engine.write32(GATE, 0x0000_0b0f).unwrap(),
            "protect" => engine.protect(GATE, 1, 5).unwrap(),
            "remap" => {
                engine.unmap(GATE, 1).unwrap();
                engine.map(GATE, 1, 7).unwrap();
                upload(&mut engine, GATE, &[0x0f, 0x0b]);
            }
            "whole" => engine.write32(WHOLE, 0x0000_eb90).unwrap(),
            "cross-first" => engine.write32(GATE, 0x0000_0b0f).unwrap(),
            "cross-second" => engine.write32(WHOLE, 0x0000_eb0b).unwrap(),
            _ => unreachable!(),
        }
        engine.arena_mut().unwrap()[16] ^= 1;
        engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
        reject(&mut engine, &[id, keep], stale(), |engine| {
            engine.complete_resident_call(KEY, id, 0, 55)
        });
        reject(&mut engine, &[id, keep], stale(), |engine| {
            engine.capture_resident_call_raw(KEY, id, 0, 17)
        });
        assert_eq!(engine.guard_resident(KEY, keep), Err(call(CallError::Busy)));
        reject(&mut engine, &[id, keep], call(CallError::Busy), |engine| {
            engine.compile_resident_with_gates(1, 1)
        });
        let before = engine.arena().to_vec();
        engine.abandon_call(KEY, 1).unwrap();
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.guard_resident(KEY, keep), Ok(()));
        if mutation == "protect" {
            engine.protect(GATE, 1, 7).unwrap();
        }
        upload(&mut engine, pc, &[0x0f, 0x0b]);
        let current = compile(&mut engine, &[(pc, 2)], &[(pc, GATE_ID)]);
        assert!(current > id);
        assert_eq!(engine.lookup_resident(pc).unwrap().get(), current);
        assert_eq!(engine.resident_bytes(id), Err(stale()));
    }
    let (mut engine, id, keep, state) = fixture();
    capture(&mut engine, id, expected_record(state, 1, GATE_ID, 1, 2));
    engine.arena_mut().unwrap()[16] ^= 1;
    engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    reject(
        &mut engine,
        &[id, keep],
        call(CallError::InvalidToken),
        |engine| engine.complete_resident_call(KEY, id, 0, 55),
    );
    reject(
        &mut engine,
        &[id, keep],
        call(CallError::StateChanged),
        |engine| engine.complete_resident_call(KEY, id, 1, 55),
    );
    engine.arena_mut().unwrap()[16] ^= 1;
    reject(
        &mut engine,
        &[id, keep],
        call(CallError::Cancelled),
        |engine| engine.complete_resident_call(KEY, id, 1, 55),
    );
    reject(
        &mut engine,
        &[id, keep],
        HostError::InvalidArtifact,
        |engine| engine.abandon_call(KEY ^ 1, 1),
    );
    reject(
        &mut engine,
        &[id, keep],
        call(CallError::InvalidToken),
        |engine| engine.abandon_call(KEY, 0),
    );
    let before = engine.arena().to_vec();
    engine.abandon_call(KEY, 1).unwrap();
    assert_eq!(engine.arena(), before);
    let state = stop(&mut engine, GATE, GATE_ID, STACK, 0);
    capture(&mut engine, id, expected_record(state, 2, GATE_ID, 1, 2));
    engine.write32(KEEP, 0x0000_0b0f).unwrap();
    assert_eq!(engine.resident_bytes(keep), Err(stale()));
    let before = engine.arena().to_vec();
    engine.complete_resident_call(KEY, id, 2, 99).unwrap();
    assert_eq!(
        engine.arena(),
        completed(&before, state, STACK + 4, RETURN, 99)
    );
    let state = stop(&mut engine, GATE, GATE_ID, STACK, 0);
    capture(&mut engine, id, expected_record(state, 3, GATE_ID, 1, 2));
    let before = engine.arena().to_vec();
    engine.close();
    for result in [
        engine
            .capture_resident_call_raw(KEY ^ 1, 0, 0, u32::MAX)
            .map(|_| ()),
        engine.complete_resident_call(KEY ^ 1, 0, 0, 0),
        engine.abandon_call(KEY ^ 1, 0),
    ] {
        assert_eq!(result, Err(HostError::Closed));
    }
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.arena().len(), ARENA_SIZE);
}

#[test]
fn replacement_and_resident_owners_share_tokens_with_explicit_callback_exclusion() {
    let (mut engine, id, keep, _) = fixture();
    let generation = install_legacy(&mut engine);
    engine.map(0x7000, 1, 3).unwrap();
    let state = stop(&mut engine, GATE, GATE_ID, STACK, 0);
    capture(&mut engine, id, expected_record(state, 1, GATE_ID, 1, 2));
    reject(
        &mut engine,
        &[id, keep],
        call(CallError::InvalidToken),
        |engine| engine.complete_call(KEY, generation, 1, 55),
    );
    reject(
        &mut engine,
        &[id, keep],
        call(CallError::InvalidToken),
        |engine| engine.begin_callback(KEY, generation, 1, LEGACY, CALLBACK_RETURN, 18, &[]),
    );
    reject(
        &mut engine,
        &[id, keep],
        call(CallError::InvalidRequest),
        |engine| {
            engine.begin_callback_from_transfer(KEY, generation, 1, LEGACY, CALLBACK_RETURN, 18, 17)
        },
    );
    reject(
        &mut engine,
        &[id, keep],
        HostError::InvalidArtifact,
        |engine| engine.begin_callback(KEY ^ 1, generation, 1, LEGACY, CALLBACK_RETURN, 18, &[]),
    );
    let before = engine.arena().to_vec();
    engine.abandon_call(KEY, 1).unwrap();
    assert_eq!(engine.arena(), before);
    // the existing legacy typed stop parks its own owner solely to check coexistence.
    stop(&mut engine, GATE, GATE_ID, STACK, 0);
    let outer = engine
        .capture_call(KEY, generation, CallingConvention32::Cdecl, 2)
        .unwrap();
    assert_eq!(outer.token, 2);
    reject(
        &mut engine,
        &[id, keep],
        call(CallError::InvalidToken),
        |engine| engine.complete_resident_call(KEY, id, outer.token, 55),
    );
    let callback = engine
        .begin_callback(
            KEY,
            generation,
            outer.token,
            LEGACY,
            CALLBACK_RETURN,
            18,
            &[],
        )
        .unwrap();
    assert_eq!(callback.token, 3);
    reject(&mut engine, &[id, keep], call(CallError::Busy), |engine| {
        engine.capture_resident_call_raw(KEY, id, 0, 17)
    });
    reject(&mut engine, &[id, keep], call(CallError::Busy), |engine| {
        engine.complete_resident_call(KEY, id, outer.token, 55)
    });
    reject(&mut engine, &[id, keep], call(CallError::Busy), |engine| {
        engine.abandon_call(KEY, outer.token)
    });
    assert_eq!(engine.lookup_resident(GATE).unwrap().get(), id);
    assert!(engine.resident_bytes(id).is_ok());
    engine.abort_callback(KEY, callback.token).unwrap();
    let before = engine.arena().to_vec();
    engine
        .complete_call(KEY, generation, outer.token, 66)
        .unwrap();
    assert_eq!(
        engine.arena(),
        completed(&before, state, STACK + 4, RETURN, 66)
    );
    let state = stop(&mut engine, GATE, GATE_ID, STACK, 0);
    capture(&mut engine, id, expected_record(state, 4, GATE_ID, 1, 0));
    assert_eq!(engine.guard(KEY, generation), Err(call(CallError::Busy)));
    assert_eq!(engine.guard_resident(KEY, id), Err(call(CallError::Busy)));
}
