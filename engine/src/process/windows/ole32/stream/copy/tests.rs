#[path = "../../../service/tests/support.rs"]
mod support;
use super::context::CHUNK;
use crate::{
    abi::{arena::*, x86::*},
    cpu::{ExecutionExit, ExitReason},
    memory::{GuestAddress, MemoryError},
    process::{EngineInstance, HostError},
    windows::{ole32::StreamMethod32, thread::ThreadSchedule32},
};
const KEY: u64 = 77;
const OBJECT: u32 = support::DATA + 4096;
const TABLE: u32 = OBJECT + 128;
const READ: u32 = support::DATA + 8192;
const WRITTEN: u32 = READ + 8;
fn start(bytes: &[u8], count: u64) -> (EngineInstance, u32) {
    let mut engine = support::engine();
    engine
        .configure_synchronization(Default::default())
        .unwrap();
    engine
        .ole32
        .as_mut()
        .unwrap()
        .apartments
        .initialize(1, 0, 0)
        .unwrap();
    engine.write32(OBJECT, TABLE).unwrap();
    engine.write32(TABLE + 16, 0x401000).unwrap();
    let source = engine
        .create_com_stream(bytes.len().max(1), bytes)
        .unwrap()
        .0;
    support::put(&mut engine, READ, &[0xa5; 16]);
    let (generation, token) = support::capture(
        &mut engine,
        StreamMethod32(7).id(),
        &[
            source,
            OBJECT,
            count as u32,
            (count >> 32) as u32,
            READ,
            WRITTEN,
        ],
    );
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
    (engine, source)
}
fn enter(engine: &mut EngineInstance) -> (Vec<u8>, u32, u32) {
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    let state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    assert_eq!(state.eip, 0x401000);
    let esp = state.registers[4];
    assert_eq!(support::word(engine, esp + 4), OBJECT);
    let address = support::word(engine, esp + 8);
    let count = support::word(engine, esp + 12);
    let output = support::word(engine, esp + 16);
    assert!(count as usize <= CHUNK);
    let mut bytes = vec![0; count as usize];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    (bytes, output, esp + 20)
}
fn returned(engine: &mut EngineInstance, output: u32, esp: u32, written: u32, result: u32) {
    engine.write32(output, written).unwrap();
    let id = crate::process::windows::threads::INVOCATION_RETURN_ID;
    let pc = engine
        .module_gates()
        .unwrap()
        .iter()
        .find(|gate| gate.id == id)
        .unwrap()
        .entry
        .0;
    for (index, value) in [pc, 2, pc, id].iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    let generation = engine.compile_with_gates(1, 1).unwrap();
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = pc;
    state.registers[4] = esp;
    state.registers[0] = result;
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
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
}
fn count(engine: &EngineInstance, address: u32) -> u64 {
    u64::from(support::word(engine, address))
        | (u64::from(support::word(engine, address + 4)) << 32)
}
#[test]
fn guest_write_receives_bounded_chunks_and_maximum_count_copies_the_remainder() {
    let bytes: Vec<u8> = (0..10000).map(|index| index as u8).collect();
    let (mut engine, source) = start(&bytes, u64::MAX);
    let mut copied = Vec::new();
    for expected in [CHUNK, CHUNK, 10000 - CHUNK * 2] {
        let (chunk, output, esp) = enter(&mut engine);
        assert_eq!(chunk.len(), expected);
        copied.extend_from_slice(&chunk);
        returned(&mut engine, output, esp, chunk.len() as u32, 0);
    }
    engine.schedule_windows_thread().unwrap();
    assert_eq!(copied, bytes);
    assert_eq!(support::result(&engine), 0);
    assert_eq!(count(&engine, READ), 10000);
    assert_eq!(count(&engine, WRITTEN), 10000);
    let runtime = engine.ole32.as_ref().unwrap();
    assert!(runtime.streams.copies.is_empty());
    let id = runtime.stream_id(engine.memory().unwrap(), source).unwrap();
    assert_eq!(runtime.streams.store.position(id), Ok(10000));
    assert_eq!(runtime.com.references(GuestAddress(source)), Ok(1));
}
#[test]
fn short_success_and_failed_write_preserve_separate_partial_counts_and_hresult() {
    for (result, expected) in [(0, 0x8003_0070), (0x8000_4005, 0x8000_4005)] {
        let (mut engine, source) = start(b"abcdefg", 7);
        let (chunk, output, esp) = enter(&mut engine);
        assert_eq!(&chunk, b"abcdefg");
        returned(&mut engine, output, esp, 2, result);
        engine.schedule_windows_thread().unwrap();
        assert_eq!(support::result(&engine), expected);
        assert_eq!(count(&engine, READ), 7);
        assert_eq!(count(&engine, WRITTEN), 2);
        let runtime = engine.ole32.as_ref().unwrap();
        let id = runtime.stream_id(engine.memory().unwrap(), source).unwrap();
        assert_eq!(runtime.streams.store.position(id), Ok(7));
        assert!(runtime.streams.copies.is_empty());
    }
}
#[test]
fn exhausted_result_publication_retries_without_repeating_the_completed_write() {
    let (mut engine, _) = start(b"abcdefg", 7);
    let (_, output, esp) = enter(&mut engine);
    returned(&mut engine, output, esp, 3, 0x80004005);
    let version = engine.memory().unwrap().transaction_version();
    engine.memory.as_mut().unwrap().exhaust_versions_for_test();
    assert_eq!(
        engine.schedule_windows_thread(),
        Err(HostError::Memory(MemoryError::VersionExhausted))
    );
    assert_eq!(count(&engine, READ), 0xa5a5_a5a5_a5a5_a5a5);
    assert_eq!(count(&engine, WRITTEN), 0xa5a5_a5a5_a5a5_a5a5);
    engine
        .memory
        .as_mut()
        .unwrap()
        .restore_version_for_test(version);
    engine.schedule_windows_thread().unwrap();
    assert_eq!(support::result(&engine), 0x80004005);
    assert_eq!(count(&engine, READ), 7);
    assert_eq!(count(&engine, WRITTEN), 3);
    assert!(engine.ole32.as_ref().unwrap().streams.copies.is_empty());
    assert!(engine.active_com_apartment_call(1).is_none());
}
#[test]
fn replaced_foreign_object_or_result_mapping_is_not_adopted_after_write_returns() {
    for address in [OBJECT, READ] {
        let (mut engine, source) = start(b"abcdefg", 7);
        let (_, output, esp) = enter(&mut engine);
        returned(&mut engine, output, esp, 7, 0);
        engine.unmap(address, 1).unwrap();
        engine.map(address, 1, 3).unwrap();
        if address == OBJECT {
            engine.write32(OBJECT, TABLE).unwrap();
            engine.write32(TABLE + 16, 0x401000).unwrap();
        } else {
            support::put(&mut engine, READ, &[0xa5; 16]);
        }
        let arena = engine.arena().to_vec();
        let version = engine.memory().unwrap().transaction_version();
        assert_eq!(
            engine.schedule_windows_thread(),
            Err(HostError::Memory(MemoryError::InvalidRange))
        );
        assert_eq!(engine.arena(), arena.as_slice());
        assert_eq!(engine.memory().unwrap().transaction_version(), version);
        let copy = &engine.ole32.as_ref().unwrap().streams.copies[0];
        assert_eq!(copy.written, 0);
        // Retirement releases only engine-owned scratch/hold; replacement application pages survive.
        let prepared = engine.prepare_com_thread_exit(1).unwrap();
        engine.check_com_thread_exit(&prepared).unwrap();
        let ranges = prepared.ranges().to_vec();
        engine
            .memory
            .as_mut()
            .unwrap()
            .unmap_ranges_with_word_writes(&ranges, &[])
            .unwrap();
        engine.commit_com_thread_exit_after_memory(prepared, true);
        assert!(
            engine
                .memory()
                .unwrap()
                .permissions_at(GuestAddress(address))
                .is_ok()
        );
        assert!(engine.ole32.as_ref().unwrap().streams.copies.is_empty());
        assert_eq!(
            engine
                .ole32
                .as_ref()
                .unwrap()
                .com
                .references(GuestAddress(source)),
            Ok(1)
        );
    }
}
#[test]
fn released_engine_destination_with_application_vtable_bytes_stays_engine_owned() {
    let mut engine = support::engine();
    engine
        .ole32
        .as_mut()
        .unwrap()
        .apartments
        .initialize(1, 0, 0)
        .unwrap();
    let source = engine.create_com_stream(8, b"abc").unwrap();
    let destination = engine.create_com_stream(8, b"").unwrap();
    let index = engine
        .ole32
        .as_ref()
        .unwrap()
        .stream_index(engine.memory().unwrap(), destination.0)
        .unwrap();
    engine
        .ole32
        .as_mut()
        .unwrap()
        .release_stream(engine.memory.as_mut().unwrap(), index)
        .unwrap();
    let page = destination.0 & !4095;
    if engine
        .memory()
        .unwrap()
        .permissions_at(GuestAddress(page))
        .is_err()
    {
        engine.map(page, 1, 3).unwrap();
    }
    engine.write32(destination.0, TABLE).unwrap();
    engine.write32(TABLE + 16, 0x401000).unwrap();
    let (generation, token) = support::capture(
        &mut engine,
        StreamMethod32(7).id(),
        &[source.0, destination.0, 3, 0, READ, WRITTEN],
    );
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
    assert_eq!(support::result(&engine), 0x8003_0006);
    assert!(engine.ole32.as_ref().unwrap().streams.copies.is_empty());
    assert!(!engine.has_pending_thread_invocation(1));
    let runtime = engine.ole32.as_ref().unwrap();
    let id = runtime
        .stream_id(engine.memory().unwrap(), source.0)
        .unwrap();
    assert_eq!(runtime.streams.store.position(id), Ok(0));
}
#[test]
fn source_release_during_application_write_is_held_until_one_terminal_ram_publication() {
    let (mut engine, source) = start(b"abcdefg", 7);
    let (_, output, esp) = enter(&mut engine);
    let index = engine
        .ole32
        .as_ref()
        .unwrap()
        .stream_index(engine.memory().unwrap(), source)
        .unwrap();
    assert_eq!(
        engine
            .ole32
            .as_mut()
            .unwrap()
            .release_stream(engine.memory.as_mut().unwrap(), index),
        Ok(0)
    );
    assert!(
        engine
            .ole32
            .as_ref()
            .unwrap()
            .com
            .lookup(GuestAddress(source))
            .is_ok()
    );
    returned(&mut engine, output, esp, 7, 0);
    let version = engine.memory().unwrap().transaction_version();
    engine.schedule_windows_thread().unwrap();
    assert_eq!(engine.memory().unwrap().transaction_version(), version + 1);
    let runtime = engine.ole32.as_ref().unwrap();
    assert!(runtime.com.lookup(GuestAddress(source)).is_err());
    assert_eq!(runtime.streams.store.reserved_bytes(), 0);
    assert_eq!(count(&engine, READ), 7);
    assert_eq!(count(&engine, WRITTEN), 7);
}
