use ring3_engine::cpu::UnsupportedFeature;
use ring3_engine::cpu::dbt::{
    BlockSpec, CompileError, CompileLimits, InstructionError, compile_region, prepare_region,
};
use ring3_engine::cpu::x86::decode::DecodeError;
use ring3_engine::memory::{
    Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
};
use ring3_engine::process::{EngineInstance, HostError};

const KEY: u64 = 0x1234_5678_9abc_def0;
const MEMORY_MOVES: &[(&str, &[u8])] = &[
    ("register from base", &[0x8b, 0x03]),
    ("base displacement from register", &[0x89, 0x4b, 0x10]),
    (
        "negative displacement immediate",
        &[0xc7, 0x43, 0xfc, 0x78, 0x56, 0x34, 0x12],
    ),
    ("scaled base index load", &[0x8b, 0x54, 0x8d, 0xe0]),
    (
        "scaled esp base store",
        &[0x89, 0x94, 0x44, 0x78, 0x56, 0x34, 0x12],
    ),
    (
        "index without base",
        &[0x8b, 0x04, 0xcd, 0x78, 0x56, 0x34, 0x12],
    ),
    ("esp displacement load", &[0x8b, 0x44, 0x24, 0x7f]),
    ("ebp zero displacement", &[0x8b, 0x45, 0]),
    ("esp destination and base", &[0x8b, 0x24, 0x24]),
    ("esp source", &[0x89, 0x65, 0]),
    ("moffs load", &[0xa1, 0x78, 0x56, 0x34, 0x12]),
    ("moffs store", &[0xa3, 0x78, 0x56, 0x34, 0x12]),
    ("absolute modrm load", &[0x8b, 0x05, 0x78, 0x56, 0x34, 0x12]),
    (
        "absolute immediate store",
        &[0xc7, 0x05, 0x78, 0x56, 0x34, 0x12, 0xef, 0xbe, 0xad, 0xde],
    ),
];

fn range(address: u32, pages: u32) -> PageRange {
    PageRange::new(GuestAddress(address), pages).unwrap()
}

fn spec(entry: u32, byte_length: u32) -> BlockSpec {
    BlockSpec {
        entry: GuestAddress(entry),
        byte_length,
    }
}

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn compile(engine: &mut EngineInstance, entry: u32, length: u32) -> Result<u32, HostError> {
    let transfer = &mut engine.arena_mut().unwrap()[140..148];
    transfer[..4].copy_from_slice(&entry.to_le_bytes());
    transfer[4..].copy_from_slice(&length.to_le_bytes());
    engine.compile(1)
}

fn backend_error(pc: u32) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause: InstructionError::BackendUnsupported,
    }
}

#[test]
fn standalone_preparation_and_compilation_keep_memory_moves_outside_their_profile() {
    for (name, instruction) in MEMORY_MOVES {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(range(0x1000, 1), Permissions::ALL)
            .unwrap();
        memory.write(GuestAddress(0x1000), &bytes).unwrap();
        let snapshot = memory
            .snapshot_code(GuestAddress(0x1000), bytes.len())
            .unwrap();
        let blocks = [spec(0x1000, bytes.len() as u32)];
        assert_eq!(
            prepare_region(&memory, &blocks, CompileLimits::default()).err(),
            Some(backend_error(0x1001)),
            "{name}"
        );
        assert_eq!(
            compile_region(&memory, &blocks, CompileLimits::default()).err(),
            Some(backend_error(0x1001)),
            "{name}"
        );
        assert!(memory.is_code_current(&snapshot));
        let mut output = vec![0; bytes.len()];
        memory.read(GuestAddress(0x1000), &mut output).unwrap();
        assert_eq!(output, bytes);
    }
}

#[test]
fn embedded_compilation_accepts_authored_ea_forms_without_reading_guest_operands() {
    for (name, bytes) in MEMORY_MOVES {
        let mut engine = EngineInstance::new(1, KEY).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        upload(&mut engine, 0x1000, bytes);
        let snapshot = engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(0x1000), bytes.len())
            .unwrap();
        let descriptor = &mut engine.arena_mut().unwrap()[140..148];
        descriptor[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
        descriptor[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        let before = engine.arena().to_vec();
        assert_eq!(engine.compile(1), Ok(1), "{name}");
        assert_eq!(engine.arena(), before);
        assert!(engine.memory().unwrap().is_code_current(&snapshot));
        assert_eq!(
            &engine.artifact_bytes().unwrap()[..4],
            &[0, 0x61, 0x73, 0x6d]
        );
        engine.guard(KEY, 1).unwrap();
        assert!(
            engine
                .memory()
                .unwrap()
                .resolve(GuestAddress(0x1234_5678), Access::Read)
                .is_err()
        );
        let mut output = vec![0; bytes.len()];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x1000), &mut output)
            .unwrap();
        assert_eq!(output, *bytes);
    }
}

#[test]
fn embedded_compilation_accepts_execute_only_instruction_pages() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    upload(&mut engine, 0x1000, &[0xa1, 0, 0x40, 0, 0]);
    engine.protect(0x1000, 1, 4).unwrap();
    assert_eq!(compile(&mut engine, 0x1000, 5), Ok(1));
    engine.guard(KEY, 1).unwrap();
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0x1000), Access::Read)
            .is_err()
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0x4000), Access::Read)
            .is_err()
    );
}

#[test]
fn decoder_exclusions_after_a_memory_prefix_remain_rejected_at_their_pc() {
    for (instruction, feature) in [
        (&[0xc0, 0x20, 1][..], UnsupportedFeature::Opcode),
        (&[0x0f, 0x06][..], UnsupportedFeature::Privileged),
    ] {
        let mut engine = EngineInstance::new(1, KEY).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        let mut bytes = vec![0x90, 0x8b, 0x03];
        bytes.extend_from_slice(instruction);
        upload(&mut engine, 0x1000, &bytes);
        assert_eq!(
            compile(&mut engine, 0x1000, bytes.len() as u32),
            Err(HostError::Compile(CompileError::Instruction {
                pc: GuestAddress(0x1003),
                cause: InstructionError::Decode(DecodeError::Unsupported(feature)),
            }))
        );
        assert_eq!(engine.generation(), 0);
        assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
    }
}

#[test]
fn failed_embedded_compile_preserves_current_artifact_and_successful_retry_replaces_it() {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x3000, 1, 7).unwrap();
    upload(&mut engine, 0x1000, &[0x90]);
    upload(&mut engine, 0x3000, &[0x8b, 0x03, 0x0f, 0x06]);
    assert_eq!(compile(&mut engine, 0x1000, 1), Ok(1));
    let original = engine.artifact_bytes().unwrap().to_vec();
    let descriptor = &mut engine.arena_mut().unwrap()[140..148];
    descriptor.copy_from_slice(&[0, 0x30, 0, 0, 4, 0, 0, 0]);
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.compile(1),
        Err(HostError::Compile(CompileError::Instruction {
            pc: GuestAddress(0x3002),
            cause: InstructionError::Decode(DecodeError::Unsupported(
                UnsupportedFeature::Privileged
            )),
        }))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.generation(), 1);
    assert_eq!(engine.artifact_bytes().unwrap(), original);
    engine.guard(KEY, 1).unwrap();
    upload(&mut engine, 0x3000, &[0x8b, 0x03, 0x89, 0x03]);
    engine.guard(KEY, 1).unwrap();
    assert_eq!(compile(&mut engine, 0x3000, 4), Ok(2));
    assert_eq!(engine.guard(KEY, 1), Err(HostError::InvalidArtifact));
    engine.guard(KEY, 2).unwrap();
}

#[test]
fn memory_move_declared_end_must_still_be_an_instruction_boundary() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    upload(&mut engine, 0x1000, &[0x90, 0xa1, 0x78, 0x56, 0x34, 0x12]);
    assert_eq!(
        compile(&mut engine, 0x1000, 5),
        Err(HostError::Compile(CompileError::Instruction {
            pc: GuestAddress(0x1001),
            cause: InstructionError::InvalidBlockEnd,
        }))
    );
    assert_eq!(engine.generation(), 0);
}

#[test]
fn truncated_fetch_retains_original_instruction_pc_and_attempted_span() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    upload(&mut engine, 0x1fff, &[0x8b]);
    assert_eq!(
        compile(&mut engine, 0x1fff, 2),
        Err(HostError::Compile(CompileError::Instruction {
            pc: GuestAddress(0x1fff),
            cause: InstructionError::Decode(DecodeError::MemoryFault {
                pc: GuestAddress(0x1fff),
                fault: MemoryFault {
                    address: GuestAddress(0x2000),
                    access: Access::Execute,
                    reason: FaultReason::Unmapped
                },
                length: 2,
            }),
        }))
    );
    assert_eq!(engine.generation(), 0);
}

#[test]
fn both_pages_of_a_cross_page_memory_instruction_contribute_code_stamps() {
    for address in [0x1000, 0x2000] {
        let mut engine = EngineInstance::new(2, KEY).unwrap();
        engine.map(0x1000, 2, 7).unwrap();
        upload(&mut engine, 0x1ffe, &[0x8b, 0x43, 4]);
        assert_eq!(compile(&mut engine, 0x1ffe, 3), Ok(1));
        engine.guard(KEY, 1).unwrap();
        engine.protect(address, 1, 7).unwrap();
        assert_eq!(engine.guard(KEY, 1), Err(HostError::CodeInvalidated));
        assert_eq!(engine.artifact_bytes(), Err(HostError::CodeInvalidated));
        assert_eq!(engine.generation(), 1);
    }
}
