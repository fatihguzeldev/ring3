use ring3_engine::{
    abi::x86::{encode_exit_v3, encode_state},
    cpu::{
        ExecutionExit, ExitReason, UnsupportedFeature,
        dbt::{CompileError, InstructionError, RegistryError},
        x86::{State32, decode::DecodeError},
    },
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError},
    windows::CallingConvention32,
};

const KEY: u64 = 0x1020_3040_5060_7080;
const A: u32 = 0x1000;
const B: u32 = 0x2000;
const BAD: u32 = 0x3000;
const A_CODE: [u8; 6] = [0x40, 0xe9, 0xfa, 0x0f, 0, 0];
const B_CODE: [u8; 7] = [0x49, 0x0f, 0x85, 0xf9, 0xef, 0xff, 0xff];

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn describe(engine: &mut EngineInstance, specs: &[(u32, u32)]) {
    let transfer = &mut engine.arena_mut().unwrap()[140..];
    transfer.fill(0xa5);
    for (index, &(pc, length)) in specs.iter().enumerate() {
        transfer[index * 8..index * 8 + 4].copy_from_slice(&pc.to_le_bytes());
        transfer[index * 8 + 4..index * 8 + 8].copy_from_slice(&length.to_le_bytes());
    }
}

fn fixture() -> EngineInstance {
    let mut engine = EngineInstance::new(4, KEY).unwrap();
    for pc in [A, B, BAD] {
        engine.map(pc, 1, 7).unwrap();
    }
    upload(&mut engine, A, &A_CODE);
    upload(&mut engine, B, &B_CODE);
    engine
}

fn compile(engine: &mut EngineInstance, pc: u32, length: usize) -> u64 {
    describe(engine, &[(pc, length as u32)]);
    let arena = engine.arena().to_vec();
    let id = engine.compile_resident(1).unwrap().get();
    assert_eq!(engine.arena(), arena);
    id
}

fn preserved(
    engine: &mut EngineInstance,
    ids: &[u64],
    operation: impl FnOnce(&mut EngineInstance),
) {
    let arena = engine.arena().to_vec();
    let address = engine.arena_address();
    let legacy = engine.artifact_bytes().map(|bytes| bytes.to_vec());
    let generation = engine.generation();
    let units: Vec<_> = ids
        .iter()
        .map(|&id| {
            (
                id,
                engine.resident_bytes(id).map(|bytes| bytes.to_vec()),
                engine.resident_bytes(id).ok().map(|bytes| bytes.as_ptr()),
                engine.guard_resident(KEY, id),
            )
        })
        .collect();
    operation(engine);
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.arena_address(), address);
    assert_eq!(engine.generation(), generation);
    assert_eq!(engine.artifact_bytes().map(|bytes| bytes.to_vec()), legacy);
    for (id, bytes, pointer, guard) in units {
        assert_eq!(engine.resident_bytes(id).map(|bytes| bytes.to_vec()), bytes);
        assert_eq!(
            engine.resident_bytes(id).ok().map(|bytes| bytes.as_ptr()),
            pointer
        );
        assert_eq!(engine.guard_resident(KEY, id), guard);
    }
}

fn failed<T>(
    engine: &mut EngineInstance,
    ids: &[u64],
    error: HostError,
    operation: impl FnOnce(&mut EngineInstance) -> Result<T, HostError>,
) {
    preserved(engine, ids, |engine| {
        assert_eq!(operation(engine).err(), Some(error));
    });
}

fn instruction_error(pc: u32, cause: InstructionError) -> HostError {
    HostError::Resident(RegistryError::Compile(CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    }))
}

#[test]
fn resident_lookup_without_units_returns_precise_misses_and_closed_errors() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    for pc in [0, A, u32::MAX] {
        failed(
            &mut engine,
            &[],
            HostError::Resident(RegistryError::NotFound {
                pc: GuestAddress(pc),
            }),
            |engine| engine.lookup_resident(pc),
        );
    }
    engine.map(A, 1, 7).unwrap();
    upload(&mut engine, A, &[0x0f, 0x06]);
    failed(
        &mut engine,
        &[],
        HostError::Resident(RegistryError::NotFound {
            pc: GuestAddress(A),
        }),
        |engine| engine.lookup_resident(A),
    );
    engine.close();
    for pc in [0, A, u32::MAX] {
        failed(&mut engine, &[], HostError::Closed, |engine| {
            engine.lookup_resident(pc)
        });
    }
}

#[test]
fn resident_lookup_selects_exact_unit_entries_and_interiors_without_changing_state() {
    let mut engine = fixture();
    upload(&mut engine, BAD, &[0x0f, 0x06]);
    engine.protect(BAD, 1, 1).unwrap();
    describe(&mut engine, &[(A, A_CODE.len() as u32)]);
    assert_eq!(engine.compile(1), Ok(1));
    let a = compile(&mut engine, A, A_CODE.len());
    let b = compile(&mut engine, B, B_CODE.len());
    preserved(&mut engine, &[a, b], |engine| {
        for (pc, expected) in [(A, a), (A + 1, a), (B, b), (B + 1, b)] {
            assert_eq!(engine.lookup_resident(pc).map(|id| id.get()), Ok(expected));
        }
    });
    for pc in [
        0,
        A + 2,
        A + 5,
        A + 6,
        B + 2,
        B + 6,
        B + 7,
        BAD,
        0x8000,
        u32::MAX,
    ] {
        failed(
            &mut engine,
            &[a, b],
            HostError::Resident(RegistryError::NotFound {
                pc: GuestAddress(pc),
            }),
            |engine| engine.lookup_resident(pc),
        );
    }
}

#[test]
fn resident_compile_keeps_legacy_generation_and_independent_bound_units() {
    let mut engine = fixture();
    describe(&mut engine, &[(A, A_CODE.len() as u32)]);
    assert_eq!(engine.compile(1), Ok(1));
    let legacy = engine.artifact_bytes().unwrap().to_vec();
    let arena = engine.arena().to_vec();
    let a = engine.compile_resident(1).unwrap();
    assert_eq!(engine.arena(), arena);
    let a_bytes = engine.resident_bytes(a.get()).unwrap().to_vec();
    let pointer = engine.resident_bytes(a.get()).unwrap().as_ptr();
    describe(&mut engine, &[(B, B_CODE.len() as u32)]);
    let arena = engine.arena().to_vec();
    let b = engine.compile_resident(1).unwrap();
    assert_ne!(a, b);
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.generation(), 1);
    assert_eq!(engine.artifact_bytes().unwrap(), legacy);
    assert_eq!(engine.resident_bytes(a.get()).unwrap(), a_bytes);
    assert_eq!(engine.resident_bytes(a.get()).unwrap().as_ptr(), pointer);
    assert!(a.get() > 0 && b.get() > a.get());
    assert_eq!(engine.guard_resident(KEY, a.get()), Ok(()));
    assert_eq!(engine.guard_resident(KEY, b.get()), Ok(()));
    assert_eq!(
        engine.guard_resident(KEY, 0),
        Err(HostError::Resident(RegistryError::InvalidUnit))
    );
    let b_bytes = engine.resident_bytes(b.get()).unwrap().to_vec();
    assert_ne!(a_bytes, b_bytes);
    describe(&mut engine, &[(B, B_CODE.len() as u32)]);
    let arena = engine.arena().to_vec();
    assert_eq!(engine.compile(1), Ok(2));
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.guard(KEY, 1), Err(HostError::InvalidArtifact));
    assert_eq!(engine.guard(KEY, 2), Ok(()));
    assert_eq!(engine.guard_resident(KEY, a.get()), Ok(()));
    assert_eq!(engine.guard_resident(KEY, b.get()), Ok(()));
    assert_eq!(engine.resident_bytes(a.get()).unwrap().as_ptr(), pointer);
    assert_eq!(engine.resident_bytes(a.get()).unwrap(), a_bytes);
    assert_eq!(engine.resident_bytes(b.get()).unwrap(), b_bytes);
    let mut output = [0; 7];
    engine
        .memory()
        .unwrap()
        .fetch(GuestAddress(A), &mut output[..6])
        .unwrap();
    assert_eq!(&output[..6], A_CODE);
    engine
        .memory()
        .unwrap()
        .fetch(GuestAddress(B), &mut output)
        .unwrap();
    assert_eq!(output, B_CODE);
}

#[test]
fn raw_id_membership_wrong_key_and_closed_errors_keep_the_arena_unchanged() {
    let mut engine = fixture();
    let a = compile(&mut engine, A, A_CODE.len());
    let b = compile(&mut engine, B, B_CODE.len());
    let mut other = fixture();
    let foreign = compile(&mut other, A, A_CODE.len());
    assert_ne!(foreign, a);
    for id in [0, foreign, a ^ (1_u64 << 32), u64::MAX] {
        assert!(![a, b].contains(&id));
        failed(
            &mut engine,
            &[a, b],
            HostError::Resident(RegistryError::InvalidUnit),
            |engine| engine.guard_resident(KEY, id),
        );
        failed(
            &mut engine,
            &[a, b],
            HostError::Resident(RegistryError::InvalidUnit),
            |engine| engine.resident_bytes(id).map(|_| ()),
        );
        failed(&mut engine, &[a, b], HostError::InvalidArtifact, |engine| {
            engine.guard_resident(KEY ^ 1, id)
        });
    }
    failed(&mut engine, &[a, b], HostError::InvalidArtifact, |engine| {
        engine.guard_resident(KEY ^ (1_u64 << 32), a)
    });
    let arena = engine.arena().to_vec();
    engine.close();
    assert_eq!(engine.arena(), arena);
    for id in [0, a, b, foreign] {
        failed(&mut engine, &[], HostError::Closed, |engine| {
            engine.guard_resident(KEY ^ 1, id)
        });
        failed(&mut engine, &[], HostError::Closed, |engine| {
            engine.resident_bytes(id).map(|_| ())
        });
    }
    failed(&mut engine, &[], HostError::Closed, |engine| {
        engine.compile_resident(0)
    });
    for pc in [0, A, B + 1, u32::MAX] {
        failed(&mut engine, &[], HostError::Closed, |engine| {
            engine.lookup_resident(pc)
        });
    }
    assert_eq!(other.guard_resident(KEY, foreign), Ok(()));
}

#[test]
fn stale_a_rejects_while_b_survives_and_fresh_same_pc_units_keep_distinct_ids() {
    for mutation in 0..3 {
        let mut engine = fixture();
        let old = compile(&mut engine, A, A_CODE.len());
        let b = compile(&mut engine, B, B_CODE.len());
        let old_bytes = engine.resident_bytes(old).unwrap().to_vec();
        let b_bytes = engine.resident_bytes(b).unwrap().to_vec();
        let b_pointer = engine.resident_bytes(b).unwrap().as_ptr();
        match mutation {
            0 => upload(&mut engine, A, &A_CODE),
            1 => engine.protect(A, 1, 7).unwrap(),
            2 => {
                engine.unmap(A, 1).unwrap();
                engine.map(A, 1, 7).unwrap();
                upload(&mut engine, A, &A_CODE);
            }
            _ => unreachable!(),
        }
        failed(
            &mut engine,
            &[old, b],
            HostError::Resident(RegistryError::CodeInvalidated),
            |engine| engine.guard_resident(KEY, old),
        );
        failed(
            &mut engine,
            &[old, b],
            HostError::InvalidArtifact,
            |engine| engine.guard_resident(KEY ^ 1, old),
        );
        assert_eq!(
            engine.resident_bytes(old).err(),
            Some(HostError::Resident(RegistryError::CodeInvalidated))
        );
        for pc in [A, A + 1] {
            failed(
                &mut engine,
                &[old, b],
                HostError::Resident(RegistryError::CodeInvalidated),
                |engine| engine.lookup_resident(pc),
            );
        }
        failed(
            &mut engine,
            &[old, b],
            HostError::Resident(RegistryError::NotFound {
                pc: GuestAddress(A + 2),
            }),
            |engine| engine.lookup_resident(A + 2),
        );
        preserved(&mut engine, &[old, b], |engine| {
            assert_eq!(engine.lookup_resident(B + 1).map(|id| id.get()), Ok(b));
        });
        let fresh = compile(&mut engine, A, A_CODE.len());
        assert!(fresh > old && fresh != b);
        assert_eq!(engine.guard_resident(KEY, fresh), Ok(()));
        assert_eq!(
            engine.guard_resident(KEY, old),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
        assert_eq!(engine.guard_resident(KEY, b), Ok(()));
        assert_eq!(engine.resident_bytes(b).unwrap(), b_bytes);
        assert_eq!(engine.resident_bytes(b).unwrap().as_ptr(), b_pointer);
        assert_ne!(engine.resident_bytes(fresh).unwrap(), old_bytes);
        preserved(&mut engine, &[old, b, fresh], |engine| {
            assert_eq!(engine.lookup_resident(A).map(|id| id.get()), Ok(fresh));
            assert_eq!(engine.lookup_resident(A + 1).map(|id| id.get()), Ok(fresh));
            assert_eq!(engine.lookup_resident(B).map(|id| id.get()), Ok(b));
        });
        let mut source = [0; 6];
        engine
            .memory()
            .unwrap()
            .fetch(GuestAddress(A), &mut source)
            .unwrap();
        assert_eq!(source, A_CODE);
    }
}

#[test]
fn whole_region_snapshot_invalidates_other_block_even_on_unchanged_page() {
    let mut engine = fixture();
    describe(&mut engine, &[(A, 6), (B, 7)]);
    let whole = engine.compile_resident(2).unwrap().get();
    preserved(&mut engine, &[whole], |engine| {
        for pc in [A, A + 1, B, B + 1] {
            assert_eq!(engine.lookup_resident(pc).map(|id| id.get()), Ok(whole));
        }
    });
    let b_snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(B), 7)
        .unwrap();
    upload(&mut engine, A, &A_CODE);
    assert!(engine.memory().unwrap().is_code_current(&b_snapshot));
    assert_eq!(
        engine.guard_resident(KEY, whole),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    for pc in [A, A + 1, B, B + 1] {
        failed(
            &mut engine,
            &[whole],
            HostError::Resident(RegistryError::CodeInvalidated),
            |engine| engine.lookup_resident(pc),
        );
    }
    let fresh_b = compile(&mut engine, B, B_CODE.len());
    assert_eq!(engine.guard_resident(KEY, fresh_b), Ok(()));
    assert_eq!(
        engine.resident_bytes(whole).err(),
        Some(HostError::Resident(RegistryError::CodeInvalidated))
    );
    preserved(&mut engine, &[whole, fresh_b], |engine| {
        assert_eq!(
            engine.lookup_resident(B + 1).map(|id| id.get()),
            Ok(fresh_b)
        );
    });
    failed(
        &mut engine,
        &[whole, fresh_b],
        HostError::Resident(RegistryError::CodeInvalidated),
        |engine| engine.lookup_resident(A),
    );
    failed(
        &mut engine,
        &[whole, fresh_b],
        HostError::Resident(RegistryError::NotFound {
            pc: GuestAddress(BAD),
        }),
        |engine| engine.lookup_resident(BAD),
    );
}

#[test]
fn rejected_counts_spans_and_standalone_profile_keep_legacy_and_resident_state() {
    let mut engine = fixture();
    describe(&mut engine, &[(A, 6)]);
    assert_eq!(engine.compile(1), Ok(1));
    let a = compile(&mut engine, A, A_CODE.len());
    let b = compile(&mut engine, B, B_CODE.len());
    for count in [0, 9, u32::MAX] {
        failed(&mut engine, &[a, b], HostError::InvalidRequest, |engine| {
            engine.compile_resident(count)
        });
    }
    for (bytes, cause) in [
        (
            &[0x0f, 0x06][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Privileged)),
        ),
        (
            &[0x0f, 0x0b][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0x8b, 0x03, 0xeb, 0][..],
            InstructionError::BackendUnsupported,
        ),
        (
            &[0xe8, 0, 0, 0, 0][..],
            InstructionError::BackendUnsupported,
        ),
        (&[0xc3][..], InstructionError::BackendUnsupported),
    ] {
        upload(&mut engine, BAD, bytes);
        describe(&mut engine, &[(BAD, bytes.len() as u32)]);
        failed(
            &mut engine,
            &[a, b],
            instruction_error(BAD, cause),
            |engine| engine.compile_resident(1),
        );
    }
    describe(&mut engine, &[(A, 0)]);
    failed(
        &mut engine,
        &[a, b],
        HostError::Resident(RegistryError::Compile(CompileError::InvalidBlocks)),
        |engine| engine.compile_resident(1),
    );
    describe(&mut engine, &[(A, 5)]);
    failed(
        &mut engine,
        &[a, b],
        instruction_error(A + 1, InstructionError::InvalidBlockEnd),
        |engine| engine.compile_resident(1),
    );
    describe(&mut engine, &[(A, 6)]);
    failed(
        &mut engine,
        &[a, b],
        HostError::Resident(RegistryError::InstructionOverlap {
            pc: GuestAddress(A),
        }),
        |engine| engine.compile_resident(1),
    );
}

#[test]
fn distinct_overlapping_decodings_are_valid_but_instruction_collisions_are_not() {
    let mut engine = fixture();
    upload(&mut engine, A, &[0xb8, 0x90, 0x90, 0xeb, 0, 0xeb, 0]);
    describe(&mut engine, &[(A, 7)]);
    assert_eq!(engine.compile(1), Ok(1));
    let original = compile(&mut engine, A, 7);
    let overlapping = compile(&mut engine, A + 1, 4);
    assert_eq!(engine.guard_resident(KEY, original), Ok(()));
    assert_eq!(engine.guard_resident(KEY, overlapping), Ok(()));
    preserved(&mut engine, &[original, overlapping], |engine| {
        for (pc, expected) in [
            (A, original),
            (A + 5, original),
            (A + 1, overlapping),
            (A + 2, overlapping),
            (A + 3, overlapping),
        ] {
            assert_eq!(engine.lookup_resident(pc).map(|id| id.get()), Ok(expected));
        }
    });
    failed(
        &mut engine,
        &[original, overlapping],
        HostError::Resident(RegistryError::NotFound {
            pc: GuestAddress(A + 4),
        }),
        |engine| engine.lookup_resident(A + 4),
    );
    describe(&mut engine, &[(A + 5, 2)]);
    failed(
        &mut engine,
        &[original, overlapping],
        HostError::Resident(RegistryError::InstructionOverlap {
            pc: GuestAddress(A + 5),
        }),
        |engine| engine.compile_resident(1),
    );
    describe(&mut engine, &[(A, 7), (A + 1, 4)]);
    failed(
        &mut engine,
        &[original, overlapping],
        HostError::Resident(RegistryError::Compile(CompileError::InvalidBlocks)),
        |engine| engine.compile_resident(2),
    );
}

#[test]
fn failed_lazy_admission_does_not_consume_capacity_and_stale_units_still_count() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(A, 1, 7).unwrap();
    for index in 0..8 {
        upload(&mut engine, A + index * 16, &[0x90, 0xeb, 0]);
    }
    upload(&mut engine, A + 128, &[0x0f, 0x06]);
    describe(&mut engine, &[(A + 128, 2)]);
    failed(
        &mut engine,
        &[],
        instruction_error(
            A + 128,
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Privileged)),
        ),
        |engine| engine.compile_resident(1),
    );
    let mut ids = Vec::new();
    for index in 0..8 {
        let id = compile(&mut engine, A + index * 16, 3);
        assert!(id > 0 && ids.iter().all(|&prior| prior != id));
        ids.push(id);
    }
    describe(&mut engine, &[(A + 128, 2)]);
    failed(
        &mut engine,
        &ids,
        HostError::Resident(RegistryError::UnitCapacity),
        |engine| engine.compile_resident(1),
    );
    failed(&mut engine, &ids, HostError::InvalidRequest, |engine| {
        engine.compile_resident(0)
    });
    failed(&mut engine, &ids, HostError::InvalidRequest, |engine| {
        engine.compile_resident(9)
    });
    upload(&mut engine, A, &[0x90, 0xeb, 0]);
    describe(&mut engine, &[(A, 3)]);
    failed(
        &mut engine,
        &ids,
        HostError::Resident(RegistryError::UnitCapacity),
        |engine| engine.compile_resident(1),
    );
    for id in ids {
        assert_eq!(
            engine.guard_resident(KEY, id),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
    }
}

#[test]
fn pending_calls_and_callbacks_block_new_units_without_changing_legacy_callbacks() {
    const OUTER: u32 = BAD + 0x100;
    const ENTRY: u32 = BAD + 0x200;
    const RETURN: u32 = BAD + 0x300;
    let mut engine = fixture();
    engine.map(0x8000, 1, 3).unwrap();
    upload(&mut engine, OUTER, &[0x0f, 0x0b]);
    upload(&mut engine, ENTRY, &[0x90, 0xc3]);
    upload(&mut engine, RETURN, &[0x0f, 0x0b]);
    upload(&mut engine, 0x8010, &0x4000_u32.to_le_bytes());
    describe(&mut engine, &[(OUTER, 2), (ENTRY, 2), (RETURN, 2)]);
    for (index, value) in [OUTER, 17, RETURN, 18].into_iter().enumerate() {
        engine.arena_mut().unwrap()[164 + index * 4..168 + index * 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(engine.compile_with_gates(3, 2), Ok(1));
    let a = compile(&mut engine, A, A_CODE.len());
    let b = compile(&mut engine, B, B_CODE.len());
    let mut state = State32 {
        eip: OUTER,
        ..State32::default()
    };
    state.registers[4] = 0x8010;
    encode_state(&state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 3,
            reason: ExitReason::Gate { id: 17 },
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
    let outer = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
        .unwrap();
    preserved(&mut engine, &[a, b], |engine| {
        assert_eq!(engine.lookup_resident(A + 1).map(|id| id.get()), Ok(a));
        assert_eq!(engine.lookup_resident(B + 1).map(|id| id.get()), Ok(b));
    });
    failed(
        &mut engine,
        &[a, b],
        HostError::Call(CallError::Busy),
        |engine| engine.guard_resident(KEY, b),
    );
    failed(
        &mut engine,
        &[a, b],
        HostError::Call(CallError::Busy),
        |engine| engine.compile_resident(0),
    );
    failed(&mut engine, &[a, b], HostError::InvalidArtifact, |engine| {
        engine.guard_resident(KEY ^ 1, b)
    });
    failed(
        &mut engine,
        &[a, b],
        HostError::Resident(RegistryError::InvalidUnit),
        |engine| engine.guard_resident(KEY, 0),
    );
    upload(&mut engine, A, &A_CODE);
    failed(
        &mut engine,
        &[a, b],
        HostError::Resident(RegistryError::CodeInvalidated),
        |engine| engine.guard_resident(KEY, a),
    );
    failed(
        &mut engine,
        &[a, b],
        HostError::Resident(RegistryError::CodeInvalidated),
        |engine| engine.lookup_resident(A),
    );
    preserved(&mut engine, &[a, b], |engine| {
        assert_eq!(engine.lookup_resident(B).map(|id| id.get()), Ok(b));
    });
    let callback = engine
        .begin_callback(KEY, 1, outer.token, ENTRY, RETURN, 18, &[])
        .unwrap();
    assert_eq!(engine.guard(KEY, 1), Ok(()));
    preserved(&mut engine, &[a, b], |engine| {
        assert_eq!(engine.lookup_resident(B + 1).map(|id| id.get()), Ok(b));
    });
    failed(
        &mut engine,
        &[a, b],
        HostError::Resident(RegistryError::CodeInvalidated),
        |engine| engine.lookup_resident(A + 1),
    );
    failed(
        &mut engine,
        &[a, b],
        HostError::Resident(RegistryError::NotFound {
            pc: GuestAddress(u32::MAX),
        }),
        |engine| engine.lookup_resident(u32::MAX),
    );
    failed(
        &mut engine,
        &[a, b],
        HostError::Call(CallError::Busy),
        |engine| engine.guard_resident(KEY, b),
    );
    failed(
        &mut engine,
        &[a, b],
        HostError::Call(CallError::Busy),
        |engine| engine.compile_resident(0),
    );
    failed(
        &mut engine,
        &[a, b],
        HostError::Resident(RegistryError::CodeInvalidated),
        |engine| engine.guard_resident(KEY, a),
    );
    engine.abort_callback(KEY, callback.token).unwrap();
    engine.abandon_call(KEY, outer.token).unwrap();
    assert_eq!(engine.guard(KEY, 1), Ok(()));
    assert_eq!(engine.guard_resident(KEY, b), Ok(()));
}
