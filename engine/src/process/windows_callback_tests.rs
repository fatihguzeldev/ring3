use super::*;
use crate::{
    abi::{
        arena::{STATE_OFFSET, TRANSFER_OFFSET},
        resident_callback::ResidentCallbackRecord32,
        x86::{STATE_SIZE, decode_state, encode_exit_v3, encode_state},
    },
    cpu::x86::State32,
    process::{ResidentInstallation, call::PendingCall, callback::SuspendedRecord},
    windows::CallingConvention32,
};

const KEY: u64 = 0xa360_1122_3344_5566;
const PAGES: [u32; 4] = [0x4000, 0x5000, 0x6000, 0x8000];

type SavedCall = (
    u32,
    PendingOwner,
    CallFrame32,
    [u8; STATE_SIZE],
    [u8; EXIT_SIZE],
);

#[derive(Debug, PartialEq, Eq)]
struct Saved {
    arena: Vec<u8>,
    inner: SavedCall,
    outer: SavedCall,
    callback: (ResidentCallbackRecord32, bool, u64),
    call_token: u32,
    last_error: u32,
    memory_identity: u64,
    mapped_pages: u32,
    ram: Vec<Vec<u8>>,
    modules: Vec<(Vec<u8>, usize)>,
    installations: Vec<Option<ResidentInstallation>>,
}

fn call(call: &PendingCall) -> SavedCall {
    (call.token, call.owner, call.frame, call.state, call.exit)
}

fn stop(engine: &mut EngineInstance, state: State32, reason: ExitReason) {
    encode_state(
        &state,
        &mut engine.arena_mut().unwrap()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE],
    )
    .unwrap();
    encode_exit_v3(
        &ExecutionExit { retired: 0, reason },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
}

fn fixture(selected: bool) -> (EngineInstance, Vec<u64>, u32, u64) {
    let mut engine = EngineInstance::new(7, KEY).unwrap();
    for address in PAGES {
        engine.map(address, 1, 7).unwrap();
    }
    for (address, bytes) in [
        (0x4000, &[0x0f, 0x0b][..]),
        (0x5000, &[0x90][..]),
        (0x5100, &[0x0f, 0x0b][..]),
        (0x5200, &[0x0f, 0x0b][..]),
        (0x6000, &[0x90][..]),
        (0x6100, &[0x0f, 0x0b][..]),
    ] {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(address), bytes)
            .unwrap();
    }
    let mut ids = Vec::new();
    for (blocks, gates, words) in [
        (1, 1, vec![0x4000, 2, 0x4000, 17]),
        (
            3,
            2,
            vec![
                0x5000,
                1,
                0x5100,
                2,
                0x5200,
                2,
                0x5100,
                WindowsApi32::VirtualAlloc.id(),
                0x5200,
                18,
            ],
        ),
        (
            2,
            1,
            vec![
                0x6000,
                1,
                0x6100,
                2,
                0x6100,
                WindowsApi32::VirtualAlloc.id(),
            ],
        ),
    ] {
        for (index, word) in words.into_iter().enumerate() {
            let at = TRANSFER_OFFSET + index * 4;
            engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&word.to_le_bytes());
        }
        let id = engine
            .compile_resident_with_gates(blocks, gates)
            .unwrap()
            .get();
        engine
            .acknowledge_resident_installation(KEY, id, ids.len() as u32)
            .unwrap();
        ids.push(id);
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x8ffc), &0x9000_u32.to_le_bytes())
        .unwrap();
    let mut state = State32::default();
    state.eip = 0x4000;
    state.eflags = 0xcd7;
    state.registers = [
        0x1234_5678,
        0x2345_6789,
        0x3456_789a,
        0x4567_89ab,
        0x8ffc,
        0x5678_9abc,
        0x6789_abcd,
        0x789a_bcde,
    ];
    stop(&mut engine, state, ExitReason::Gate { id: 17 });
    let outer = engine
        .capture_resident_call(KEY, ids[0], CallingConvention32::Cdecl, 0)
        .unwrap();
    let callback = engine
        .begin_resident_callback(KEY, ids[0], ids[1], outer.token, 0x5000, 0x5200, 18, &[])
        .unwrap();
    engine
        .authorize_resident_callback(KEY, ids[1], callback.token)
        .unwrap();
    let active = if selected { ids[2] } else { ids[1] };
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    if selected {
        state.eip = 0x6000;
        stop(&mut engine, state, ExitReason::NeedCode);
        engine
            .select_resident_callback_unit(KEY, ids[1], callback.token, active)
            .unwrap();
    }
    for (index, value) in [0x5001_u32, 0, 4097, 0x3000, 4].into_iter().enumerate() {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(
                GuestAddress(0x8fe0 + index as u32 * 4),
                &value.to_le_bytes(),
            )
            .unwrap();
    }
    state.eip = if selected { 0x6100 } else { 0x5100 };
    state.registers[4] = 0x8fe0;
    stop(
        &mut engine,
        state,
        ExitReason::Gate {
            id: WindowsApi32::VirtualAlloc.id(),
        },
    );
    engine
        .capture_active_resident_callback_call(
            KEY,
            active,
            callback.token,
            CallingConvention32::Stdcall,
            4,
        )
        .unwrap();
    (engine, ids, callback.token, active)
}

fn saved(engine: &EngineInstance, ids: &[u64]) -> Saved {
    let memory = engine.memory.as_ref().unwrap();
    let inner = engine.pending_call.as_ref().unwrap();
    let callback = engine.callback.as_ref().unwrap();
    let SuspendedRecord::Resident {
        record,
        authorized,
        active_unit_id,
    } = callback.record
    else {
        unreachable!()
    };
    let frame = CallFrame32::capture(
        memory,
        *inner.frame.state(),
        CallingConvention32::Stdcall,
        0,
    )
    .unwrap();
    let (error, _) = engine
        .windows_thread
        .prepare(
            WindowsApi32::GetLastError,
            &frame,
            ProcessContext32::default(),
        )
        .unwrap();
    let WindowsOutcome32::Return(last_error) = error else {
        unreachable!()
    };
    Saved {
        arena: engine.arena().to_vec(),
        inner: call(inner),
        outer: call(&callback.outer),
        callback: (record, authorized, active_unit_id),
        call_token: engine.call_token,
        last_error,
        memory_identity: memory.identity(),
        mapped_pages: memory.mapped_pages(),
        ram: PAGES
            .into_iter()
            .map(|address| {
                let mut bytes = vec![0; 4096];
                memory.read(GuestAddress(address), &mut bytes).unwrap();
                bytes
            })
            .collect(),
        modules: ids
            .iter()
            .map(|id| {
                let bytes = engine
                    .guard_resident_unit(KEY, *id)
                    .unwrap()
                    .wasm_bytes(memory)
                    .unwrap();
                (bytes.to_vec(), bytes.as_ptr() as usize)
            })
            .collect(),
        installations: engine.resident_installations.to_vec(),
    }
}

#[test]
fn exhausted_allocation_version_keeps_callback_inner_outer_and_published_units_exact() {
    for selected in [false, true] {
        let (mut engine, ids, callback_token, active) = fixture(selected);
        engine.memory.as_mut().unwrap().exhaust_versions_for_test();
        let before = saved(&engine, &ids);
        for _ in 0..2 {
            assert_eq!(
                engine.complete_active_resident_callback_windows_call(
                    KEY,
                    active,
                    callback_token,
                    before.inner.0
                ),
                Err(HostError::Memory(MemoryError::VersionExhausted))
            );
            assert_eq!(saved(&engine, &ids), before);
            assert_eq!(engine.memory.as_ref().unwrap().mapped_pages(), 4);
        }
    }
}

#[test]
fn invalid_internal_return_fails_before_mapping_and_valid_repair_keeps_callback_armed() {
    for selected in [false, true] {
        let (mut engine, ids, callback_token, active) = fixture(selected);
        let mut state = *engine.pending_call.as_ref().unwrap().frame.state();
        state.eflags = 0;
        engine.pending_call.as_mut().unwrap().frame = CallFrame32::capture(
            engine.memory.as_ref().unwrap(),
            state,
            CallingConvention32::Stdcall,
            4,
        )
        .unwrap();
        let before = saved(&engine, &ids);
        assert_eq!(
            engine.complete_active_resident_callback_windows_call(
                KEY,
                active,
                callback_token,
                before.inner.0
            ),
            Err(HostError::Infrastructure)
        );
        assert_eq!(saved(&engine, &ids), before);
        state.eflags = 0xcd7;
        engine.pending_call.as_mut().unwrap().frame = CallFrame32::capture(
            engine.memory.as_ref().unwrap(),
            state,
            CallingConvention32::Stdcall,
            4,
        )
        .unwrap();
        engine
            .complete_active_resident_callback_windows_call(
                KEY,
                active,
                callback_token,
                before.inner.0,
            )
            .unwrap();
        assert!(engine.pending_call.is_none());
        let callback = engine.callback.as_ref().unwrap();
        assert_eq!(call(&callback.outer), before.outer);
        assert_eq!(
            callback.authorized_resident_record(),
            Some(before.callback.0)
        );
        assert_eq!(callback.authorized_resident_active_id(), Some(active));
        assert_eq!(engine.call_token, before.call_token);
        let state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
        assert_eq!(state.registers[0], 0x1000_0000);
        assert_eq!(state.eflags, 0xcd7);
        assert_eq!(engine.memory.as_ref().unwrap().mapped_pages(), 6);
        engine.guard_resident(KEY, active).unwrap();
        for id in ids {
            engine.guard_resident_unit(KEY, id).unwrap();
        }
    }
}
