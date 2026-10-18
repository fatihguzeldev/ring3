use super::*;
use crate::{
    cpu::{
        dbt::{
            GateSpec,
            cold::{prepare_embedded_entry_region, prepare_runtime_entry},
        },
        x86::{SegmentContext, decode::DecodeAuthority},
    },
    memory::{GuestAddress, PageRange, Permissions},
};

fn cs() -> CodeFetchContext {
    CodeFetchContext::from_segments(&SegmentContext::default()).unwrap()
}

#[test]
fn nonmonotonic_search_keeps_only_a_measured_fitting_candidate() {
    let sizes = [100, 5, 100, 100, 100, 5, 100, 5];
    let mut emitted = Vec::new();
    let fitting = search(7, 5, |position| {
        emitted.push(position);
        Ok(Emission {
            bytes: vec![position as u8; sizes[position]],
            tier: CompilationTier32::Baseline,
        })
    })
    .unwrap()
    .unwrap();
    assert!(emitted.contains(&fitting.position));
    assert_eq!(fitting.emission.bytes, vec![fitting.position as u8; 5]);
    assert!(emitted.len() <= 4);
    assert_eq!(
        sizes[1], 5,
        "a larger fitting island can be skipped without claiming maximality"
    );
}

#[test]
fn successful_minimum_bounds_probes_without_assuming_global_monotonicity() {
    for limit in 1usize..=64 {
        for first_fitting in 1..=limit {
            let mut calls = 0;
            let fitting = search(limit, 5, |position| {
                calls += 1;
                Ok(Emission {
                    bytes: vec![position as u8; if position >= first_fitting { 5 } else { 100 }],
                    tier: CompilationTier32::Baseline,
                })
            })
            .unwrap()
            .unwrap();
            assert_eq!(fitting.position, first_fitting);
            assert_eq!(fitting.emission.bytes, vec![first_fitting as u8; 5]);
            assert!(calls <= 1 + limit.ilog2() as usize + usize::from(!limit.is_power_of_two()));
            assert!(calls + 1 <= 8, "including the initial oversized emission");
        }
    }
}

#[test]
fn failed_minimum_requests_exhaustive_fallback_instead_of_claiming_wasm_limit() {
    let sizes = [100, 5, 100, 100];
    let mut emitted = Vec::new();
    let fitting = search(3, 5, |position| {
        emitted.push(position);
        Ok(Emission {
            bytes: vec![0; sizes[position]],
            tier: CompilationTier32::Baseline,
        })
    })
    .unwrap();
    assert!(fitting.is_none());
    assert_eq!(emitted, [3]);
    assert_eq!(
        sizes[1], 5,
        "the original linear fallback must still test this fitting island"
    );
    let error = search(3, 5, |position| {
        if position == 3 {
            Ok(Emission {
                bytes: vec![0; 5],
                tier: CompilationTier32::Baseline,
            })
        } else {
            Err(CompileError::Allocation)
        }
    });
    assert!(matches!(error, Err(CompileError::Allocation)));
}

fn mixed() -> (AddressSpace, PreparedRegion, CodeFetchContext) {
    let mut memory = AddressSpace::new(3).unwrap();
    for page in [0x1000, 0x3000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(page), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    memory.write(GuestAddress(0x1ffe), &[0x90, 0xb8]).unwrap();
    memory
        .write(GuestAddress(0x3000), &[0x90, 0x90, 0xff, 0xe2])
        .unwrap();
    memory.write(GuestAddress(0x3100), &[0x0f, 0x0b]).unwrap();
    let cs = cs();
    let mut prepared = prepare_embedded_entry_region(
        &memory,
        &[GuestAddress(0x3100), GuestAddress(0x3000)],
        CompileLimits::default(),
        &[GateSpec {
            entry: GuestAddress(0x3100),
            id: 17,
        }],
        Some(cs),
        DecodeAuthority::default(),
    )
    .unwrap();
    for entry in [GuestAddress(0x1ffe), GuestAddress(0x2000)] {
        let mut terminal = prepare_runtime_entry(
            &memory,
            entry,
            CompileLimits::default(),
            Some(cs),
            DecodeAuthority::default(),
        )
        .unwrap();
        prepared.blocks.append(&mut terminal.blocks);
    }
    (memory, prepared, cs)
}

#[test]
fn trim_journal_restores_exact_fault_ownership_and_preserves_all_requested_entries() {
    let (memory, mut prepared, cs) = mixed();
    let binding = Some(wasm::EmbeddedBinding::Replacement {
        key: 17,
        generation: 1,
    });
    let original = emit_bytes(&prepared, binding, CompilationTier32::Baseline).bytes;
    let capacities = prepared
        .blocks
        .iter()
        .map(|block| block.instructions.capacity())
        .collect::<Vec<_>>();
    let source_pointer = match &prepared.blocks[2].source {
        BlockSource::ContextFault {
            consumed: Some(snapshot),
            ..
        } => snapshot.versions.as_ptr(),
        _ => panic!("authored consumed-prefix fault"),
    };
    let mut journal = TrimJournal::new(&prepared).unwrap();
    assert_eq!(journal.limit, 3);
    journal.seek(&memory, &mut prepared, Some(cs), 1).unwrap();
    assert!(
        prepared.blocks[2].context_fault().is_none(),
        "remove trailing fault before material instructions"
    );
    assert_eq!(prepared.blocks[1].instructions.len(), 3);
    for position in [3, 2, 0, 1, 0] {
        journal
            .seek(&memory, &mut prepared, Some(cs), position)
            .unwrap();
    }
    assert_eq!(
        emit_bytes(&prepared, binding, CompilationTier32::Baseline).bytes,
        original
    );
    assert_eq!(
        prepared
            .blocks
            .iter()
            .map(|block| block.instructions.capacity())
            .collect::<Vec<_>>(),
        capacities
    );
    let restored_pointer = match &prepared.blocks[2].source {
        BlockSource::ContextFault {
            consumed: Some(snapshot),
            ..
        } => snapshot.versions.as_ptr(),
        _ => panic!("original fault source must return"),
    };
    assert_eq!(
        restored_pointer, source_pointer,
        "restore owned source rather than copying snapshot identities"
    );
    journal.seek(&memory, &mut prepared, Some(cs), 3).unwrap();
    journal.finalize(&memory, &mut prepared, Some(cs)).unwrap();
    assert_eq!(prepared.block_count(), 4);
    assert_eq!(prepared.instruction_count(), 4);
    assert_eq!(prepared.blocks[0].gate().unwrap().id, 17);
    assert_eq!(
        prepared.blocks[1].instructions[0].pc(),
        GuestAddress(0x3000)
    );
    assert_eq!(
        prepared.blocks[2].instructions[0].pc(),
        GuestAddress(0x1ffe)
    );
    assert_eq!(
        prepared.blocks[3].context_fault().unwrap().pc,
        GuestAddress(0x2000)
    );
    assert!(
        prepared.blocks[3].instructions.is_empty(),
        "fault-only requested entry is never removed"
    );
    assert!(prepared.is_current(&memory));
}

#[test]
fn selected_artifact_has_exact_retained_page_snapshot_and_actual_applied_tier() {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 2).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    let mut code = vec![0x90; 63];
    code.extend_from_slice(&[0xff, 0xe2]);
    memory.write(GuestAddress(0x1fff), &code).unwrap();
    let cs = cs();
    let binding = Some(wasm::EmbeddedBinding::Replacement {
        key: 19,
        generation: 1,
    });
    let prepare = |limits| {
        prepare_runtime_entry(
            &memory,
            GuestAddress(0x1fff),
            limits,
            Some(cs),
            DecodeAuthority::default(),
        )
        .unwrap()
    };
    let minimum = prepare(CompileLimits {
        instructions: 1,
        ..CompileLimits::default()
    });
    let minimum_bytes = emit_bytes(&minimum, binding, CompilationTier32::Optimized).bytes;
    let limits = CompileLimits {
        wasm_bytes: minimum_bytes.len(),
        ..CompileLimits::default()
    };
    let full = prepare(limits);
    assert_eq!(full.instruction_count(), 64);
    assert!(
        emit_bytes(&full, binding, CompilationTier32::Optimized)
            .bytes
            .len()
            > limits.wasm_bytes
    );
    let artifact = emit(
        &memory,
        full,
        limits,
        binding,
        Some(cs),
        CompilationTier32::Optimized,
    )
    .unwrap();
    assert_eq!(artifact.metadata().instructions, 1);
    assert_eq!(artifact.tier(), CompilationTier32::Baseline);
    assert_eq!(artifact.wasm_bytes(&memory).unwrap(), minimum_bytes);
    assert!(artifact.contains_material(0x1fff));
    assert!(!artifact.contains_executable(0x2000));
    memory.write(GuestAddress(0x2000), &[0x90]).unwrap();
    assert!(
        artifact.wasm_bytes(&memory).is_ok(),
        "trial superset snapshot cannot escape publication"
    );
    memory.write(GuestAddress(0x1fff), &[0x90]).unwrap();
    assert!(artifact.wasm_bytes(&memory).is_err());
}

#[test]
fn impossible_quota_preserves_all_gate_and_fault_only_entries_until_failure() {
    let (memory, mut prepared, cs) = mixed();
    let binding = Some(wasm::EmbeddedBinding::Replacement {
        key: 23,
        generation: 1,
    });
    let mut journal = TrimJournal::new(&prepared).unwrap();
    journal
        .seek(&memory, &mut prepared, Some(cs), journal.limit)
        .unwrap();
    journal.finalize(&memory, &mut prepared, Some(cs)).unwrap();
    assert_eq!(prepared.instruction_count(), 4);
    let mut journal = TrimJournal::new(&prepared).unwrap();
    assert_eq!(journal.limit, 0);
    journal.seek(&memory, &mut prepared, Some(cs), 0).unwrap();
    assert!(matches!(
        emit(
            &memory,
            prepared,
            CompileLimits {
                wasm_bytes: 1,
                ..CompileLimits::default()
            },
            binding,
            Some(cs),
            CompilationTier32::Optimized
        ),
        Err(CompileError::WasmLimit)
    ));
    let (memory, full, cs) = mixed();
    assert!(
        matches!(
            emit(
                &memory,
                full,
                CompileLimits {
                    wasm_bytes: 1,
                    ..CompileLimits::default()
                },
                binding,
                Some(cs),
                CompilationTier32::Optimized,
            ),
            Err(CompileError::WasmLimit)
        ),
        "a failed searched minimum restores all owned sources before the exhaustive fallback"
    );
}
