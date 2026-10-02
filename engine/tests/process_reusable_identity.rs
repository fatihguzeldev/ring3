use ring3_engine::{
    abi::x86::{encode_exit_v3, encode_state},
    cpu::{
        ExecutionExit, ExitReason, UnsupportedFeature,
        dbt::{
            ArtifactError, BlockSpec, CompileError, CompileLimits, InstructionError, compile_region,
        },
        x86::{State32, decode::DecodeError},
    },
    memory::GuestAddress,
    process::{EngineInstance, HostError},
};

const KEY: u64 = 0x1234_5678_9abc_def0;
const A: u32 = 0x1000;
const B: u32 = 0x2000;
const BAD: u32 = 0x3000;
const A_CODE: [u8; 3] = [0x40, 0xeb, 0];
const B_CODE: [u8; 3] = [0x48, 0xeb, 0];

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn fixture() -> EngineInstance {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    for pc in [A, B, BAD] {
        engine.map(pc, 1, 7).unwrap();
    }
    upload(&mut engine, A, &A_CODE);
    upload(&mut engine, B, &B_CODE);
    upload(&mut engine, BAD, &[0x0f, 0x06]);
    engine
}

fn describe(engine: &mut EngineInstance, pc: u32, length: u32) {
    let transfer = &mut engine.arena_mut().unwrap()[140..];
    transfer.fill(0xa5);
    transfer[..4].copy_from_slice(&pc.to_le_bytes());
    transfer[4..8].copy_from_slice(&length.to_le_bytes());
}

fn compile(engine: &mut EngineInstance, entries: bool) -> Result<u32, HostError> {
    if entries {
        engine.compile_entries(1, 0)
    } else {
        engine.compile(1)
    }
}

fn spec(pc: u32) -> BlockSpec {
    BlockSpec {
        entry: GuestAddress(pc),
        byte_length: 3,
    }
}

#[test]
fn legacy_replacement_rejects_old_generation_and_failed_compile_preserves_current_unit() {
    for entries in [false, true] {
        let mut engine = fixture();
        describe(&mut engine, A, 3);
        assert_eq!(compile(&mut engine, entries), Ok(1));
        assert_eq!(engine.guard(KEY, 1), Ok(()));
        let a_bytes = engine.artifact_bytes().unwrap().to_vec();
        let a_snapshot = engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(A), 3)
            .unwrap();

        describe(&mut engine, B, 3);
        assert_eq!(compile(&mut engine, entries), Ok(2));
        let b_bytes = engine.artifact_bytes().unwrap().to_vec();
        assert_ne!(a_bytes, b_bytes);
        assert!(engine.memory().unwrap().is_code_current(&a_snapshot));
        assert_eq!(engine.guard(KEY, 1), Err(HostError::InvalidArtifact));
        assert_eq!(engine.guard(KEY, 2), Ok(()));
        assert_eq!(engine.guard(KEY ^ 1, 2), Err(HostError::InvalidArtifact));

        let state = State32 {
            registers: [0x89ab_cdef, 1, 2, 3, 0x8765_4321, 5, 6, 7],
            eip: B + 1,
            eflags: 0xcd7,
        };
        encode_state(&state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
        encode_exit_v3(
            &ExecutionExit {
                retired: 17,
                reason: ExitReason::NeedCode,
            },
            &mut engine.arena_mut().unwrap()[56..96],
        )
        .unwrap();
        engine.arena_mut().unwrap()[100..140].fill(0x5a);
        describe(&mut engine, BAD, 2);
        let arena = engine.arena().to_vec();
        let arena_address = engine.arena_address();
        assert_eq!(
            compile(&mut engine, entries),
            Err(HostError::Compile(CompileError::Instruction {
                pc: GuestAddress(BAD),
                cause: InstructionError::Decode(DecodeError::Unsupported(
                    UnsupportedFeature::Privileged,
                )),
            }))
        );
        assert_eq!(engine.arena(), arena);
        assert_eq!(engine.arena_address(), arena_address);
        assert_eq!(engine.generation(), 2);
        assert_eq!(engine.artifact_bytes().unwrap(), b_bytes);
        assert_eq!(engine.guard(KEY, 1), Err(HostError::InvalidArtifact));
        assert_eq!(engine.guard(KEY, 2), Ok(()));

        engine.close();
        assert!(!engine.is_open());
        assert_eq!(engine.generation(), 0);
        assert_eq!(engine.guard(KEY, 2), Err(HostError::Closed));
        assert_eq!(engine.guard(KEY ^ 1, 2), Err(HostError::Closed));
        assert_eq!(engine.artifact_bytes(), Err(HostError::Closed));
        assert_eq!(compile(&mut engine, entries), Err(HostError::Closed));
        assert_eq!(engine.arena(), arena);
    }
}

#[test]
fn retained_regions_track_own_memory_identity_and_inactive_page_versions() {
    for mutation in 0..3 {
        let mut engine = fixture();
        let retained_a = compile_region(
            engine.memory().unwrap(),
            &[spec(A)],
            CompileLimits::default(),
        )
        .unwrap();
        let retained_b = compile_region(
            engine.memory().unwrap(),
            &[spec(B)],
            CompileLimits::default(),
        )
        .unwrap();
        let a_snapshot = engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(A), 3)
            .unwrap();
        let b_snapshot = engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(B), 3)
            .unwrap();
        let a_bytes = retained_a
            .wasm_bytes(engine.memory().unwrap())
            .unwrap()
            .to_vec();
        let b_bytes = retained_b
            .wasm_bytes(engine.memory().unwrap())
            .unwrap()
            .to_vec();
        describe(&mut engine, B, 3);
        assert_eq!(engine.compile(1), Ok(1));
        let active_b_bytes = engine.artifact_bytes().unwrap().to_vec();

        let other = fixture();
        assert!(!other.memory().unwrap().is_code_current(&a_snapshot));
        assert_eq!(
            retained_a.wasm_bytes(other.memory().unwrap()),
            Err(ArtifactError::CodeInvalidated)
        );
        assert_eq!(
            retained_a.wasm_bytes(engine.memory().unwrap()).unwrap(),
            a_bytes
        );

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
        let mut source = [0; 3];
        engine
            .memory()
            .unwrap()
            .fetch(GuestAddress(A), &mut source)
            .unwrap();
        assert_eq!(source, A_CODE);
        assert!(!engine.memory().unwrap().is_code_current(&a_snapshot));
        assert_eq!(
            retained_a.wasm_bytes(engine.memory().unwrap()),
            Err(ArtifactError::CodeInvalidated)
        );
        assert!(engine.memory().unwrap().is_code_current(&b_snapshot));
        assert_eq!(
            retained_b.wasm_bytes(engine.memory().unwrap()).unwrap(),
            b_bytes
        );
        assert_eq!(engine.artifact_bytes().unwrap(), active_b_bytes);
        assert_eq!(engine.generation(), 1);
        assert_eq!(engine.guard(KEY, 1), Ok(()));
    }
}
