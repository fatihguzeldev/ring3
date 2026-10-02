use ring3_engine::cpu::UnsupportedFeature;
use ring3_engine::cpu::dbt::{
    BlockSpec, CompileError, CompileLimits, InstructionError, compile_region, prepare_region,
};
use ring3_engine::cpu::x86::decode::DecodeError;
use ring3_engine::memory::{Access, AddressSpace, GuestAddress, PageRange, Permissions};
use ring3_engine::process::{EngineInstance, HostError};

const KEY: u64 = 0x1234_9876_abcd_5678;

fn with_code(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    upload(&mut engine, 0x1000, bytes);
    engine
}

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn compile(engine: &mut EngineInstance, entry: u32, length: u32) -> Result<u32, HostError> {
    describe(engine, entry, length);
    engine.compile(1)
}

fn describe(engine: &mut EngineInstance, entry: u32, length: u32) {
    let descriptor = &mut engine.arena_mut().unwrap()[140..148];
    descriptor[..4].copy_from_slice(&entry.to_le_bytes());
    descriptor[4..].copy_from_slice(&length.to_le_bytes());
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
fn push_pop_values_are_sequential_and_may_appear_inside_one_block() {
    for bytes in [
        &[0x90, 0x54, 0x5c, 0x90][..],
        &[0x90, 0x6a, 0xff, 0x58, 0x90][..],
    ] {
        let mut engine = with_code(bytes);
        assert_eq!(compile(&mut engine, 0x1000, bytes.len() as u32), Ok(1));
        engine.guard(KEY, 1).unwrap();
    }
}

#[test]
fn all_eight_push_and_pop_registers_and_modrm_aliases_compile_without_guest_stack_access() {
    for register in 0..8 {
        for instruction in [
            vec![0x50 + register],
            vec![0xff, 0xf0 + register],
            vec![0x58 + register],
            vec![0x8f, 0xc0 + register],
        ] {
            let mut bytes = vec![0x90];
            bytes.extend_from_slice(&instruction);
            bytes.push(0x90);
            let mut engine = with_code(&bytes);
            let snapshot = engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(0x1000), bytes.len())
                .unwrap();
            describe(&mut engine, 0x1000, bytes.len() as u32);
            let before = engine.arena().to_vec();
            assert_eq!(engine.compile(1), Ok(1), "{instruction:02x?}");
            assert_eq!(engine.arena(), before);
            assert_eq!(code_bytes(&engine, 0x1000, bytes.len()), bytes);
            assert!(engine.memory().unwrap().is_code_current(&snapshot));
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(0), Access::Read)
                    .is_err()
            );
            engine.guard(KEY, 1).unwrap();
        }
    }
}

#[test]
fn immediate_push_accepts_full_width_values_and_signed_byte_boundaries() {
    for instruction in [
        &[0x68, 0, 0, 0, 0][..],
        &[0x68, 0xff, 0xff, 0xff, 0xff][..],
        &[0x68, 0, 0, 0, 0x80][..],
        &[0x68, 0x78, 0x56, 0x34, 0x12][..],
        &[0x6a, 0x80][..],
        &[0x6a, 0xff][..],
        &[0x6a, 0][..],
        &[0x6a, 0x7f][..],
    ] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        bytes.push(0x90);
        let mut engine = with_code(&bytes);
        assert_eq!(compile(&mut engine, 0x1000, bytes.len() as u32), Ok(1));
        assert_eq!(code_bytes(&engine, 0x1000, bytes.len()), bytes);
        engine.guard(KEY, 1).unwrap();
    }
}

#[test]
fn near_ret_cleanup_accepts_unsigned_unaligned_counts_only_at_block_end() {
    for cleanup in [0_u16, 1, 4, u16::MAX] {
        let [low, high] = cleanup.to_le_bytes();
        let bytes = [0x90, 0xc2, low, high, 0x90];
        let mut engine = with_code(&bytes);
        assert_eq!(compile(&mut engine, 0x1000, 4), Ok(1));
        let artifact = engine.artifact_bytes().unwrap().to_vec();
        assert_eq!(
            compile(&mut engine, 0x1000, 5),
            Err(HostError::Compile(instruction_error(
                0x1001,
                InstructionError::InvalidBlockEnd
            )))
        );
        assert_eq!(engine.generation(), 1);
        assert_eq!(engine.artifact_bytes().unwrap(), artifact);
        engine.guard(KEY, 1).unwrap();
    }
}

#[test]
fn call_and_ret_still_terminate_blocks_after_sequential_push_and_pop() {
    for terminator in [&[0xe8, 0, 0, 0, 0][..], &[0xc3][..], &[0xc2, 4, 0][..]] {
        let mut bytes = vec![0x50, 0x58];
        bytes.extend_from_slice(terminator);
        bytes.push(0x90);
        let mut engine = with_code(&bytes);
        assert_eq!(compile(&mut engine, 0x1000, bytes.len() as u32 - 1), Ok(1));
        assert_eq!(
            compile(&mut engine, 0x1000, bytes.len() as u32),
            Err(HostError::Compile(instruction_error(
                0x1002,
                InstructionError::InvalidBlockEnd
            )))
        );
        assert_eq!(engine.generation(), 1);
        engine.guard(KEY, 1).unwrap();
    }
}

#[test]
fn standalone_keeps_new_stack_values_outside_its_profile_with_original_error_priority() {
    for instruction in [
        &[0x54][..],
        &[0xff, 0xf4][..],
        &[0x5c][..],
        &[0x8f, 0xc4][..],
        &[0x68, 0x78, 0x56, 0x34, 0x12][..],
        &[0x6a, 0xff][..],
        &[0xc2, 0xff, 0xff][..],
    ] {
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
                Some(expected)
            );
            assert_eq!(
                compile_region(&memory, &specs, CompileLimits::default()).err(),
                Some(expected)
            );
            assert!(memory.is_code_current(&snapshot));
        }
    }
}

#[test]
fn memory_push_and_pop_compile_sequentially_after_a_register_stack_prefix() {
    for instruction in [
        &[0xff, 0x33][..],
        &[0xff, 0x74, 0x24, 4][..],
        &[0xff, 0x35, 0, 0x80, 0, 0][..],
        &[0x8f, 0x03][..],
        &[0x8f, 0x04, 0x24][..],
        &[0x8f, 0x05, 0, 0x80, 0, 0][..],
    ] {
        for trailing in [false, true] {
            let mut bytes = vec![0x54, 0x5c];
            bytes.extend_from_slice(instruction);
            if trailing {
                bytes.push(0x90);
            }
            let mut engine = with_code(&bytes);
            assert_eq!(compile(&mut engine, 0x1000, bytes.len() as u32), Ok(1));
            assert_eq!(engine.generation(), 1);
            engine.guard(KEY, 1).unwrap();
        }
    }
}

#[test]
fn expanded_stack_operations_keep_prefix_and_far_decoder_exclusions() {
    for instruction in [
        &[0x66, 0x54][..],
        &[0x66, 0x5c][..],
        &[0x66, 0xc2, 4, 0][..],
        &[0x67, 0x54][..],
        &[0xf3, 0x58][..],
        &[0xca, 0xff, 0xff][..],
    ] {
        let mut bytes = vec![0x54, 0x5c];
        bytes.extend_from_slice(instruction);
        let mut engine = with_code(&bytes);
        assert_eq!(
            compile(&mut engine, 0x1000, bytes.len() as u32),
            Err(HostError::Compile(instruction_error(
                0x1002,
                InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode))
            )))
        );
        assert_eq!(engine.generation(), 0);
    }
    let mut engine = with_code(&[0x54, 0x5c, 0x64, 0x58]);
    assert_eq!(
        compile(&mut engine, 0x1000, 4),
        Err(HostError::Compile(instruction_error(
            0x1002,
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Segment))
        )))
    );
}

#[test]
fn declared_spans_cannot_cut_new_multibyte_stack_instructions() {
    for instruction in [
        &[0xff, 0xf4][..],
        &[0x8f, 0xc4][..],
        &[0x68, 1, 0, 0, 0][..],
        &[0x6a, 0xff][..],
        &[0xc2, 4, 0][..],
    ] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        let mut engine = with_code(&bytes);
        assert_eq!(
            compile(&mut engine, 0x1000, bytes.len() as u32 - 1),
            Err(HostError::Compile(instruction_error(
                0x1001,
                InstructionError::InvalidBlockEnd
            )))
        );
        assert_eq!(engine.generation(), 0);
    }
}

#[test]
fn multibyte_stack_values_stamp_both_consumed_code_pages() {
    for instruction in [&[0xff, 0xf4][..], &[0x8f, 0xc4][..], &[0xc2, 0, 0][..]] {
        for page in [0x1000, 0x2000] {
            let mut engine = EngineInstance::new(2, KEY).unwrap();
            engine.map(0x1000, 2, 7).unwrap();
            upload(&mut engine, 0x1fff, instruction);
            assert_eq!(
                compile(&mut engine, 0x1fff, instruction.len() as u32),
                Ok(1)
            );
            engine.guard(KEY, 1).unwrap();
            engine.protect(page, 1, 7).unwrap();
            assert_eq!(engine.guard(KEY, 1), Err(HostError::CodeInvalidated));
            assert_eq!(engine.artifact_bytes(), Err(HostError::CodeInvalidated));
            assert_eq!(engine.generation(), 1);
        }
    }
}
