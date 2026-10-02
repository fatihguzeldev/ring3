use ring3_engine::cpu::UnsupportedFeature;
use ring3_engine::cpu::dbt::{
    BlockSpec, CompileError, CompileLimits, InstructionError, compile_region, prepare_region,
};
use ring3_engine::cpu::x86::decode::{DecodeError, decode_one};
use ring3_engine::cpu::x86::ir::Operation;
use ring3_engine::memory::{
    Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
};
use ring3_engine::process::{EngineInstance, HostError};

const KEY: u64 = 0x8765_4321_1234_5678;

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn with_code(address: u32, bytes: &[u8]) -> EngineInstance {
    let first = address & !0xfff;
    let pages = (u64::from(address - first) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 2, KEY).unwrap();
    engine.map(first, pages, 7).unwrap();
    upload(&mut engine, address, bytes);
    engine
}

fn compile(engine: &mut EngineInstance, blocks: &[(u32, u32)]) -> Result<u32, HostError> {
    let transfer = &mut engine.arena_mut().unwrap()[140..];
    for (index, (entry, length)) in blocks.iter().enumerate() {
        transfer[index * 8..index * 8 + 4].copy_from_slice(&entry.to_le_bytes());
        transfer[index * 8 + 4..index * 8 + 8].copy_from_slice(&length.to_le_bytes());
    }
    engine.compile(blocks.len() as u32)
}

fn instruction_error(pc: u32, cause: InstructionError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    }
}

fn code_bytes(engine: &EngineInstance, address: u32, length: usize) -> Vec<u8> {
    let mut output = vec![0; length];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut output)
        .unwrap();
    output
}

#[test]
fn embedded_direct_call_and_exact_ret_compile_without_touching_stack_or_target_ram() {
    for bytes in [
        &[0xe8, 0, 0, 0, 0][..],
        &[0xe8, 0xfb, 0x6f, 0, 0][..],
        &[0xe8, 0xfb, 0xef, 0xff, 0xff][..],
        &[0xc3][..],
    ] {
        let mut engine = with_code(0x1000, bytes);
        let snapshot = engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(0x1000), bytes.len())
            .unwrap();
        let descriptor = &mut engine.arena_mut().unwrap()[140..148];
        descriptor[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
        descriptor[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        let before = engine.arena().to_vec();
        assert_eq!(engine.compile(1), Ok(1));
        assert_eq!(engine.arena(), before);
        assert_eq!(code_bytes(&engine, 0x1000, bytes.len()), bytes);
        assert!(engine.memory().unwrap().is_code_current(&snapshot));
        assert_eq!(
            &engine.artifact_bytes().unwrap()[..4],
            &[0, 0x61, 0x73, 0x6d]
        );
        engine.guard(KEY, 1).unwrap();
        for address in [0, 0x8000] {
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
fn call_and_ret_end_adjacent_nonoverlapping_blocks_in_either_descriptor_order() {
    let bytes = [0x90, 0xe8, 1, 0, 0, 0, 0x90, 0xc3];
    for blocks in [
        vec![(0x1000, 6), (0x1006, 2)],
        vec![(0x1006, 2), (0x1000, 6)],
    ] {
        let mut engine = with_code(0x1000, &bytes);
        assert_eq!(compile(&mut engine, &blocks), Ok(1));
        assert_eq!(code_bytes(&engine, 0x1000, bytes.len()), bytes);
        engine.guard(KEY, 1).unwrap();
    }
}

#[test]
fn public_standalone_call_and_ret_errors_keep_their_existing_priority() {
    for instruction in [&[0xe8, 0, 0, 0, 0][..], &[0xc3][..], &[0xc2, 0, 0][..]] {
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
            let blocks = [BlockSpec {
                entry: GuestAddress(0x1000),
                byte_length: bytes.len() as u32,
            }];
            let expected = instruction_error(0x1001, InstructionError::BackendUnsupported);
            assert_eq!(
                prepare_region(&memory, &blocks, CompileLimits::default()).err(),
                Some(expected)
            );
            assert_eq!(
                compile_region(&memory, &blocks, CompileLimits::default()).err(),
                Some(expected)
            );
            assert!(memory.is_code_current(&snapshot));
            let mut output = vec![0; bytes.len()];
            memory.read(GuestAddress(0x1000), &mut output).unwrap();
            assert_eq!(output, bytes);
        }
    }
}

#[test]
fn supported_mid_block_call_and_ret_report_invalid_end_at_their_original_pc() {
    for instruction in [&[0xe8, 0, 0, 0, 0][..], &[0xc3][..]] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        bytes.push(0x90);
        let mut engine = with_code(0x1000, &bytes);
        assert_eq!(
            compile(&mut engine, &[(0x1000, bytes.len() as u32)]),
            Err(HostError::Compile(instruction_error(
                0x1001,
                InstructionError::InvalidBlockEnd
            )))
        );
        assert_eq!(engine.generation(), 0);
        assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
        assert_eq!(code_bytes(&engine, 0x1000, bytes.len()), bytes);
    }
}

#[test]
fn zero_adjustment_immediate_ret_shares_plain_ret_ir_but_has_its_own_terminator_span() {
    let mut engine = with_code(0x1000, &[0xc3, 0xc2, 0, 0, 0x90]);
    let memory = engine.memory().unwrap();
    let plain = decode_one(memory, GuestAddress(0x1000)).unwrap();
    let immediate = decode_one(memory, GuestAddress(0x1001)).unwrap();
    assert_eq!(plain.operation(), &Operation::Return { stack_adjust: 0 });
    assert_eq!(immediate.operation(), plain.operation());
    assert_eq!(plain.length(), 1);
    assert_eq!(immediate.length(), 3);
    assert_eq!(compile(&mut engine, &[(0x1001, 3)]), Ok(1));
    assert_eq!(
        compile(&mut engine, &[(0x1001, 4)]),
        Err(HostError::Compile(instruction_error(
            0x1001,
            InstructionError::InvalidBlockEnd
        )))
    );
    assert_eq!(compile(&mut engine, &[(0x1000, 1)]), Ok(2));
}

#[test]
fn decoded_but_excluded_stack_forms_report_backend_error_even_with_trailing_bytes() {
    for instruction in [
        &[0xff, 0xd0][..],
        &[0xff, 0x13][..],
        &[0xff, 0x15, 0, 0x80, 0, 0][..],
        &[0xff, 0x33][..],
        &[0x8f, 0x03][..],
    ] {
        for trailing in [false, true] {
            let mut bytes = vec![0x90];
            bytes.extend_from_slice(instruction);
            if trailing {
                bytes.push(0x90);
            }
            let mut engine = with_code(0x1000, &bytes);
            assert_eq!(
                compile(&mut engine, &[(0x1000, bytes.len() as u32)]),
                Err(HostError::Compile(instruction_error(
                    0x1001,
                    InstructionError::BackendUnsupported
                )))
            );
            assert_eq!(engine.generation(), 0);
        }
    }
    for instruction in [&[0xff, 0xe0][..], &[0xff, 0x23][..]] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        let mut engine = with_code(0x1000, &bytes);
        assert_eq!(
            compile(&mut engine, &[(0x1000, bytes.len() as u32)]),
            Err(HostError::Compile(instruction_error(
                0x1001,
                InstructionError::BackendUnsupported
            )))
        );
    }
}

#[test]
fn far_prefixed_and_sixteen_bit_stack_forms_keep_decoder_rejection_at_their_pc() {
    for instruction in [
        &[0x9a, 0x78, 0x56, 0x34, 0x12, 8, 0][..],
        &[0xea, 0x78, 0x56, 0x34, 0x12, 8, 0][..],
        &[0xff, 0x1b][..],
        &[0xff, 0x2b][..],
        &[0xcb][..],
        &[0xca, 0, 0][..],
        &[0x66, 0xe8, 0x34, 0x12][..],
        &[0x66, 0xc3][..],
        &[0xf3, 0xc3][..],
        &[0x67, 0xe8, 0, 0, 0, 0][..],
    ] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        let mut engine = with_code(0x1000, &bytes);
        assert_eq!(
            compile(&mut engine, &[(0x1000, bytes.len() as u32)]),
            Err(HostError::Compile(instruction_error(
                0x1001,
                InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode))
            )))
        );
        assert_eq!(engine.generation(), 0);
    }
}

#[test]
fn declared_call_end_and_truncated_fetch_keep_precise_error_spans() {
    let mut engine = with_code(0x1000, &[0x90, 0xe8, 0, 0, 0, 0]);
    assert_eq!(
        compile(&mut engine, &[(0x1000, 5)]),
        Err(HostError::Compile(instruction_error(
            0x1001,
            InstructionError::InvalidBlockEnd
        )))
    );
    let mut engine = with_code(0x1fff, &[0xe8]);
    assert_eq!(
        compile(&mut engine, &[(0x1fff, 5)]),
        Err(HostError::Compile(instruction_error(
            0x1fff,
            InstructionError::Decode(DecodeError::MemoryFault {
                pc: GuestAddress(0x1fff),
                fault: MemoryFault {
                    address: GuestAddress(0x2000),
                    access: Access::Execute,
                    reason: FaultReason::Unmapped
                },
                length: 2,
            })
        )))
    );
}

#[test]
fn last_guest_byte_ret_and_final_five_byte_call_are_valid_but_overflowing_spans_are_not() {
    for (address, bytes) in [
        (u32::MAX, &[0xc3][..]),
        (0xffff_fffb, &[0xe8, 0, 0, 0, 0][..]),
    ] {
        let mut engine = with_code(address, bytes);
        assert_eq!(
            compile(&mut engine, &[(address, bytes.len() as u32)]),
            Ok(1)
        );
        engine.guard(KEY, 1).unwrap();
        assert_eq!(
            compile(&mut engine, &[(address, bytes.len() as u32 + 1)]),
            Err(HostError::Compile(CompileError::InvalidBlocks))
        );
        assert_eq!(engine.generation(), 1);
        engine.guard(KEY, 1).unwrap();
    }
}

#[test]
fn total_instruction_and_block_limits_include_stack_terminators() {
    for terminator in [&[0xe8, 0, 0, 0, 0][..], &[0xc3][..]] {
        for (nops, expected) in [
            (63, Ok(1)),
            (64, Err(HostError::Compile(CompileError::InstructionLimit))),
        ] {
            let mut bytes = vec![0x90; nops];
            bytes.extend_from_slice(terminator);
            let mut engine = with_code(0x1000, &bytes);
            assert_eq!(
                compile(&mut engine, &[(0x1000, bytes.len() as u32)]),
                expected
            );
        }
    }
    let mut engine = with_code(0x1000, &[0xc3; 9]);
    let blocks: Vec<_> = (0..8).map(|index| (0x1000 + index, 1)).collect();
    assert_eq!(compile(&mut engine, &blocks), Ok(1));
    let before = engine.arena().to_vec();
    assert_eq!(engine.compile(9), Err(HostError::InvalidRequest));
    assert_eq!(engine.arena(), before);
    assert_eq!(
        compile(&mut engine, &[(0x1000, 1), (0x1000, 1)]),
        Err(HostError::Compile(CompileError::InvalidBlocks))
    );
    assert_eq!(engine.generation(), 1);
    engine.guard(KEY, 1).unwrap();
}

#[test]
fn cross_page_call_stamps_both_code_pages_and_ignores_unrelated_stack_page_changes() {
    for address in [0x1000, 0x2000] {
        let mut engine = with_code(0x1ffd, &[0xe8, 0, 0, 0, 0]);
        engine.map(0x4000, 1, 3).unwrap();
        assert_eq!(compile(&mut engine, &[(0x1ffd, 5)]), Ok(1));
        engine.write32(0x4000, 0x1122_3344).unwrap();
        engine.protect(0x4000, 1, 1).unwrap();
        engine.guard(KEY, 1).unwrap();
        engine.protect(address, 1, 7).unwrap();
        assert_eq!(engine.guard(KEY, 1), Err(HostError::CodeInvalidated));
        assert_eq!(engine.artifact_bytes(), Err(HostError::CodeInvalidated));
        assert_eq!(engine.generation(), 1);
    }
}

#[test]
fn failed_stack_compile_preserves_the_installed_artifact_and_successful_retry_replaces_it() {
    let mut engine = with_code(0x1000, &[0x90]);
    engine.map(0x3000, 1, 7).unwrap();
    assert_eq!(compile(&mut engine, &[(0x1000, 1)]), Ok(1));
    let original = engine.artifact_bytes().unwrap().to_vec();
    for (bytes, cause) in [
        (
            &[0x90, 0xff, 0x33, 0x90][..],
            InstructionError::BackendUnsupported,
        ),
        (
            &[0x90, 0xe8, 0, 0, 0, 0, 0x90][..],
            InstructionError::InvalidBlockEnd,
        ),
    ] {
        upload(&mut engine, 0x3000, bytes);
        let descriptor = &mut engine.arena_mut().unwrap()[140..148];
        descriptor[..4].copy_from_slice(&0x3000_u32.to_le_bytes());
        descriptor[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.compile(1),
            Err(HostError::Compile(instruction_error(0x3001, cause)))
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.generation(), 1);
        assert_eq!(engine.artifact_bytes().unwrap(), original);
        engine.guard(KEY, 1).unwrap();
        assert_eq!(code_bytes(&engine, 0x3000, bytes.len()), bytes);
    }
    upload(&mut engine, 0x3000, &[0xc3]);
    assert_eq!(compile(&mut engine, &[(0x3000, 1)]), Ok(2));
    assert_eq!(engine.guard(KEY, 1), Err(HostError::InvalidArtifact));
    engine.guard(KEY, 2).unwrap();
}
