use ring3_engine::cpu::dbt::{
    BlockSpec, CompileError, CompileLimits, InstructionError, prepare_region,
};
use ring3_engine::cpu::x86::decode::DecodeError;
use ring3_engine::memory::{
    Access, AddressSpace, FaultReason, GuestAddress, PageRange, Permissions,
};

fn range(address: u32, pages: u32) -> PageRange {
    PageRange::new(GuestAddress(address), pages).unwrap()
}

fn spec(entry: u32, byte_length: u32) -> BlockSpec {
    BlockSpec {
        entry: GuestAddress(entry),
        byte_length,
    }
}

fn code_memory(pc: u32, bytes: &[u8]) -> AddressSpace {
    let first = pc & !0xfff;
    let pages = (pc as u64 - first as u64 + bytes.len() as u64).div_ceil(4096) as u32;
    let mut memory = AddressSpace::new(pages + 2).unwrap();
    memory
        .map_zeroed(range(first, pages), Permissions::ALL)
        .unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory
}

#[test]
fn default_limits_are_the_declared_hard_maxima() {
    let limits = CompileLimits::default();
    assert_eq!(limits.blocks, 8);
    assert_eq!(limits.instructions, 64);
    assert_eq!(limits.wasm_bytes, 65536);
}

#[test]
fn invalid_limits_are_rejected_before_reading_guest_code() {
    let memory = AddressSpace::new(1).unwrap();
    for limits in [
        CompileLimits {
            blocks: 0,
            ..CompileLimits::default()
        },
        CompileLimits {
            blocks: 9,
            ..CompileLimits::default()
        },
        CompileLimits {
            instructions: 0,
            ..CompileLimits::default()
        },
        CompileLimits {
            instructions: 65,
            ..CompileLimits::default()
        },
        CompileLimits {
            wasm_bytes: 0,
            ..CompileLimits::default()
        },
        CompileLimits {
            wasm_bytes: 65537,
            ..CompileLimits::default()
        },
    ] {
        assert!(matches!(
            prepare_region(&memory, &[spec(0x1000, 1)], limits),
            Err(CompileError::InvalidLimits)
        ));
    }
}

#[test]
fn positive_lower_limits_are_allowed() {
    let memory = code_memory(0x1000, &[0x90]);
    let prepared = prepare_region(
        &memory,
        &[spec(0x1000, 1)],
        CompileLimits {
            blocks: 1,
            instructions: 1,
            wasm_bytes: 1024,
        },
    )
    .unwrap();
    assert_eq!(prepared.block_count(), 1);
    assert_eq!(prepared.instruction_count(), 1);
    assert!(prepared.is_current(&memory));
}

#[test]
fn empty_zero_length_overlapping_and_overflowing_blocks_are_rejected() {
    let memory = code_memory(0x1000, &[0x90; 16]);
    for blocks in [
        vec![],
        vec![spec(0x1000, 0)],
        vec![spec(u32::MAX, 2)],
        vec![spec(0x1000, 2), spec(0x1001, 1)],
        vec![spec(0x1001, 1), spec(0x1000, 2)],
        vec![spec(0x1000, 1), spec(0x1000, 1)],
    ] {
        assert!(matches!(
            prepare_region(&memory, &blocks, CompileLimits::default()),
            Err(CompileError::InvalidBlocks)
        ));
    }
}

#[test]
fn block_count_limit_rejects_excess_blocks() {
    let memory = code_memory(0x1000, &[0x90; 9]);
    let blocks: Vec<_> = (0..9).map(|index| spec(0x1000 + index, 1)).collect();
    assert!(matches!(
        prepare_region(&memory, &blocks, CompileLimits::default()),
        Err(CompileError::InvalidBlocks)
    ));
    let limited = CompileLimits {
        blocks: 1,
        ..CompileLimits::default()
    };
    assert!(matches!(
        prepare_region(&memory, &blocks[..2], limited),
        Err(CompileError::InvalidBlocks)
    ));
}

#[test]
fn adjacent_and_out_of_order_nonoverlapping_spans_are_allowed() {
    let memory = code_memory(0x1000, &[0x90; 4]);
    let prepared = prepare_region(
        &memory,
        &[spec(0x1002, 2), spec(0x1000, 2)],
        CompileLimits::default(),
    )
    .unwrap();
    assert_eq!(prepared.block_count(), 2);
    assert_eq!(prepared.instruction_count(), 4);
}

#[test]
fn eight_blocks_can_share_the_total_sixty_four_instruction_budget() {
    let memory = code_memory(0x1000, &[0x90; 64]);
    let blocks: Vec<_> = (0..8).map(|index| spec(0x1000 + index * 8, 8)).collect();
    let prepared = prepare_region(&memory, &blocks, CompileLimits::default()).unwrap();
    assert_eq!(prepared.block_count(), 8);
    assert_eq!(prepared.instruction_count(), 64);
}

#[test]
fn total_instruction_limits_are_enforced_across_blocks() {
    let memory = code_memory(0x1000, &[0x90; 65]);
    assert!(matches!(
        prepare_region(&memory, &[spec(0x1000, 65)], CompileLimits::default()),
        Err(CompileError::InstructionLimit)
    ));
    assert!(matches!(
        prepare_region(
            &memory,
            &[spec(0x1000, 1), spec(0x1001, 1)],
            CompileLimits {
                instructions: 1,
                ..CompileLimits::default()
            },
        ),
        Err(CompileError::InstructionLimit)
    ));
}

#[test]
fn admitted_register_operations_prepare_without_mutating_input_memory() {
    let bytes = [
        0x90, 0x89, 0xd8, 0xb8, 0x78, 0x56, 0x34, 0x12, 0x01, 0xd8, 0x83, 0xc0, 0xff, 0x29, 0xd8,
        0x83, 0xe8, 0x7f, 0x39, 0xd8, 0x3d, 0x78, 0x56, 0x34, 0x12,
    ];
    let mut memory = code_memory(0x1000, &bytes);
    memory
        .protect(range(0x1000, 1), Permissions::READ_EXECUTE)
        .unwrap();
    let snapshot = memory
        .snapshot_code(GuestAddress(0x1000), bytes.len())
        .unwrap();
    let offset = memory
        .resolve(GuestAddress(0x1000), Access::Execute)
        .unwrap();
    let prepared = prepare_region(
        &memory,
        &[spec(0x1000, bytes.len() as u32)],
        CompileLimits::default(),
    )
    .unwrap();
    assert_eq!(prepared.block_count(), 1);
    assert_eq!(prepared.instruction_count(), 9);
    assert!(prepared.is_current(&memory));
    assert!(memory.is_code_current(&snapshot));
    assert_eq!(
        memory
            .resolve(GuestAddress(0x1000), Access::Execute)
            .unwrap(),
        offset
    );
    let mut output = vec![0; bytes.len()];
    memory.read(GuestAddress(0x1000), &mut output).unwrap();
    assert_eq!(output, bytes);
    assert!(memory.resolve(GuestAddress(0x1000), Access::Write).is_err());
}

#[test]
fn direct_jump_and_all_sixteen_conditions_are_admitted_at_block_end() {
    let memory = code_memory(0x1000, &[0xe9, 0xfb, 0xef, 0, 0]);
    let prepared = prepare_region(&memory, &[spec(0x1000, 5)], CompileLimits::default()).unwrap();
    assert_eq!(prepared.instruction_count(), 1);
    for (short, near) in [
        (0x70, 0x80),
        (0x71, 0x81),
        (0x72, 0x82),
        (0x73, 0x83),
        (0x74, 0x84),
        (0x75, 0x85),
        (0x76, 0x86),
        (0x77, 0x87),
        (0x78, 0x88),
        (0x79, 0x89),
        (0x7a, 0x8a),
        (0x7b, 0x8b),
        (0x7c, 0x8c),
        (0x7d, 0x8d),
        (0x7e, 0x8e),
        (0x7f, 0x8f),
    ] {
        for bytes in [&[short, 0][..], &[0x0f, near, 0, 0, 0, 0][..]] {
            let memory = code_memory(0x1000, bytes);
            let prepared = prepare_region(
                &memory,
                &[spec(0x1000, bytes.len() as u32)],
                CompileLimits::default(),
            )
            .unwrap();
            assert_eq!(prepared.instruction_count(), 1);
        }
    }
}

#[test]
fn declared_end_must_be_an_instruction_boundary() {
    let memory = code_memory(0x1000, &[0x90, 0xb8, 0x78, 0x56, 0x34, 0x12]);
    assert!(matches!(
        prepare_region(&memory, &[spec(0x1000, 5)], CompileLimits::default()),
        Err(CompileError::Instruction {
            pc: GuestAddress(0x1001),
            cause: InstructionError::InvalidBlockEnd,
        })
    ));
}

#[test]
fn direct_control_transfer_before_declared_end_is_rejected_at_its_pc() {
    for bytes in [&[0x90, 0xeb, 0, 0x90][..], &[0x90, 0x74, 0, 0x90][..]] {
        let memory = code_memory(0x1000, bytes);
        assert!(matches!(
            prepare_region(&memory, &[spec(0x1000, 4)], CompileLimits::default()),
            Err(CompileError::Instruction {
                pc: GuestAddress(0x1001),
                cause: InstructionError::InvalidBlockEnd,
            })
        ));
    }
}

#[test]
fn operations_outside_backend_profile_are_rejected_at_their_own_pc() {
    for instruction in [
        &[0x8b, 0x03][..],
        &[0x89, 0x03][..],
        &[0x01, 0x03][..],
        &[0x03, 0x03][..],
        &[0x2b, 0x03][..],
        &[0x39, 0x03][..],
        &[0x3b, 0x03][..],
        &[0xff, 0xe0][..],
        &[0x50][..],
        &[0x58][..],
        &[0xc3][..],
        &[0xe8, 0, 0, 0, 0][..],
        &[0x0f, 0xb6, 0xc0][..],
        &[0x40][..],
        &[0xf7, 0xd8][..],
    ] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        let memory = code_memory(0x1000, &bytes);
        assert!(matches!(
            prepare_region(
                &memory,
                &[spec(0x1000, bytes.len() as u32)],
                CompileLimits::default()
            ),
            Err(CompileError::Instruction {
                pc: GuestAddress(0x1001),
                cause: InstructionError::BackendUnsupported,
            })
        ));
    }
}

#[test]
fn invalid_and_unsupported_decodings_are_wrapped_with_original_instruction_pc() {
    for (bytes, unsupported) in [
        (&[0x90, 0x8d, 0xc0][..], false),
        (&[0x90, 0xd9, 0xe8][..], true),
    ] {
        let memory = code_memory(0x1000, bytes);
        let error = prepare_region(
            &memory,
            &[spec(0x1000, bytes.len() as u32)],
            CompileLimits::default(),
        )
        .err()
        .unwrap();
        match error {
            CompileError::Instruction {
                pc: GuestAddress(0x1001),
                cause: InstructionError::Decode(error),
            } => {
                assert!(matches!(
                    (unsupported, error),
                    (false, DecodeError::InvalidEncoding) | (true, DecodeError::Unsupported(_))
                ));
            }
            other => panic!("expected original instruction decode failure, got {other:?}"),
        }
    }
}

#[test]
fn instruction_fetch_fault_preserves_original_pc_address_and_attempted_span() {
    let memory = code_memory(0x1ffd, &[0x90, 0xb8, 0x78]);
    let error = prepare_region(&memory, &[spec(0x1ffd, 6)], CompileLimits::default())
        .err()
        .unwrap();
    let CompileError::Instruction {
        pc,
        cause:
            InstructionError::Decode(DecodeError::MemoryFault {
                pc: decode_pc,
                fault,
                length,
            }),
    } = error
    else {
        panic!("expected instruction fetch fault, got {error:?}");
    };
    assert_eq!(pc.0, 0x1ffe);
    assert_eq!(decode_pc.0, 0x1ffe);
    assert_eq!(fault.address.0, 0x2000);
    assert_eq!(fault.access, Access::Execute);
    assert_eq!(fault.reason, FaultReason::Unmapped);
    assert_eq!(length, 3);
}

#[test]
fn final_address_nop_span_is_valid_even_when_next_pc_wraps() {
    let memory = code_memory(u32::MAX, &[0x90]);
    let prepared = prepare_region(&memory, &[spec(u32::MAX, 1)], CompileLimits::default()).unwrap();
    assert_eq!(prepared.instruction_count(), 1);
    assert!(prepared.is_current(&memory));
}

#[test]
fn prepared_region_tracks_writes_to_each_cross_page_code_mapping() {
    for changed_address in [0x1000, 0x2fff] {
        let mut memory = code_memory(0x1fff, &[0x90; 3]);
        let prepared =
            prepare_region(&memory, &[spec(0x1fff, 3)], CompileLimits::default()).unwrap();
        assert!(prepared.is_current(&memory));
        memory
            .write(GuestAddress(changed_address), &[0x90])
            .unwrap();
        assert!(!prepared.is_current(&memory));
    }
}

#[test]
fn prepared_region_never_revives_after_protect_or_unmap_remap() {
    let mut memory = code_memory(0x1000, &[0x90]);
    let protected = prepare_region(&memory, &[spec(0x1000, 1)], CompileLimits::default()).unwrap();
    memory.protect(range(0x1000, 1), Permissions::ALL).unwrap();
    assert!(!protected.is_current(&memory));
    let remapped = prepare_region(&memory, &[spec(0x1000, 1)], CompileLimits::default()).unwrap();
    memory.unmap(range(0x1000, 1)).unwrap();
    assert!(!remapped.is_current(&memory));
    memory
        .map_zeroed(range(0x1000, 1), Permissions::ALL)
        .unwrap();
    memory.write(GuestAddress(0x1000), &[0x90]).unwrap();
    assert!(!protected.is_current(&memory));
    assert!(!remapped.is_current(&memory));
}

#[test]
fn unrelated_page_changes_preserve_plan_but_cross_space_identity_does_not() {
    let mut memory = code_memory(0x1000, &[0x90]);
    let prepared = prepare_region(&memory, &[spec(0x1000, 1)], CompileLimits::default()).unwrap();
    let other = code_memory(0x1000, &[0x90]);
    assert!(!prepared.is_current(&other));
    memory
        .map_zeroed(range(0x8000, 1), Permissions::ALL)
        .unwrap();
    memory.write(GuestAddress(0x8000), &[0x90]).unwrap();
    memory.protect(range(0x8000, 1), Permissions::READ).unwrap();
    memory.unmap(range(0x8000, 1)).unwrap();
    assert!(prepared.is_current(&memory));
}
