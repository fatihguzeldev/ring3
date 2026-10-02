use ring3_engine::memory::{Access, GuestAddress, MemoryError};
use ring3_engine::process::{EngineInstance, HostError};

const KEY: u64 = 0xfedc_ba98_7654_3210;

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

fn engine_with_code() -> EngineInstance {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    upload(&mut engine, 0x1000, &[0xb8, 0, 0, 0, 0]);
    engine
}

#[test]
fn fixed_arena_has_canonical_initial_state_exit_and_helper_records() {
    let engine = EngineInstance::new(1, KEY).unwrap();
    assert!(engine.is_open());
    assert_eq!(engine.key(), KEY);
    assert_eq!(engine.generation(), 0);
    assert_eq!(engine.arena().len(), 4236);
    assert_ne!(engine.arena_address(), 0);
    let arena = engine.arena();
    for (offset, magic, length) in [(0, b"R3ST", 56u32), (56, b"R3EX", 40), (100, b"R3MH", 40)] {
        assert_eq!(&arena[offset..offset + 4], magic);
        assert_eq!(&arena[offset + 4..offset + 8], &[1, 0, 1, 0]);
        assert_eq!(&arena[offset + 8..offset + 12], &length.to_le_bytes());
        assert_eq!(&arena[offset + 12..offset + 16], &[0; 4]);
    }
    assert_eq!(&arena[16..52], &[0; 36]);
    assert_eq!(&arena[52..56], &[2, 0, 0, 0]);
    assert_eq!(
        &arena[72..96],
        &[
            1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
        ]
    );
    assert_eq!(&arena[96..100], &[0; 4]);
    assert_eq!(&arena[116..140], &[0; 24]);
    assert_eq!(&arena[140..], &[0; 4096]);
}

#[test]
fn arena_address_survives_moves_mapping_and_compilation_allocations() {
    let engine = engine_with_code();
    let pointer = engine.arena_address();
    let mut moved = Box::new(engine);
    assert_eq!(moved.arena_address(), pointer);
    moved.map(0x8000, 1, 3).unwrap();
    compile(&mut moved, 0x1000, 5).unwrap();
    assert_eq!(moved.arena_address(), pointer);
}

#[test]
fn zero_key_is_invalid_and_guard_requires_an_installed_artifact() {
    assert!(matches!(
        EngineInstance::new(1, 0),
        Err(HostError::InvalidRequest)
    ));
    let engine = EngineInstance::new(1, KEY).unwrap();
    assert!(matches!(
        engine.guard(KEY, 0),
        Err(HostError::InvalidArtifact)
    ));
    assert!(matches!(
        engine.artifact_bytes(),
        Err(HostError::InvalidArtifact)
    ));
}

#[test]
fn all_three_permission_bits_are_independent_and_unknown_bits_are_rejected() {
    let mut engine = EngineInstance::new(8, KEY).unwrap();
    for bits in 0..8 {
        let address = (bits + 1) * 4096;
        engine.map(address, 1, bits).unwrap();
        for (access, bit) in [(Access::Read, 1), (Access::Write, 2), (Access::Execute, 4)] {
            assert_eq!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(address), access)
                    .is_ok(),
                bits & bit != 0
            );
        }
    }
    for bits in [8, 0x8000_0000, u32::MAX] {
        assert!(matches!(
            engine.map(0x20000, 1, bits),
            Err(HostError::InvalidRequest)
        ));
        assert!(matches!(
            engine.protect(0x1000, 1, bits),
            Err(HostError::InvalidRequest)
        ));
    }
    assert_eq!(engine.memory().unwrap().mapped_pages(), 8);
}

#[test]
fn invalid_page_ranges_are_typed_memory_errors() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    for (address, pages) in [(1, 1), (0, 0), (0xffff_f000, 2)] {
        assert!(matches!(
            engine.map(address, pages, 7),
            Err(HostError::Memory(MemoryError::InvalidRange))
        ));
        assert!(matches!(
            engine.protect(address, pages, 7),
            Err(HostError::Memory(MemoryError::InvalidRange))
        ));
        assert!(matches!(
            engine.unmap(address, pages),
            Err(HostError::Memory(MemoryError::InvalidRange))
        ));
    }
}

#[test]
fn upload_is_bounded_by_transfer_and_guest_write_is_atomic() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(0x1000, 1, 3).unwrap();
    upload(&mut engine, 0x1ffe, &[7, 8]);
    engine.arena_mut().unwrap()[140..144].copy_from_slice(&[1, 2, 3, 4]);
    assert!(matches!(
        engine.upload(0x1ffe, 4),
        Err(HostError::Memory(_))
    ));
    let mut output = [0; 2];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x1ffe), &mut output)
        .unwrap();
    assert_eq!(output, [7, 8]);
    assert!(matches!(
        engine.upload(0x1000, 4097),
        Err(HostError::InvalidRequest)
    ));
    engine.upload(u32::MAX, 0).unwrap();
}

#[test]
fn successful_replacement_advances_generation_and_failed_compile_keeps_artifact() {
    let mut engine = engine_with_code();
    let first = compile(&mut engine, 0x1000, 5).unwrap();
    assert_eq!(first, 1);
    assert_eq!(engine.generation(), first);
    let original = engine.artifact_bytes().unwrap().to_vec();
    engine.guard(KEY, first).unwrap();
    assert!(matches!(
        compile(&mut engine, 0x1000, 4),
        Err(HostError::Compile(_))
    ));
    assert_eq!(engine.generation(), first);
    assert_eq!(engine.artifact_bytes().unwrap(), original);
    engine.guard(KEY, first).unwrap();
    let replacement = compile(&mut engine, 0x1000, 5).unwrap();
    assert_eq!(replacement, first + 1);
    assert!(matches!(
        engine.guard(KEY, first),
        Err(HostError::InvalidArtifact)
    ));
    assert!(matches!(
        engine.guard(KEY ^ 1, replacement),
        Err(HostError::InvalidArtifact)
    ));
    engine.guard(KEY, replacement).unwrap();
}

#[test]
fn invalid_compile_count_preserves_existing_generation() {
    let mut engine = engine_with_code();
    let generation = compile(&mut engine, 0x1000, 5).unwrap();
    for count in [0, 9, u32::MAX] {
        assert!(matches!(
            engine.compile(count),
            Err(HostError::InvalidRequest)
        ));
        assert_eq!(engine.generation(), generation);
        engine.guard(KEY, generation).unwrap();
    }
}

#[test]
fn code_write_protect_and_remap_invalidate_guard_without_arena_mutation() {
    for mutation in 0..3 {
        let mut engine = engine_with_code();
        let generation = compile(&mut engine, 0x1000, 5).unwrap();
        match mutation {
            0 => upload(&mut engine, 0x1000, &[0xb8, 0, 0, 0, 0]),
            1 => engine.protect(0x1000, 1, 7).unwrap(),
            2 => {
                engine.unmap(0x1000, 1).unwrap();
                engine.map(0x1000, 1, 7).unwrap();
                upload(&mut engine, 0x1000, &[0xb8, 0, 0, 0, 0]);
            }
            _ => unreachable!(),
        }
        let arena = engine.arena().to_vec();
        assert!(matches!(
            engine.guard(KEY, generation),
            Err(HostError::CodeInvalidated)
        ));
        assert!(matches!(
            engine.artifact_bytes(),
            Err(HostError::CodeInvalidated)
        ));
        assert_eq!(engine.arena(), arena);
    }
}

#[test]
fn close_retains_arena_and_rejects_every_live_context_operation() {
    let mut engine = engine_with_code();
    let generation = compile(&mut engine, 0x1000, 5).unwrap();
    let pointer = engine.arena_address();
    engine.close();
    assert!(!engine.is_open());
    assert_eq!(engine.generation(), 0);
    assert_eq!(engine.arena_address(), pointer);
    let arena = engine.arena().to_vec();
    assert!(matches!(engine.arena_mut(), Err(HostError::Closed)));
    assert!(matches!(engine.memory(), Err(HostError::Closed)));
    assert!(matches!(engine.map(0x1000, 1, 7), Err(HostError::Closed)));
    assert!(matches!(
        engine.protect(0x1000, 1, 7),
        Err(HostError::Closed)
    ));
    assert!(matches!(engine.unmap(0x1000, 1), Err(HostError::Closed)));
    assert!(matches!(engine.upload(0x1000, 1), Err(HostError::Closed)));
    assert!(matches!(engine.compile(1), Err(HostError::Closed)));
    assert!(matches!(engine.artifact_bytes(), Err(HostError::Closed)));
    assert!(matches!(
        engine.guard(KEY, generation),
        Err(HostError::Closed)
    ));
    assert!(matches!(engine.read32(0x1000), Err(HostError::Closed)));
    assert!(matches!(engine.write32(0x1000, 0), Err(HostError::Closed)));
    engine.close();
    assert_eq!(engine.arena(), arena);
}

#[test]
fn constructor_rejects_empty_and_oversized_resident_capacity() {
    for pages in [0, 4097, u32::MAX] {
        assert!(EngineInstance::new(pages, KEY).is_err());
    }
}

#[test]
fn upload_consumes_the_entire_four_kib_transfer_window() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(0x1000, 1, 3).unwrap();
    let expected: Vec<_> = (0..4096)
        .map(|index| (index as u8).wrapping_mul(17))
        .collect();
    engine.arena_mut().unwrap()[140..4236].copy_from_slice(&expected);
    engine.upload(0x1000, 4096).unwrap();
    let mut actual = vec![0; 4096];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x1000), &mut actual)
        .unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn compile_reads_all_eight_block_descriptors_including_the_last() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    for unsupported_last in [true, false] {
        let mut bytes = [0x90; 8];
        if unsupported_last {
            bytes[7] = 0x40;
        }
        upload(&mut engine, 0x1000, &bytes);
        let transfer = &mut engine.arena_mut().unwrap()[140..204];
        for index in 0..8 {
            transfer[index * 8..index * 8 + 4]
                .copy_from_slice(&(0x1000u32 + index as u32).to_le_bytes());
            transfer[index * 8 + 4..index * 8 + 8].copy_from_slice(&1u32.to_le_bytes());
        }
        if unsupported_last {
            assert!(matches!(
                engine.compile(8),
                Err(HostError::Compile(
                    ring3_engine::cpu::dbt::CompileError::Instruction {
                        pc: GuestAddress(0x1007),
                        cause: ring3_engine::cpu::dbt::InstructionError::BackendUnsupported,
                    }
                ))
            ));
        } else {
            assert_eq!(engine.compile(8).unwrap(), 1);
            assert!(engine.artifact_bytes().unwrap().starts_with(b"\0asm"));
        }
    }
}
