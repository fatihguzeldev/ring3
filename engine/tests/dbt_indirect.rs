use ring3_engine::cpu::UnsupportedFeature;
use ring3_engine::cpu::dbt::{
    BlockSpec, CompileError, CompileLimits, InstructionError, compile_region, prepare_region,
};
use ring3_engine::cpu::x86::decode::DecodeError;
use ring3_engine::memory::{Access, AddressSpace, GuestAddress, PageRange, Permissions};
use ring3_engine::process::{EngineInstance, HostError};

const KEY: u64 = 0xfabc_9876_1234_5678;
const MEMORY_FORMS: &[(&str, bool, &[u8])] = &[
    ("call base", true, &[0xff, 0x13]),
    ("jump base", false, &[0xff, 0x23]),
    ("call esp", true, &[0xff, 0x14, 0x24]),
    ("jump esp", false, &[0xff, 0x24, 0x24]),
    ("call esp minus four", true, &[0xff, 0x54, 0x24, 0xfc]),
    ("jump esp minus four", false, &[0xff, 0x64, 0x24, 0xfc]),
    ("call ebp zero displacement", true, &[0xff, 0x55, 0]),
    ("jump ebp zero displacement", false, &[0xff, 0x65, 0]),
    (
        "call base index displacement",
        true,
        &[0xff, 0x54, 0x8b, 0x10],
    ),
    (
        "jump base index displacement",
        false,
        &[0xff, 0x64, 0x8b, 0x10],
    ),
    (
        "call index without base",
        true,
        &[0xff, 0x14, 0xcd, 0, 0x80, 0, 0],
    ),
    (
        "jump index without base",
        false,
        &[0xff, 0x24, 0xcd, 0, 0x80, 0, 0],
    ),
    ("call absolute", true, &[0xff, 0x15, 0, 0x80, 0, 0]),
    ("jump absolute", false, &[0xff, 0x25, 0, 0x80, 0, 0]),
    (
        "call esp scaled index",
        true,
        &[0xff, 0x94, 0xf4, 0, 0, 0, 0x80],
    ),
    (
        "jump esp scaled index",
        false,
        &[0xff, 0xa4, 0xf4, 0, 0, 0, 0x80],
    ),
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

fn assert_standalone_error(instruction: &[u8], call: bool, trailing: bool) {
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
    let blocks = [BlockSpec {
        entry: GuestAddress(0x1000),
        byte_length: bytes.len() as u32,
    }];
    let cause = if !call && trailing {
        InstructionError::InvalidBlockEnd
    } else {
        InstructionError::BackendUnsupported
    };
    let expected = instruction_error(0x1001, cause);
    assert_eq!(
        prepare_region(&memory, &blocks, CompileLimits::default()).err(),
        Some(expected)
    );
    assert_eq!(
        compile_region(&memory, &blocks, CompileLimits::default()).err(),
        Some(expected)
    );
    assert!(memory.is_code_current(&snapshot));
    let mut observed = vec![0; bytes.len()];
    memory.read(GuestAddress(0x1000), &mut observed).unwrap();
    assert_eq!(observed, bytes);
}

#[test]
fn embedded_indirect_call_after_nop_compiles_at_the_declared_end() {
    let mut engine = with_code(&[0x90, 0xff, 0xd4]);
    assert_eq!(compile(&mut engine, 0x1000, 3), Ok(1));
    engine.guard(KEY, 1).unwrap();
}

#[test]
fn all_eight_register_call_and_jump_targets_compile_without_target_or_stack_access() {
    for register in 0..8 {
        for modrm in [0xd0 + register, 0xe0 + register] {
            let bytes = [0x90, 0xff, modrm];
            let mut engine = with_code(&bytes);
            let snapshot = engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(0x1000), bytes.len())
                .unwrap();
            describe(&mut engine, 0x1000, bytes.len() as u32);
            let before = engine.arena().to_vec();
            assert_eq!(engine.compile(1), Ok(1), "modrm{modrm:02x}");
            assert_eq!(engine.arena(), before);
            assert_eq!(code_bytes(&engine, 0x1000, bytes.len()), bytes);
            assert!(engine.memory().unwrap().is_code_current(&snapshot));
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(0), Access::Execute)
                    .is_err()
            );
            engine.guard(KEY, 1).unwrap();
        }
    }
}

#[test]
fn memory_target_ea_forms_compile_without_reading_operand_target_or_stack_ram() {
    for (name, _, instruction) in MEMORY_FORMS {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        let mut engine = with_code(&bytes);
        let snapshot = engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(0x1000), bytes.len())
            .unwrap();
        describe(&mut engine, 0x1000, bytes.len() as u32);
        let before = engine.arena().to_vec();
        assert_eq!(engine.compile(1), Ok(1), "{name}");
        assert_eq!(engine.arena(), before);
        assert_eq!(code_bytes(&engine, 0x1000, bytes.len()), bytes);
        assert!(engine.memory().unwrap().is_code_current(&snapshot));
        for address in [0, 0x8000] {
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(address), Access::Read)
                    .is_err()
            );
        }
        engine.guard(KEY, 1).unwrap();
    }
}

#[test]
fn embedded_indirect_call_and_jump_are_terminators_at_their_original_pc() {
    for instruction in [
        &[0xff, 0xd0][..],
        &[0xff, 0xd4][..],
        &[0xff, 0xe0][..],
        &[0xff, 0xe4][..],
    ] {
        let bytes = [0x90, instruction[0], instruction[1], 0x90];
        let mut engine = with_code(&bytes);
        assert_eq!(
            compile(&mut engine, 0x1000, 4),
            Err(HostError::Compile(instruction_error(
                0x1001,
                InstructionError::InvalidBlockEnd
            )))
        );
        assert_eq!(engine.generation(), 0);
    }
    for (name, _, instruction) in MEMORY_FORMS {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        bytes.push(0x90);
        let mut engine = with_code(&bytes);
        assert_eq!(
            compile(&mut engine, 0x1000, bytes.len() as u32),
            Err(HostError::Compile(instruction_error(
                0x1001,
                InstructionError::InvalidBlockEnd
            ))),
            "{name}"
        );
        assert_eq!(engine.generation(), 0);
        assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
    }
}

#[test]
fn standalone_retains_call_and_jump_error_priority_asymmetry() {
    for register in 0..8 {
        for trailing in [false, true] {
            assert_standalone_error(&[0xff, 0xd0 + register], true, trailing);
            assert_standalone_error(&[0xff, 0xe0 + register], false, trailing);
        }
    }
    for (_, call, instruction) in MEMORY_FORMS {
        for trailing in [false, true] {
            assert_standalone_error(instruction, *call, trailing);
        }
    }
}

#[test]
fn declared_spans_cannot_cut_indirect_target_encodings() {
    for instruction in [&[0xff, 0xd4][..], &[0xff, 0xe4][..]] {
        let mut engine = with_code(&[0x90, instruction[0], instruction[1]]);
        assert_eq!(
            compile(&mut engine, 0x1000, 2),
            Err(HostError::Compile(instruction_error(
                0x1001,
                InstructionError::InvalidBlockEnd
            )))
        );
    }
    for (name, _, instruction) in MEMORY_FORMS {
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
    }
}

#[test]
fn far_prefixed_and_other_unimplemented_forms_keep_their_precise_errors() {
    for (instruction, cause) in [
        (
            &[0xff, 0x1b][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0xff, 0x2b][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0x66, 0xff, 0xd4][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0x67, 0xff, 0x13][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0x66, 0xff, 0xe4][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0x67, 0xff, 0x23][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0x64, 0xff, 0x13][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Segment)),
        ),
        (
            &[0x64, 0xff, 0x23][..],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Segment)),
        ),
        (&[0x40][..], InstructionError::BackendUnsupported),
        (&[0x31, 0xc0][..], InstructionError::BackendUnsupported),
    ] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        let mut engine = with_code(&bytes);
        assert_eq!(
            compile(&mut engine, 0x1000, bytes.len() as u32),
            Err(HostError::Compile(instruction_error(0x1001, cause)))
        );
        assert_eq!(engine.generation(), 0);
    }
}

#[test]
fn indirect_ea_spans_stamp_both_code_pages_and_ignore_unrelated_data_changes() {
    for instruction in [
        &[0xff, 0x94, 0xf4, 0, 0, 0, 0x80][..],
        &[0xff, 0xa4, 0xf4, 0, 0, 0, 0x80][..],
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
            engine.write32(0x4000, 0x8000).unwrap();
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
fn failed_indirect_compile_preserves_installed_artifact_and_generation_until_retry() {
    let mut engine = with_code(&[0x90, 0xff, 0xd4]);
    engine.map(0x3000, 1, 7).unwrap();
    assert_eq!(compile(&mut engine, 0x1000, 3), Ok(1));
    let installed = engine.artifact_bytes().unwrap().to_vec();
    for (bad, cause) in [
        (
            &[0x90, 0xff, 0x24, 0x24, 0x90][..],
            InstructionError::InvalidBlockEnd,
        ),
        (&[0x90, 0x40][..], InstructionError::BackendUnsupported),
    ] {
        upload(&mut engine, 0x3000, bad);
        describe(&mut engine, 0x3000, bad.len() as u32);
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.compile(1),
            Err(HostError::Compile(instruction_error(0x3001, cause)))
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.generation(), 1);
        assert_eq!(engine.artifact_bytes().unwrap(), installed);
        assert_eq!(code_bytes(&engine, 0x3000, bad.len()), bad);
        engine.guard(KEY, 1).unwrap();
    }
    upload(&mut engine, 0x3000, &[0x90, 0xff, 0x24, 0x24]);
    assert_eq!(compile(&mut engine, 0x3000, 4), Ok(2));
    assert_eq!(engine.guard(KEY, 1), Err(HostError::InvalidArtifact));
    engine.guard(KEY, 2).unwrap();
}
