use ring3_engine::cpu::UnsupportedFeature;
use ring3_engine::cpu::dbt::{CompileError, InstructionError, RegistryError};
use ring3_engine::cpu::x86::decode::DecodeError;
use ring3_engine::memory::{Access, FaultReason, GuestAddress, MemoryFault};
use ring3_engine::process::{EngineInstance, HostError};

const KEY: u64 = 0xa123_4567_89ab_cdef;

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn describe(engine: &mut EngineInstance, blocks: &[(u32, u32)], gates: &[(u32, u32)]) {
    for (index, (first, second)) in blocks.iter().chain(gates).enumerate() {
        let pair = &mut engine.arena_mut().unwrap()[140 + index * 8..148 + index * 8];
        pair[..4].copy_from_slice(&first.to_le_bytes());
        pair[4..].copy_from_slice(&second.to_le_bytes());
    }
}

fn compile(
    engine: &mut EngineInstance,
    blocks: &[(u32, u32)],
    gates: &[(u32, u32)],
) -> Result<u32, HostError> {
    describe(engine, blocks, gates);
    engine.compile_with_gates(blocks.len() as u32, gates.len() as u32)
}

fn with_code(pc: u32, bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(4, KEY).unwrap();
    let first_page = pc & !0xfff;
    let pages = (u64::from(pc - first_page) + bytes.len() as u64).div_ceil(4096) as u32;
    engine.map(first_page, pages, 7).unwrap();
    upload(&mut engine, pc, bytes);
    engine
}

fn instruction_error(pc: u32, cause: InstructionError) -> HostError {
    HostError::Compile(CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    })
}

fn fetch_error(pc: u32, address: u32, reason: FaultReason) -> HostError {
    instruction_error(
        pc,
        InstructionError::Decode(DecodeError::MemoryFault {
            pc: GuestAddress(pc),
            fault: MemoryFault {
                address: GuestAddress(address),
                access: Access::Execute,
                reason,
            },
            length: 2,
        }),
    )
}

#[test]
fn registered_whole_ud2_block_compiles_as_a_numeric_gate() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..142].copy_from_slice(&[0x0f, 0x0b]);
    engine.upload(0x1000, 2).unwrap();
    let request = &mut engine.arena_mut().unwrap()[140..156];
    request[..4].copy_from_slice(&0x1000u32.to_le_bytes());
    request[4..8].copy_from_slice(&2u32.to_le_bytes());
    request[8..12].copy_from_slice(&0x1000u32.to_le_bytes());
    request[12..].copy_from_slice(&17u32.to_le_bytes());
    assert_eq!(engine.compile_with_gates(1, 1), Ok(1));
    engine.guard(KEY, 1).unwrap();
}

#[test]
fn gate_counts_are_rejected_before_transfer_descriptor_or_guest_reads() {
    for (blocks, gates) in [(0, 0), (9, 0), (1, 2), (8, 9), (u32::MAX, 0), (1, u32::MAX)] {
        let mut engine = EngineInstance::new(1, KEY).unwrap();
        engine.arena_mut().unwrap()[140..].fill(0xff);
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.compile_with_gates(blocks, gates),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.generation(), 0);
    }
    let mut closed = EngineInstance::new(1, KEY).unwrap();
    closed.close();
    let before = closed.arena().to_vec();
    assert_eq!(
        closed.compile_with_gates(0, u32::MAX),
        Err(HostError::Closed)
    );
    assert_eq!(closed.arena(), before);
}

#[test]
fn gate_structure_is_validated_before_any_unmapped_code_fetch() {
    for (blocks, gates) in [
        (vec![(0x1000, 2)], vec![(0x1000, 0)]),
        (
            vec![(0x1000, 2), (0x2000, 2)],
            vec![(0x1000, 1), (0x1000, 2)],
        ),
        (
            vec![(0x1000, 2), (0x2000, 2)],
            vec![(0x1000, 1), (0x2000, 1)],
        ),
        (vec![(0x1000, 2)], vec![(0x2000, 1)]),
        (vec![(0x1000, 1)], vec![(0x1000, 1)]),
        (vec![(0x1000, 3)], vec![(0x1000, 1)]),
        (vec![(0x1000, 3)], vec![(0x1001, 1)]),
    ] {
        let mut engine = EngineInstance::new(1, KEY).unwrap();
        describe(&mut engine, &blocks, &gates);
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.compile_with_gates(blocks.len() as u32, gates.len() as u32),
            Err(HostError::Compile(CompileError::InvalidGates))
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.generation(), 0);
    }
}

#[test]
fn gate_requests_keep_existing_block_bounds_and_overlap_rejections() {
    for blocks in [
        vec![(0x1000, 0)],
        vec![(u32::MAX, 2)],
        vec![(0x1000, 2), (0x1001, 2)],
        vec![(0x1000, 2), (0x1000, 2)],
    ] {
        let mut engine = EngineInstance::new(1, KEY).unwrap();
        let gates = [(blocks[0].0, 17)];
        assert_eq!(
            compile(&mut engine, &blocks, &gates),
            Err(HostError::Compile(CompileError::InvalidBlocks))
        );
        assert_eq!(engine.generation(), 0);
    }
}

#[test]
fn wrong_registered_markers_report_invalid_gate_at_the_original_pc() {
    for bytes in [[0x90, 0x90], [0x0b, 0x0f], [0x66, 0x0f], [0x0f, 0x0a]] {
        let mut engine = with_code(0x1234, &bytes);
        let snapshot = engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(0x1234), 2)
            .unwrap();
        assert_eq!(
            compile(&mut engine, &[(0x1234, 2)], &[(0x1234, 17)]),
            Err(instruction_error(0x1234, InstructionError::InvalidGate))
        );
        assert!(engine.memory().unwrap().is_code_current(&snapshot));
        assert_eq!(engine.generation(), 0);
    }
}

#[test]
fn unregistered_and_prefixed_ud2_keep_the_ordinary_decoder_rejection() {
    for instruction in [&[0x0f, 0x0b][..], &[0x66, 0x0f, 0x0b][..]] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        let mut engine = with_code(0x1000, &bytes);
        assert_eq!(
            compile(&mut engine, &[(0x1000, bytes.len() as u32)], &[]),
            Err(instruction_error(
                0x1001,
                InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode))
            ))
        );
        assert_eq!(engine.generation(), 0);
    }
}

#[test]
fn gates_fetch_exactly_two_execute_bytes_without_guest_data_or_stack_access() {
    let mut engine = with_code(0x1ffe, &[0x0f, 0x0b]);
    engine.protect(0x1000, 1, 4).unwrap();
    let snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x1ffe), 2)
        .unwrap();
    describe(&mut engine, &[(0x1ffe, 2)], &[(0x1ffe, u32::MAX)]);
    let before = engine.arena().to_vec();
    assert_eq!(engine.compile_with_gates(1, 1), Ok(1));
    assert_eq!(engine.arena(), before);
    assert!(engine.memory().unwrap().is_code_current(&snapshot));
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0x1ffe), Access::Read)
            .is_err()
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0x2000), Access::Execute)
            .is_err()
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0), Access::Read)
            .is_err()
    );
    engine.guard(KEY, 1).unwrap();
}

#[test]
fn gate_fetch_faults_keep_original_entry_attempted_span_and_first_denied_address() {
    for (bits, expected) in [
        (None, FaultReason::Unmapped),
        (Some(3), FaultReason::Permission),
    ] {
        let mut engine = with_code(0x1fff, &[0x0f, 0x0b]);
        if let Some(bits) = bits {
            engine.protect(0x2000, 1, bits).unwrap();
        } else {
            engine.unmap(0x2000, 1).unwrap();
        }
        assert_eq!(
            compile(&mut engine, &[(0x1fff, 2)], &[(0x1fff, 17)]),
            Err(fetch_error(0x1fff, 0x2000, expected))
        );
        assert_eq!(engine.generation(), 0);
    }
    let mut engine = with_code(0x1000, &[0x0f, 0x0b]);
    engine.protect(0x1000, 1, 3).unwrap();
    assert_eq!(
        compile(&mut engine, &[(0x1000, 2)], &[(0x1000, 17)]),
        Err(fetch_error(0x1000, 0x1000, FaultReason::Permission))
    );
    let mut engine = with_code(0x1fff, &[0x90, 0x0b]);
    engine.unmap(0x2000, 1).unwrap();
    assert_eq!(
        compile(&mut engine, &[(0x1fff, 2)], &[(0x1fff, 17)]),
        Err(fetch_error(0x1fff, 0x2000, FaultReason::Unmapped))
    );
}

#[test]
fn gate_can_end_at_the_guest_address_limit_without_wrapped_fetch() {
    let mut engine = with_code(0xffff_fffe, &[0x0f, 0x0b]);
    assert_eq!(
        compile(&mut engine, &[(0xffff_fffe, 2)], &[(0xffff_fffe, 17)]),
        Ok(1)
    );
    engine.guard(KEY, 1).unwrap();
}

#[test]
fn each_gate_charges_one_of_the_sixty_four_compilation_slots() {
    for (nops, expected) in [
        (63, Ok(1)),
        (64, Err(HostError::Compile(CompileError::InstructionLimit))),
    ] {
        for gate_first in [false, true] {
            let mut engine = with_code(0x1000, &vec![0x90; nops]);
            upload(&mut engine, 0x1100, &[0x0f, 0x0b]);
            let mut blocks = vec![(0x1000, nops as u32), (0x1100, 2)];
            if gate_first {
                blocks.reverse();
            }
            assert_eq!(compile(&mut engine, &blocks, &[(0x1100, 17)]), expected);
        }
    }
}

#[test]
fn all_eight_gate_descriptors_including_the_last_are_consumed() {
    let mut engine = with_code(
        0x1000,
        &[
            0x0f, 0x0b, 0x0f, 0x0b, 0x0f, 0x0b, 0x0f, 0x0b, 0x0f, 0x0b, 0x0f, 0x0b, 0x0f, 0x0b,
            0x0f, 0x0b,
        ],
    );
    let blocks: Vec<_> = (0..8).map(|index| (0x1000 + index * 2, 2)).collect();
    let mut gates: Vec<_> = (0..8)
        .map(|index| (0x1000 + index * 2, 0x1122_3300 + index))
        .collect();
    assert_eq!(compile(&mut engine, &blocks, &gates), Ok(1));
    let installed = engine.artifact_bytes().unwrap().to_vec();
    gates[7].1 = 0;
    assert_eq!(
        compile(&mut engine, &blocks, &gates),
        Err(HostError::Compile(CompileError::InvalidGates))
    );
    assert_eq!(engine.generation(), 1);
    assert_eq!(engine.artifact_bytes().unwrap(), installed);
    engine.guard(KEY, 1).unwrap();
}

#[test]
fn gate_snapshots_include_both_marker_pages_and_exclude_unrelated_data() {
    for page in [0x1000, 0x2000] {
        for change in 0..3 {
            let mut engine = with_code(0x1fff, &[0x0f, 0x0b]);
            engine.map(0x4000, 1, 3).unwrap();
            assert_eq!(compile(&mut engine, &[(0x1fff, 2)], &[(0x1fff, 17)]), Ok(1));
            engine.write32(0x4000, 0x7654_3210).unwrap();
            engine.protect(0x4000, 1, 1).unwrap();
            engine.guard(KEY, 1).unwrap();
            match change {
                0 => engine.protect(page, 1, 7).unwrap(),
                1 => engine.unmap(page, 1).unwrap(),
                _ => upload(
                    &mut engine,
                    if page == 0x1000 { 0x1fff } else { 0x2000 },
                    if page == 0x1000 { &[0x0f] } else { &[0x0b] },
                ),
            }
            assert_eq!(engine.guard(KEY, 1), Err(HostError::CodeInvalidated));
            assert_eq!(engine.artifact_bytes(), Err(HostError::CodeInvalidated));
        }
    }
}

#[test]
fn warmed_gate_only_replacement_and_resident_stay_stale_after_same_byte_write() {
    for resident in [false, true] {
        let mut engine = with_code(0x1000, &[0x0f, 0x0b]);
        describe(&mut engine, &[(0x1000, 2)], &[(0x1000, 17)]);
        let id = if resident {
            engine.compile_resident_with_gates(1, 1).unwrap().get()
        } else {
            u64::from(engine.compile_with_gates(1, 1).unwrap())
        };
        for _ in 0..2 {
            if resident {
                engine.guard_resident(KEY, id).unwrap();
                assert!(engine.resident_bytes(id).is_ok());
            } else {
                engine.guard(KEY, id as u32).unwrap();
                assert!(engine.artifact_bytes().is_ok());
            }
        }

        engine.write8(0x1000, 0x0f).unwrap();
        for _ in 0..2 {
            if resident {
                let error = HostError::Resident(RegistryError::CodeInvalidated);
                assert_eq!(engine.guard_resident(KEY, id), Err(error));
                assert_eq!(engine.resident_bytes(id), Err(error));
            } else {
                assert_eq!(
                    engine.guard(KEY, id as u32),
                    Err(HostError::CodeInvalidated)
                );
                assert_eq!(engine.artifact_bytes(), Err(HostError::CodeInvalidated));
            }
        }
    }
}

#[test]
fn failed_gate_install_preserves_old_artifact_arena_and_generation_until_replacement() {
    let mut engine = with_code(0x1000, &[0x90]);
    engine.map(0x3000, 1, 7).unwrap();
    upload(&mut engine, 0x3000, &[0x90, 0x90]);
    describe(&mut engine, &[(0x1000, 1)], &[]);
    assert_eq!(engine.compile(1), Ok(1));
    let installed = engine.artifact_bytes().unwrap().to_vec();
    for (blocks, gates, expected) in [
        (
            vec![(0x3000, 2)],
            vec![(0x3000, 0)],
            HostError::Compile(CompileError::InvalidGates),
        ),
        (
            vec![(0x3000, 2)],
            vec![(0x3000, 17)],
            instruction_error(0x3000, InstructionError::InvalidGate),
        ),
        (
            vec![(0x5000, 2)],
            vec![(0x5000, 17)],
            fetch_error(0x5000, 0x5000, FaultReason::Unmapped),
        ),
    ] {
        describe(&mut engine, &blocks, &gates);
        let before = engine.arena().to_vec();
        assert_eq!(engine.compile_with_gates(1, 1), Err(expected));
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.generation(), 1);
        assert_eq!(engine.artifact_bytes().unwrap(), installed);
        engine.guard(KEY, 1).unwrap();
    }
    upload(&mut engine, 0x3000, &[0x0f, 0x0b]);
    assert_eq!(compile(&mut engine, &[(0x3000, 2)], &[(0x3000, 17)]), Ok(2));
    assert_eq!(engine.guard(KEY, 1), Err(HostError::InvalidArtifact));
    engine.guard(KEY, 2).unwrap();
}

#[test]
fn zero_gate_compile_preserves_legacy_bytes_and_ignores_trailing_gate_descriptors() {
    let mut legacy = with_code(0x1000, &[0x90]);
    let mut explicit = with_code(0x1000, &[0x90]);
    for engine in [&mut legacy, &mut explicit] {
        describe(engine, &[(0x1000, 1)], &[(u32::MAX, 0)]);
    }
    assert_eq!(legacy.compile(1), Ok(1));
    assert_eq!(explicit.compile_with_gates(1, 0), Ok(1));
    assert_eq!(
        legacy.artifact_bytes().unwrap(),
        explicit.artifact_bytes().unwrap()
    );
}
