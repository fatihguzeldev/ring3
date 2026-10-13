use super::super::{
    ArtifactError, compile_embedded_entry_region, compile_embedded_region, compile_entry_region,
};
use super::*;
use crate::memory::{PageRange, Permissions};

#[test]
fn embedded_seeds_keep_order_and_match_explicit_gate_memory_stack_emission() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    for (address, bytes) in [
        (0x1100, &[0x0f, 0x0b][..]),
        (0x1300, &[0x0f, 0x0b][..]),
        (0x1400, &[0x8b, 0x44, 0x24, 4, 0x50, 0xff, 0x17, 0xf4][..]),
        (0x1600, &[0xc2, 8, 0][..]),
    ] {
        memory.write(GuestAddress(address), bytes).unwrap();
    }
    let entries = [0x1100, 0x1400, 0x1600, 0x1300].map(GuestAddress);
    let gates = [
        GateSpec {
            entry: entries[0],
            id: 17,
        },
        GateSpec {
            entry: entries[3],
            id: 18,
        },
    ];
    let specs =
        [(0x1100, 2), (0x1400, 7), (0x1600, 3), (0x1300, 2)].map(|(pc, length)| BlockSpec {
            entry: GuestAddress(pc),
            byte_length: length,
        });
    let limits = CompileLimits::default();
    let prepared =
        prepare_embedded_entry_region(&memory, &entries, limits, &gates, None, None).unwrap();
    assert_eq!(
        (prepared.block_count(), prepared.instruction_count()),
        (4, 6)
    );
    assert_eq!(prepared.blocks[0].gate().unwrap().entry, entries[0]);
    assert_eq!(
        prepared.blocks[1]
            .instructions
            .iter()
            .map(|instruction| instruction.pc().0)
            .collect::<Vec<_>>(),
        [0x1400, 0x1404, 0x1405]
    );
    assert_eq!(prepared.blocks[2].instructions[0].pc(), entries[2]);
    assert_eq!(prepared.blocks[3].gate().unwrap().entry, entries[3]);
    assert!(prepared.is_current(&memory));
    // no stack or indirect-source data page exists; preparation uses Execute bytes only.
    let discovered = compile_embedded_entry_region(
        &memory,
        &entries,
        limits,
        91,
        7,
        &gates,
        crate::cpu::x86::decode::CodeFetchContext::from_segments(
            &crate::cpu::x86::SegmentContext::default(),
        )
        .unwrap(),
        None,
    )
    .unwrap();
    let explicit = compile_embedded_region(
        &memory,
        &specs,
        limits,
        91,
        7,
        &gates,
        crate::cpu::x86::decode::CodeFetchContext::from_segments(
            &crate::cpu::x86::SegmentContext::default(),
        )
        .unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(discovered.metadata(), explicit.metadata());
    assert_eq!(
        discovered.wasm_bytes(&memory).unwrap(),
        explicit.wasm_bytes(&memory).unwrap()
    );
}

#[test]
fn terminal_snapshot_omits_unfetched_successor_page_and_still_guards_consumed_bytes() {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 2).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1ffe), &[0xeb, 0]).unwrap();
    memory.write(GuestAddress(0x2000), &[0x40]).unwrap();
    let artifact =
        compile_entry_region(&memory, &[GuestAddress(0x1ffe)], CompileLimits::default()).unwrap();
    assert_eq!(artifact.metadata().instructions, 1);
    memory.write(GuestAddress(0x2000), &[0x40]).unwrap();
    assert!(artifact.wasm_bytes(&memory).is_ok());
    memory.write(GuestAddress(0x1ffe), &[0xeb, 0]).unwrap();
    assert_eq!(
        artifact.wasm_bytes(&memory),
        Err(ArtifactError::CodeInvalidated)
    );
}

#[test]
fn runtime_entry_fault_preparation_keeps_strict_public_refusals_and_terminal_bounds() {
    use crate::cpu::x86::{ControlOperation, decode::DecodeError, ir::Operation};
    for (bytes, operation) in [
        (&[0xf0, 0x90][..], ControlOperation::InvalidOpcode),
        (&[0xf0, 0xf4][..], ControlOperation::InvalidOpcode),
        (&[0xf4][..], ControlOperation::GeneralProtectionZero),
        (&[0x2e; 15][..], ControlOperation::GeneralProtectionZero),
    ] {
        let entry = GuestAddress(0x2000 - bytes.len() as u32);
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(entry, bytes).unwrap();
        let limits = CompileLimits::default();
        let strict =
            prepare_embedded_entry_region(&memory, &[entry], limits, &[], None, None).unwrap_err();
        assert!(matches!(
            strict,
            CompileError::Instruction {
                cause: InstructionError::Decode(
                    DecodeError::InvalidEncoding | DecodeError::Unsupported(_)
                ),
                ..
            }
        ));
        let prepared = prepare_runtime_entry(&memory, entry, limits, None, None).unwrap();
        assert_eq!(
            (prepared.block_count(), prepared.instruction_count()),
            (1, 1)
        );
        let instruction = &prepared.blocks[0].instructions[0];
        assert_eq!(*instruction.operation(), Operation::Control(operation));
        assert_eq!(usize::from(instruction.length()), bytes.len());
        assert!(prepared.is_current(&memory));
        let artifact = super::super::artifact::compile_runtime_entry(
            &memory,
            entry,
            limits,
            91,
            7,
            crate::cpu::x86::decode::CodeFetchContext::from_segments(
                &crate::cpu::x86::SegmentContext::default(),
            )
            .unwrap(),
            None,
        )
        .unwrap();
        assert!(artifact.contains_executable(entry.0));
        assert!(artifact.wasm_bytes(&memory).is_ok());
        memory.write(entry, &[0x90]).unwrap();
        assert_eq!(
            artifact.wasm_bytes(&memory),
            Err(ArtifactError::CodeInvalidated)
        );
    }
}

#[test]
fn runtime_entry_keeps_limits_fetch_and_valid_unsupported_errors_as_preparation_errors() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), &[0xcd, 0x80]).unwrap();
    let limits = CompileLimits::default();
    assert_eq!(
        prepare_runtime_entry(&memory, GuestAddress(0x1000), limits, None, None).unwrap_err(),
        prepare_embedded_entry_region(&memory, &[GuestAddress(0x1000)], limits, &[], None, None)
            .unwrap_err()
    );
    memory.write(GuestAddress(0x1fff), &[0x0f]).unwrap();
    assert_eq!(
        prepare_runtime_entry(&memory, GuestAddress(0x1fff), limits, None, None).unwrap_err(),
        prepare_embedded_entry_region(&memory, &[GuestAddress(0x1fff)], limits, &[], None, None)
            .unwrap_err()
    );
    assert_eq!(
        prepare_runtime_entry(
            &memory,
            GuestAddress(0x1000),
            CompileLimits {
                instructions: 0,
                ..limits
            },
            None,
            None
        )
        .unwrap_err(),
        CompileError::InvalidLimits
    );
    memory
        .write(GuestAddress(0x1000), &[0x90, 0xf0, 0x90])
        .unwrap();
    let prefix = prepare_runtime_entry(
        &memory,
        GuestAddress(0x1000),
        CompileLimits {
            instructions: 1,
            ..limits
        },
        None,
        None,
    )
    .unwrap();
    assert_eq!(prefix.instruction_count(), 1);
    assert_eq!(prefix.blocks[0].instructions[0].length(), 1);
}

#[test]
fn wrapped_translated_prefix_capture_preserves_consumed_and_source_free_currentness() {
    use crate::cpu::x86::{SegmentCache, SegmentContext};
    for (limit, consumed, stale_page) in [
        (0x2000, 3, Some(0_u32)),
        (0x1fff, 2, Some(0xfffff000)),
        (0x1ffd, 0, None),
    ] {
        let mut memory = AddressSpace::new(3).unwrap();
        for page in [0, 0x3000, 0xfffff000] {
            memory
                .map_zeroed(
                    PageRange::new(GuestAddress(page), 1).unwrap(),
                    Permissions::ALL,
                )
                .unwrap();
        }
        memory
            .write(GuestAddress(0xfffffffe), &[0x66, 0x66])
            .unwrap();
        memory.write(GuestAddress(0), &[0x66]).unwrap();
        let mut segments = SegmentContext::default();
        segments.state.bases[1] = 0xffffe000;
        segments.caches[1] = SegmentCache::from_bits(0xf2b00000 | limit).unwrap();
        let cs = CodeFetchContext::from_segments(&segments).unwrap();
        let prepared = prepare_runtime_entry(
            &memory,
            GuestAddress(0x1ffe),
            CompileLimits::default(),
            Some(cs),
            None,
        )
        .unwrap();
        let fault = prepared.blocks[0].context_fault().unwrap();
        assert_eq!(
            (fault.attempted_length, fault.consumed_prefix_length),
            (consumed + 1, consumed)
        );
        assert_eq!(prepared.instruction_count(), 1);
        assert!(prepared.is_current(&memory));
        memory.write(GuestAddress(0x3000), &[0x90]).unwrap();
        assert!(prepared.is_current(&memory));
        if consumed < 3 {
            memory.write(GuestAddress(0), &[0x66]).unwrap();
            assert!(
                prepared.is_current(&memory),
                "unfetched low linear page is not captured"
            );
        }
        if let Some(page) = stale_page {
            memory
                .write(
                    GuestAddress(if page == 0 { 0 } else { 0xfffffffe }),
                    &[0x66],
                )
                .unwrap();
            assert!(!prepared.is_current(&memory));
        } else {
            memory.write(GuestAddress(0xfffffffe), &[0x66]).unwrap();
            assert!(
                prepared.is_current(&memory),
                "source-free CS fault has no physical snapshot"
            );
        }
    }
}

#[test]
fn wrapped_block_snapshot_counts_instructions_and_stops_at_quota() {
    use crate::cpu::x86::SegmentContext;
    let mut memory = AddressSpace::new(2).unwrap();
    for page in [0, 0xfffff000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(page), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    memory
        .write(GuestAddress(0xfffffffd), &[0xb8, 0x78, 0x56])
        .unwrap();
    memory
        .write(GuestAddress(0), &[0x34, 0x12, 0xeb, 0xfe])
        .unwrap();
    let mut context = SegmentContext::default();
    context.state.bases[1] = 0xffffe000;
    let cs = CodeFetchContext::from_segments(&context).unwrap();
    let mut limits = CompileLimits {
        instructions: 1,
        ..CompileLimits::default()
    };
    let prefix = prepare_embedded_entry_region(
        &memory,
        &[GuestAddress(0x1ffd)],
        limits,
        &[],
        Some(cs),
        None,
    )
    .unwrap();
    assert_eq!(prefix.instruction_count(), 1);
    assert_eq!(prefix.blocks[0].instructions[0].length(), 5);
    assert!(prefix.is_current(&memory));
    limits.instructions = 2;
    let prepared = prepare_embedded_entry_region(
        &memory,
        &[GuestAddress(0x1ffd)],
        limits,
        &[],
        Some(cs),
        None,
    )
    .unwrap();
    assert_eq!(prepared.instruction_count(), 2);
    assert!(prepared.is_current(&memory));
    memory.write(GuestAddress(0), &[0x34]).unwrap();
    assert!(!prepared.is_current(&memory));
}
