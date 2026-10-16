use ring3_engine::{
    abi::{
        arena::{EXIT_OFFSET, SEGMENTS_OFFSET, TRANSFER_OFFSET},
        x86::{SEGMENTS_SIZE, encode_exit, encode_segment_context, encode_state},
    },
    cpu::{
        ExecutionExit, ExitReason,
        dbt::{CompileError, InstructionError, RegistryError},
        x86::{SegmentCache, State32, decode::DecodeError},
    },
    memory::GuestAddress,
    process::{EngineInstance, HostError},
};

const KEY: u64 = 0x4353_4645_5443_4801;

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn cs_limit(engine: &mut EngineInstance, limit: u32) {
    let mut context = engine.segment_context().unwrap();
    context.caches[1] = SegmentCache::from_bits(0xf2b0_0000 | limit).unwrap();
    let mut record = [0; SEGMENTS_SIZE];
    encode_segment_context(&context, &mut record).unwrap();
    engine.arena_mut().unwrap()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE]
        .copy_from_slice(&record);
}

fn descriptor(engine: &mut EngineInstance, entry: u32, length: u32, entries: bool) {
    let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    transfer[..4].copy_from_slice(&entry.to_le_bytes());
    transfer[4..].copy_from_slice(&length.to_le_bytes());
    if entries {
        transfer[4..].fill(0);
    }
}

fn stop(engine: &mut EngineInstance, entry: u32) {
    let arena = engine.arena_mut().unwrap();
    encode_state(
        &State32 {
            registers: [0x1122_3344; 8],
            eip: entry,
            eflags: 2,
        },
        &mut arena[..56],
    )
    .unwrap();
    encode_exit(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::NeedCode,
        },
        &mut arena[EXIT_OFFSET..EXIT_OFFSET + 40],
    )
    .unwrap();
}

fn strict(engine: &mut EngineInstance, resident: bool, entries: bool) -> Result<(), HostError> {
    match (resident, entries) {
        (false, false) => engine.compile(1).map(|_| ()),
        (false, true) => engine.compile_entries(1, 0).map(|_| ()),
        (true, false) => engine.compile_resident(1).map(|_| ()),
        (true, true) => engine.compile_resident_entries(1, 0).map(|_| ()),
    }
}

fn strict_cause(error: HostError) -> CompileError {
    match error {
        HostError::Compile(error) | HostError::Resident(RegistryError::Compile(error)) => error,
        other => panic!("expected typed compile refusal, received {other:?}"),
    }
}

fn prior(resident: bool, entry: u32, prefix: bool, nonexec: bool) -> EngineInstance {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    if nonexec {
        engine.map(0x2000, 1, 3).unwrap();
    }
    let mut branch = vec![0xe9];
    branch.extend_from_slice(&entry.wrapping_sub(0x1005).to_le_bytes());
    upload(&mut engine, 0x1000, &branch);
    if prefix {
        upload(&mut engine, entry, &[0x0f]);
    }
    descriptor(&mut engine, 0x1000, 5, false);
    strict(&mut engine, resident, false).unwrap();
    cs_limit(&mut engine, 0x1fff);
    stop(&mut engine, entry);
    engine
}

fn capture(engine: &mut EngineInstance, resident: bool, generation: u32) -> u64 {
    if resident {
        engine.compile_stopped_resident_entry(KEY).unwrap().get()
    } else {
        u64::from(engine.compile_stopped_entry(KEY, generation).unwrap())
    }
}

fn guard(engine: &EngineInstance, resident: bool, token: u64) -> Result<(), HostError> {
    if resident {
        engine.guard_resident(KEY, token)
    } else {
        engine.guard(KEY, token as u32)
    }
}

#[test]
fn strict_bound_profiles_refuse_mapped_cs_denial_without_publication() {
    for (resident, entries) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut engine = EngineInstance::new(1, KEY).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        upload(&mut engine, 0x1000, &[0x90, 0xeb, 0]);
        cs_limit(&mut engine, 0xfff);
        descriptor(&mut engine, 0x1000, 3, entries);
        let before = engine.arena().to_vec();
        assert_eq!(
            strict_cause(strict(&mut engine, resident, entries).unwrap_err()),
            CompileError::Instruction {
                pc: GuestAddress(0x1000),
                cause: InstructionError::Decode(DecodeError::CodeSegment {
                    pc: GuestAddress(0x1000),
                    attempted_length: 1
                }),
            }
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.generation(), 0);
    }
}

#[test]
fn incremental_cs_denial_precedes_missing_or_nonexec_byte_but_exact_end_nop_succeeds() {
    for nonexec in [false, true] {
        let mut engine = EngineInstance::new(2, KEY).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        if nonexec {
            engine.map(0x2000, 1, 3).unwrap();
        }
        upload(&mut engine, 0x1fff, &[0x0f]);
        cs_limit(&mut engine, 0x1fff);
        descriptor(&mut engine, 0x1fff, 2, false);
        let before = engine.arena().to_vec();
        assert_eq!(
            strict_cause(engine.compile(1).unwrap_err()),
            CompileError::Instruction {
                pc: GuestAddress(0x1fff),
                cause: InstructionError::Decode(DecodeError::CodeSegment {
                    pc: GuestAddress(0x1fff),
                    attempted_length: 2
                }),
            }
        );
        assert_eq!(engine.arena(), before);
        upload(&mut engine, 0x1fff, &[0x90]);
        descriptor(&mut engine, 0x1fff, 1, false);
        engine.compile(1).unwrap();
        engine.guard(KEY, 1).unwrap();
    }
}

#[test]
fn source_free_runtime_fault_depends_on_identity_and_preserves_arena_for_both_owners() {
    for resident in [false, true] {
        let mut engine = prior(resident, 0x2000, false, false);
        let before = engine.arena().to_vec();
        let token = capture(&mut engine, resident, 1);
        assert_eq!(engine.arena(), before);
        guard(&engine, resident, token).unwrap();
        engine.map(0x2000, 1, 3).unwrap();
        guard(&engine, resident, token).unwrap();
        if resident {
            assert_eq!(engine.lookup_resident(0x2000).unwrap().get(), token);
        }
    }
}

#[test]
fn real_consumed_prefix_invalidates_runtime_fault_and_stale_preparation_is_neutral() {
    for (resident, nonexec) in [(false, false), (true, true)] {
        let mut engine = prior(resident, 0x1fff, true, nonexec);
        let token = capture(&mut engine, resident, 1);
        guard(&engine, resident, token).unwrap();
        upload(&mut engine, 0x1fff, &[0x0f]);
        assert!(matches!(
            guard(&engine, resident, token),
            Err(HostError::CodeInvalidated | HostError::Resident(RegistryError::CodeInvalidated))
        ));
        let before = engine.arena().to_vec();
        let result = if resident {
            engine.compile_stopped_resident_entry(KEY).map(|_| ())
        } else {
            engine.compile_stopped_entry(KEY, token as u32).map(|_| ())
        };
        assert!(result.is_err());
        assert_eq!(engine.arena(), before);
    }
}

#[test]
fn repaired_synthetic_pc_can_compile_material_code_and_resident_routing_selects_it() {
    for resident in [false, true] {
        let mut engine = prior(resident, 0x2000, false, false);
        let old = capture(&mut engine, resident, 1);
        engine.map(0x2000, 1, 7).unwrap();
        upload(&mut engine, 0x2000, &[0x90, 0xeb, 0]);
        cs_limit(&mut engine, 0x2fff);
        if resident {
            assert!(matches!(
                engine.lookup_resident(0x2000),
                Err(HostError::Resident(RegistryError::NotFound { .. }))
            ));
        }
        let next = capture(&mut engine, resident, old as u32);
        assert_ne!(next, old);
        guard(&engine, resident, next).unwrap();
        if resident {
            assert_eq!(engine.lookup_resident(0x2000).unwrap().get(), next);
            guard(&engine, resident, old).unwrap();
        } else {
            crate::support_fetch::assert_host_contract(
                (guard(&engine, resident, old)).expect_err("compilation must refuse this fixture"),
                HostError::InvalidArtifact,
            );
        }
    }
}
