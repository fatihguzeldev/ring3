use ring3_engine::abi::x86::{decode_state, encode_exit_v3, encode_state};
use ring3_engine::cpu::x86::State32;
use ring3_engine::cpu::{ExecutionExit, ExitReason};
use ring3_engine::memory::{Access, FaultReason, GuestAddress, MemoryError, MemoryFault};
use ring3_engine::process::{CallError, EngineInstance, HostError};
use ring3_engine::windows::CallingConvention32;

const KEY: u64 = 0xfedc_ba98_1234_5678;

fn batch(engine: &mut EngineInstance, words: &[(u32, u32)]) {
    let transfer = &mut engine.arena_mut().unwrap()[140..];
    transfer.fill(0xcc);
    for (index, (address, value)) in words.iter().enumerate() {
        transfer[index * 8..index * 8 + 4].copy_from_slice(&address.to_le_bytes());
        transfer[index * 8 + 4..index * 8 + 8].copy_from_slice(&value.to_le_bytes());
    }
}

fn bytes(engine: &EngineInstance, address: u32, length: usize) -> Vec<u8> {
    let mut output = vec![0; length];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut output)
        .unwrap();
    output
}

fn unchanged(engine: &mut EngineInstance, count: u32, expected: Result<(), HostError>) {
    let arena = engine.arena().to_vec();
    let generation = engine.generation();
    let artifact = engine.artifact_bytes().map(|value| value.to_vec());
    let key = engine.key();
    assert_eq!(engine.write_words32(count), expected);
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.generation(), generation);
    assert_eq!(engine.key(), key);
    assert_eq!(
        engine.artifact_bytes().map(|value| value.to_vec()),
        artifact
    );
}

fn compile_gate(engine: &mut EngineInstance) -> u32 {
    engine.arena_mut().unwrap()[140..156]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0, 0, 0x10, 0, 0, 17, 0, 0, 0]);
    engine.compile_with_gates(1, 1).unwrap()
}

fn with_gate() -> EngineInstance {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x8000, 1, 3).unwrap();
    engine.arena_mut().unwrap()[140..142].copy_from_slice(&[0x0f, 0x0b]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[140..148].copy_from_slice(&[0, 0x20, 0, 0, 0x98, 0xba, 0xdc, 0xfe]);
    engine.upload(0x8000, 8).unwrap();
    assert_eq!(compile_gate(&mut engine), 1);
    engine
}

fn inject_gate_stop(engine: &mut EngineInstance) -> State32 {
    // native injection supplies a canonical gate stop; translated execution is covered separately.
    let state = State32 {
        registers: [
            0x89ab_cdef,
            0x1357_9bdf,
            0x2345_6789,
            0x3456_789a,
            0x8000,
            0x5678_9abc,
            0x6789_abcd,
            0x789a_bcde,
        ],
        eip: 0x1000,
        eflags: 0xcd7,
    };
    let arena = engine.arena_mut().unwrap();
    encode_state(&state, &mut arena[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 2,
            reason: ExitReason::Gate { id: 17 },
        },
        &mut arena[56..96],
    )
    .unwrap();
    arena[96..100].fill(0);
    arena[100..140].fill(0x5a);
    state
}

#[test]
fn empty_and_write_only_single_word_batches_need_no_artifact_or_cpu_header() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.arena_mut().unwrap()[..140].fill(0xa5);
    batch(&mut engine, &[(u32::MAX, u32::MAX)]);
    unchanged(&mut engine, 0, Ok(()));
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    engine.map(0x8000, 1, 2).unwrap();
    batch(&mut engine, &[(0x8001, 0x4433_2211)]);
    unchanged(&mut engine, 1, Ok(()));
    assert_eq!(engine.generation(), 0);
    assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0x8001), Access::Read)
            .is_err()
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0x8001), Access::Execute)
            .is_err()
    );
    engine.protect(0x8000, 1, 1).unwrap();
    assert_eq!(bytes(&engine, 0x8000, 6), [0, 0x11, 0x22, 0x33, 0x44, 0]);
}

#[test]
fn oversized_counts_reject_before_guest_reads_and_keep_the_transfer_and_helper() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(0x8000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[..140].fill(0xa5);
    batch(&mut engine, &[(u32::MAX, 0)]);
    let snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x8000), 4)
        .unwrap();
    for count in [18, u32::MAX] {
        unchanged(&mut engine, count, Err(HostError::InvalidRequest));
    }
    assert_eq!(bytes(&engine, 0x8000, 4), [0; 4]);
    assert!(engine.memory().unwrap().is_code_current(&snapshot));
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
}

#[test]
fn closed_precedes_zero_or_invalid_count_and_preserves_the_arena_tombstone() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    batch(&mut engine, &[(u32::MAX, u32::MAX)]);
    let arena = engine.arena().to_vec();
    engine.close();
    for count in [0, 1, 18, u32::MAX] {
        unchanged(&mut engine, count, Err(HostError::Closed));
    }
    assert_eq!(engine.arena(), arena);
}

#[test]
fn all_seventeen_transfer_pairs_are_read_and_a_last_descriptor_fault_is_atomic() {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x3000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[..140].fill(0xa5);
    let mut writes = std::array::from_fn::<_, 17, _>(|index| {
        (
            if index == 16 {
                0x3000
            } else {
                0x1000 + index as u32 * 4
            },
            0x0102_0300 + index as u32,
        )
    });
    batch(&mut engine, &writes);
    unchanged(&mut engine, 17, Ok(()));
    for index in 0..16u8 {
        assert_eq!(
            bytes(&engine, 0x1000 + u32::from(index) * 4, 4),
            [index, 3, 2, 1]
        );
    }
    assert_eq!(bytes(&engine, 0x3000, 4), [16, 3, 2, 1]);
    let first = bytes(&engine, 0x1000, 64);
    let last = bytes(&engine, 0x3000, 4);
    let snapshots = [0x1000, 0x3000].map(|address| {
        engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(address), 4)
            .unwrap()
    });
    for entry in &mut writes[..16] {
        entry.1 = u32::MAX;
    }
    writes[16] = (0x5000, 0);
    batch(&mut engine, &writes);
    unchanged(
        &mut engine,
        17,
        Err(HostError::Memory(MemoryError::Fault(MemoryFault {
            address: GuestAddress(0x5000),
            access: Access::Write,
            reason: FaultReason::Unmapped,
        }))),
    );
    assert_eq!(bytes(&engine, 0x1000, 64), first);
    assert_eq!(bytes(&engine, 0x3000, 4), last);
    for snapshot in snapshots {
        assert!(engine.memory().unwrap().is_code_current(&snapshot));
    }
    assert_eq!(engine.memory().unwrap().mapped_pages(), 2);
}

#[test]
fn later_permission_fault_preserves_data_installed_code_and_all_arena_bytes() {
    let mut engine = with_gate();
    engine.map(0x9000, 1, 1).unwrap();
    engine.arena_mut().unwrap()[100..140].fill(0xa5);
    batch(&mut engine, &[(0x8004, 0), (0x9000, 1), (0x1000, u32::MAX)]);
    unchanged(
        &mut engine,
        3,
        Err(HostError::Memory(MemoryError::Fault(MemoryFault {
            address: GuestAddress(0x9000),
            access: Access::Write,
            reason: FaultReason::Permission,
        }))),
    );
    assert_eq!(
        bytes(&engine, 0x8000, 8),
        [0, 0x20, 0, 0, 0x98, 0xba, 0xdc, 0xfe]
    );
    assert_eq!(bytes(&engine, 0x9000, 4), [0; 4]);
    assert_eq!(bytes(&engine, 0x1000, 4), [0x0f, 0x0b, 0, 0]);
    engine.guard(KEY, 1).unwrap();
}

#[test]
fn pending_data_batches_leave_private_inputs_and_single_use_completion_intact() {
    let mut engine = with_gate();
    let mut expected = inject_gate_stop(&mut engine);
    let record = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 1)
        .unwrap();
    assert_eq!(record.token, 1);
    assert_eq!(record.arguments[0], 0xfedc_ba98);
    batch(&mut engine, &[(0x8000, 0xdead_beef), (0x8004, 0x1234_5678)]);
    unchanged(&mut engine, 18, Err(HostError::InvalidRequest));
    unchanged(&mut engine, 0, Ok(()));
    unchanged(&mut engine, 2, Ok(()));
    assert_eq!(
        bytes(&engine, 0x8000, 8),
        [0xef, 0xbe, 0xad, 0xde, 0x78, 0x56, 0x34, 0x12]
    );
    assert_eq!(engine.guard(KEY, 1), Err(HostError::Call(CallError::Busy)));
    engine.complete_call(KEY, 1, record.token, 55).unwrap();
    expected.registers[0] = 55;
    expected.registers[4] = 0x8004;
    expected.eip = 0x2000;
    assert_eq!(decode_state(&engine.arena()[..56]), Ok(expected));
    inject_gate_stop(&mut engine);
    let next = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
        .unwrap();
    assert_eq!(next.token, 2);
    assert_eq!(next.return_pc, 0xdead_beef);
}

#[test]
fn pending_code_batches_commit_without_cpu_effects_then_guard_reports_stale() {
    let mut engine = with_gate();
    inject_gate_stop(&mut engine);
    let record = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
        .unwrap();
    batch(&mut engine, &[(0x1000, 0x0000_0b0f), (0x8004, 0x1234_5678)]);
    let arena = engine.arena().to_vec();
    assert_eq!(engine.write_words32(2), Ok(()));
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.generation(), 1);
    assert_eq!(bytes(&engine, 0x1000, 4), [0x0f, 0x0b, 0, 0]);
    assert_eq!(bytes(&engine, 0x8004, 4), [0x78, 0x56, 0x34, 0x12]);
    assert_eq!(engine.guard(KEY, 1), Err(HostError::CodeInvalidated));
    assert_eq!(engine.artifact_bytes(), Err(HostError::CodeInvalidated));
    batch(&mut engine, &[(0x8004, 0x4433_2211)]);
    unchanged(&mut engine, 1, Ok(()));
    assert_eq!(bytes(&engine, 0x8004, 4), [0x11, 0x22, 0x33, 0x44]);
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.complete_call(KEY, 1, record.token, 7),
        Err(HostError::CodeInvalidated)
    );
    assert_eq!(engine.compile(0), Err(HostError::Call(CallError::Busy)));
    assert_eq!(engine.arena(), before);
    engine.abandon_call(KEY, record.token).unwrap();
    assert_eq!(engine.arena(), before);
    assert_eq!(compile_gate(&mut engine), 2);
    inject_gate_stop(&mut engine);
    assert_eq!(
        engine
            .capture_call(KEY, 2, CallingConvention32::Cdecl, 0)
            .unwrap()
            .token,
        2
    );
}
