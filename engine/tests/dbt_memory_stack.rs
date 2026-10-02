use ring3_engine::cpu::UnsupportedFeature;
use ring3_engine::cpu::dbt::{
    BlockSpec, CompileError, CompileLimits, InstructionError, compile_region, prepare_region,
};
use ring3_engine::cpu::x86::decode::DecodeError;
use ring3_engine::memory::{Access, AddressSpace, GuestAddress, PageRange, Permissions};
use ring3_engine::process::{EngineInstance, HostError};

const KEY: u64 = 0xfedc_1234_5678_abcd;
const MEMORY_FORMS: &[(&str, &[u8])] = &[
    ("push base", &[0xff, 0x33]),
    ("pop base", &[0x8f, 0x03]),
    ("push esp", &[0xff, 0x34, 0x24]),
    ("pop esp", &[0x8f, 0x04, 0x24]),
    ("push esp minus four", &[0xff, 0x74, 0x24, 0xfc]),
    ("pop esp minus four", &[0x8f, 0x44, 0x24, 0xfc]),
    ("push ebp zero displacement", &[0xff, 0x75, 0]),
    ("pop ebp zero displacement", &[0x8f, 0x45, 0]),
    ("push base index displacement", &[0xff, 0x74, 0x8b, 0x10]),
    ("pop base index displacement", &[0x8f, 0x44, 0x8b, 0x10]),
    ("push no base index", &[0xff, 0x34, 0xb5, 0x10, 0x40, 0, 0]),
    ("pop no base index", &[0x8f, 0x04, 0xb5, 0x10, 0x40, 0, 0]),
    ("push absolute", &[0xff, 0x35, 0, 0x80, 0, 0]),
    ("pop absolute", &[0x8f, 0x05, 0, 0x80, 0, 0]),
    (
        "push esp full displacement",
        &[0xff, 0xb4, 0x24, 0xfc, 0xff, 0xff, 0xff],
    ),
    (
        "pop esp full displacement",
        &[0x8f, 0x84, 0x24, 0xfc, 0xff, 0xff, 0xff],
    ),
    ("push esp scaled index", &[0xff, 0xb4, 0xf4, 0, 0, 0, 0x80]),
    ("pop esp scaled index", &[0x8f, 0x84, 0xf4, 0, 0, 0, 0x80]),
];

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn with_code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    upload(&mut engine, 0x1000, bytes);
    engine
}

fn describe(engine: &mut EngineInstance, entry: u32, length: u32) {
    let descriptor = &mut engine.arena_mut().unwrap()[140..148];
    descriptor[..4].copy_from_slice(&entry.to_le_bytes());
    descriptor[4..].copy_from_slice(&length.to_le_bytes());
}

fn compile(engine: &mut EngineInstance, entry: u32, length: u32) -> Result<u32, HostError> {
    describe(engine, entry, length);
    engine.compile(1)
}

fn instruction_error(pc: u32, cause: InstructionError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    }
}

fn code_bytes(engine: &EngineInstance, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    bytes
}

#[test]
fn memory_push_and_pop_are_sequential_inside_one_block() {
    let bytes = [0x90, 0xff, 0x34, 0x24, 0x8f, 0x04, 0x24, 0x90];
    let mut engine = with_code(&bytes);
    assert_eq!(compile(&mut engine, 0x1000, bytes.len() as u32), Ok(1));
    engine.guard(KEY, 1).unwrap();
}

#[test]
fn ea_forms_compile_without_accessing_unmapped_operand_or_stack_ram() {
    for (name, instruction) in MEMORY_FORMS {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        bytes.push(0x90);
        let mut engine = with_code(&bytes);
        let snapshot = engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(0x1000), bytes.len())
            .unwrap();
        describe(&mut engine, 0x1000, bytes.len() as u32);
        let before = engine.arena().to_vec();
        assert_eq!(engine.compile(1), Ok(1), "{name}");
        assert_eq!(engine.arena(), before, "{name}");
        assert_eq!(code_bytes(&engine, 0x1000, bytes.len()), bytes);
        assert!(engine.memory().unwrap().is_code_current(&snapshot));
        engine.guard(KEY, 1).unwrap();
        for address in [0, 0x4010, 0x8000] {
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(address), Access::Read)
                    .is_err()
            );
        }
    }
}

#[test]
fn standalone_memory_stack_exclusion_and_original_pc_priority_remain_unchanged() {
    for (name, instruction) in MEMORY_FORMS {
        for trailing in [false, true] {
            let mut bytes = vec![0x90];
            bytes.extend_from_slice(instruction);
            if trailing {
                bytes.push(0x90);
            }
            let mut memory = AddressSpace::new(1).unwrap();
            memory
                .map_zeroed(
                    PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                    Permissions::ALL,
                )
                .unwrap();
            memory.write(GuestAddress(0x1000), &bytes).unwrap();
            let snapshot = memory
                .snapshot_code(GuestAddress(0x1000), bytes.len())
                .unwrap();
            let specs = [BlockSpec {
                entry: GuestAddress(0x1000),
                byte_length: bytes.len() as u32,
            }];
            let expected = instruction_error(0x1001, InstructionError::BackendUnsupported);
            assert_eq!(
                prepare_region(&memory, &specs, CompileLimits::default()).err(),
                Some(expected),
                "{name}"
            );
            assert_eq!(
                compile_region(&memory, &specs, CompileLimits::default()).err(),
                Some(expected),
                "{name}"
            );
            assert!(memory.is_code_current(&snapshot));
            let mut observed = vec![0; bytes.len()];
            memory.read(GuestAddress(0x1000), &mut observed).unwrap();
            assert_eq!(observed, bytes);
        }
    }
}

#[test]
fn unsupported_indirect_control_reports_exact_pc_after_a_memory_stack_prefix() {
    for instruction in [
        &[0xff, 0xd0][..],
        &[0xff, 0x13][..],
        &[0xff, 0xe0][..],
        &[0xff, 0x23][..],
    ] {
        let mut bytes = vec![0x90, 0xff, 0x34, 0x24, 0x8f, 0x04, 0x24];
        bytes.extend_from_slice(instruction);
        let mut engine = with_code(&bytes);
        assert_eq!(
            compile(&mut engine, 0x1000, bytes.len() as u32),
            Err(HostError::Compile(instruction_error(
                0x1007,
                InstructionError::BackendUnsupported
            )))
        );
        assert_eq!(engine.generation(), 0);
        assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
    }
}

#[test]
fn prefixes_and_far_transfers_keep_decoder_errors_after_a_memory_stack_prefix() {
    for (instruction, feature) in [
        (&[0x66, 0xff, 0x33][..], UnsupportedFeature::Opcode),
        (&[0x66, 0x8f, 0x03][..], UnsupportedFeature::Opcode),
        (&[0x67, 0xff, 0x33][..], UnsupportedFeature::Opcode),
        (&[0x67, 0x8f, 0x03][..], UnsupportedFeature::Opcode),
        (&[0x64, 0xff, 0x33][..], UnsupportedFeature::Segment),
        (&[0x64, 0x8f, 0x03][..], UnsupportedFeature::Segment),
        (&[0xff, 0x1b][..], UnsupportedFeature::Opcode),
        (&[0xff, 0x2b][..], UnsupportedFeature::Opcode),
        (&[0xcb][..], UnsupportedFeature::Opcode),
    ] {
        let mut bytes = vec![0x90, 0xff, 0x34, 0x24, 0x8f, 0x04, 0x24];
        bytes.extend_from_slice(instruction);
        let mut engine = with_code(&bytes);
        assert_eq!(
            compile(&mut engine, 0x1000, bytes.len() as u32),
            Err(HostError::Compile(instruction_error(
                0x1007,
                InstructionError::Decode(DecodeError::Unsupported(feature))
            )))
        );
        assert_eq!(engine.generation(), 0);
    }
}

#[test]
fn declared_spans_cannot_cut_memory_stack_ea_encodings() {
    for (name, instruction) in MEMORY_FORMS {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        let mut engine = with_code(&bytes);
        assert_eq!(
            compile(&mut engine, 0x1000, bytes.len() as u32 - 1),
            Err(HostError::Compile(instruction_error(
                0x1001,
                InstructionError::InvalidBlockEnd
            ))),
            "{name}"
        );
        assert_eq!(engine.generation(), 0);
        assert_eq!(code_bytes(&engine, 0x1000, bytes.len()), bytes);
    }
}

#[test]
fn full_ea_spans_stamp_both_consumed_pages_without_stamping_data_pages() {
    for instruction in [
        &[0xff, 0xb4, 0xf4, 0, 0, 0, 0x80][..],
        &[0x8f, 0x84, 0xf4, 0, 0, 0, 0x80][..],
    ] {
        for page in [0x1000, 0x2000] {
            let mut engine = EngineInstance::new(3, KEY).unwrap();
            engine.map(0x1000, 2, 7).unwrap();
            engine.map(0x4000, 1, 3).unwrap();
            upload(&mut engine, 0x1ffd, instruction);
            assert_eq!(
                compile(&mut engine, 0x1ffd, instruction.len() as u32),
                Ok(1)
            );
            engine.write32(0x4000, 0x1122_3344).unwrap();
            engine.protect(0x4000, 1, 1).unwrap();
            engine.guard(KEY, 1).unwrap();
            engine.protect(page, 1, 7).unwrap();
            assert_eq!(engine.guard(KEY, 1), Err(HostError::CodeInvalidated));
            assert_eq!(engine.artifact_bytes(), Err(HostError::CodeInvalidated));
            assert_eq!(engine.generation(), 1);
        }
    }
}

#[test]
fn failed_compile_preserves_an_installed_memory_stack_artifact_and_retry_replaces_it() {
    let mut engine = with_code(&[0xff, 0x34, 0x24, 0x8f, 0x04, 0x24]);
    engine.map(0x3000, 1, 7).unwrap();
    assert_eq!(compile(&mut engine, 0x1000, 6), Ok(1));
    let installed = engine.artifact_bytes().unwrap().to_vec();
    let bad = [0xff, 0x34, 0x24, 0xff, 0xd0];
    upload(&mut engine, 0x3000, &bad);
    describe(&mut engine, 0x3000, bad.len() as u32);
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.compile(1),
        Err(HostError::Compile(instruction_error(
            0x3003,
            InstructionError::BackendUnsupported
        )))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.generation(), 1);
    assert_eq!(engine.artifact_bytes().unwrap(), installed);
    assert_eq!(code_bytes(&engine, 0x3000, bad.len()), bad);
    engine.guard(KEY, 1).unwrap();
    upload(&mut engine, 0x3000, &[0x8f, 0x04, 0x24]);
    assert_eq!(compile(&mut engine, 0x3000, 3), Ok(2));
    assert_eq!(engine.guard(KEY, 1), Err(HostError::InvalidArtifact));
    engine.guard(KEY, 2).unwrap();
}
