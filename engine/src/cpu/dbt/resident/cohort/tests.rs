use super::*;
use crate::{
    cpu::{
        dbt::CompilationTier32,
        x86::{SegmentContext, decode::DecodeAuthority},
    },
    memory::{PageRange, Permissions},
};

fn fixture() -> (AddressSpace, ResidentRegistry, CodeFetchContext) {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    let registry = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
    let cs = CodeFetchContext::from_segments(&SegmentContext::default()).unwrap();
    (memory, registry, cs)
}
fn cached(
    registry: &mut ResidentRegistry,
    memory: &AddressSpace,
    entry: u32,
    cs: CodeFetchContext,
    tier: CompilationTier32,
    limits: CompileLimits,
    is_gate: &dyn Fn(GuestAddress) -> bool,
) -> UnitId {
    registry
        .compile_cached_bound(
            memory,
            GuestAddress(entry),
            None,
            limits,
            7,
            cs,
            DecodeAuthority::default(),
            tier,
            is_gate,
        )
        .unwrap()
}
fn plan(
    registry: &ResidentRegistry,
    memory: &AddressSpace,
    entry: u32,
    cs: CodeFetchContext,
) -> Option<ResidentCohort> {
    registry
        .plan_cached_cohort(
            memory,
            GuestAddress(entry),
            CompileLimits::default(),
            cs,
            DecodeAuthority::default(),
            &|_| true,
            &|_| false,
        )
        .unwrap()
}

#[test]
fn a_disconnected_side_entry_prevents_retiring_its_whole_owner() {
    let (mut memory, mut registry, cs) = fixture();
    for (pc, bytes) in [
        (0x1000, &[0x90, 0xeb, 0x1d][..]),
        (0x1020, &[0x90, 0xc3][..]),
        (0x1100, &[0x90, 0xc3][..]),
    ] {
        memory.write(GuestAddress(pc), bytes).unwrap();
    }
    let tail = registry
        .compile_entries_bound(
            &memory,
            &[GuestAddress(0x1020), GuestAddress(0x1100)],
            &[],
            CompileLimits::default(),
            7,
            Some(cs),
            DecodeAuthority::default(),
        )
        .unwrap();
    let root = cached(
        &mut registry,
        &memory,
        0x1000,
        cs,
        CompilationTier32::Baseline,
        CompileLimits::default(),
        &|_| false,
    );
    let old = registry
        .get(&memory, tail)
        .unwrap()
        .wasm_bytes(&memory)
        .unwrap();
    let pointer = old.as_ptr();
    let bytes = old.to_vec();
    assert!(plan(&registry, &memory, 0x1000, cs).is_none());
    assert_eq!(
        registry.lookup_bound(&memory, GuestAddress(0x1100), cs),
        Ok(tail)
    );
    assert_eq!(
        registry.lookup_bound(&memory, GuestAddress(0x1000), cs),
        Ok(root)
    );
    let current = registry
        .get(&memory, tail)
        .unwrap()
        .wasm_bytes(&memory)
        .unwrap();
    assert_eq!(current, bytes);
    assert_eq!(current.as_ptr(), pointer);
    assert_eq!(registry.usage().units, 2);
}

#[test]
fn final_byte_limit_fallback_does_not_satisfy_a_preplanned_whole_cohort() {
    let (mut memory, mut registry, cs) = fixture();
    for index in 0..8 {
        memory
            .write(
                GuestAddress(0x1000 + index * 16),
                &[0x90, 0xeb, if index == 7 { 0xfd } else { 0x0d }],
            )
            .unwrap();
    }
    let mut ids = Vec::new();
    for index in 0..8 {
        ids.push(cached(
            &mut registry,
            &memory,
            0x1000 + index * 16,
            cs,
            CompilationTier32::Baseline,
            CompileLimits::default(),
            &|_| false,
        ));
    }
    let coverage = plan(&registry, &memory, 0x1000, cs).unwrap();
    assert_eq!(coverage.owner_count, 8);
    let mut single = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
    let mandatory = cached(
        &mut single,
        &memory,
        0x1000,
        cs,
        CompilationTier32::Optimized,
        CompileLimits::default(),
        &|pc| pc == GuestAddress(0x1010),
    );
    let quota = single
        .get(&memory, mandatory)
        .unwrap()
        .wasm_bytes(&memory)
        .unwrap()
        .len()
        + 64;
    for id in ids {
        registry.discard_quiescent_raw(&memory, id.get()).unwrap();
    }
    let limited = cached(
        &mut registry,
        &memory,
        0x1000,
        cs,
        CompilationTier32::Optimized,
        CompileLimits {
            wasm_bytes: quota,
            ..CompileLimits::default()
        },
        &|_| false,
    );
    let limited = registry.get(&memory, limited).unwrap();
    assert_eq!(limited.metadata().blocks, 1);
    assert!(!coverage.covers(limited));
    assert!(limited.wasm_bytes(&memory).unwrap().len() <= quota);
}

#[test]
fn a_gate_owner_never_enters_the_instruction_only_cohort() {
    let (mut memory, mut registry, cs) = fixture();
    memory
        .write(GuestAddress(0x1000), &[0x90, 0xeb, 0x1d])
        .unwrap();
    memory.write(GuestAddress(0x1020), &[0x0f, 0x0b]).unwrap();
    let gate = registry
        .compile_entries_bound(
            &memory,
            &[GuestAddress(0x1020)],
            &[GateSpec {
                entry: GuestAddress(0x1020),
                id: 17,
            }],
            CompileLimits::default(),
            7,
            Some(cs),
            DecodeAuthority::default(),
        )
        .unwrap();
    cached(
        &mut registry,
        &memory,
        0x1000,
        cs,
        CompilationTier32::Baseline,
        CompileLimits::default(),
        &|_| false,
    );
    assert!(plan(&registry, &memory, 0x1000, cs).is_none());
    assert_eq!(
        registry.lookup_bound(&memory, GuestAddress(0x1020), cs),
        Ok(gate)
    );
}

#[test]
fn selected_larger_limit_captures_all_one_hundred_twenty_eight_owner_starts() {
    let (mut memory, mut registry, cs) = fixture();
    for (pc, target) in [(0x1000_u32, 0x1100_u32), (0x1100, 0x1000)] {
        let mut bytes = vec![0x90; 63];
        bytes.push(0xe9);
        bytes.extend_from_slice(&target.wrapping_sub(pc + 68).to_le_bytes());
        memory.write(GuestAddress(pc), &bytes).unwrap();
    }
    let mut owners = Vec::new();
    for entry in [0x1000, 0x1100] {
        let id = cached(
            &mut registry,
            &memory,
            entry,
            cs,
            CompilationTier32::Baseline,
            CompileLimits::default(),
            &|_| false,
        );
        assert_eq!(
            registry.get(&memory, id).unwrap().metadata().instructions,
            64
        );
        owners.push(id);
    }
    assert!(plan(&registry, &memory, 0x1000, cs).is_none());
    let usage = registry.usage();
    let limits = CompileLimits {
        instructions: 128,
        wasm_bytes: 131072,
        ..CompileLimits::default()
    };
    let coverage = registry
        .plan_cached_cohort(
            &memory,
            GuestAddress(0x1000),
            limits,
            cs,
            DecodeAuthority::default(),
            &|_| true,
            &|_| false,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        COHORT_INSTRUCTIONS,
        super::super::super::region::MAX_INSTRUCTIONS
    );
    assert_eq!(coverage.owner_count, 2);
    assert_eq!(coverage.address_count, 128);
    assert_eq!(
        coverage.owners(),
        owners.iter().map(|id| id.get()).collect::<Vec<_>>()
    );
    for (entry, owner) in [0x1000, 0x1100].into_iter().zip(&owners) {
        for offset in 0..64 {
            assert!(coverage.addresses[..coverage.address_count].contains(&(entry + offset)));
            assert_eq!(
                registry.lookup_bound(&memory, GuestAddress(entry + offset), cs),
                Ok(*owner)
            );
        }
    }
    assert_eq!(registry.usage(), usage);
    assert!(
        registry
            .plan_cached_cohort(
                &memory,
                GuestAddress(0x1000),
                CompileLimits {
                    instructions: 127,
                    ..limits
                },
                cs,
                DecodeAuthority::default(),
                &|_| true,
                &|_| false,
            )
            .unwrap()
            .is_none()
    );
    for owner in owners {
        registry
            .discard_quiescent_raw(&memory, owner.get())
            .unwrap();
    }
    let fused = cached(
        &mut registry,
        &memory,
        0x1000,
        cs,
        CompilationTier32::Optimized,
        limits,
        &|_| false,
    );
    let fused = registry.get(&memory, fused).unwrap();
    assert_eq!(fused.metadata().instructions, 128);
    assert!(coverage.covers(fused));
}

#[test]
fn maximum_unit_stages_and_publishes_all_256_whole_owner_starts() {
    let (mut memory, mut registry, cs) = fixture();
    let entries = [0x1000_u32, 0x1100, 0x1200, 0x1300];
    for (index, pc) in entries.into_iter().enumerate() {
        let target = entries[(index + 1) % entries.len()];
        let mut bytes = vec![0x90; 63];
        bytes.push(0xe9);
        bytes.extend_from_slice(&target.wrapping_sub(pc + 68).to_le_bytes());
        memory.write(GuestAddress(pc), &bytes).unwrap();
    }
    let owners = entries.map(|entry| {
        cached(
            &mut registry,
            &memory,
            entry,
            cs,
            CompilationTier32::Baseline,
            CompileLimits::default(),
            &|_| false,
        )
    });
    let usage = registry.usage();
    let limits = CompileLimits {
        instructions: 256,
        wasm_bytes: 262144,
        ..CompileLimits::default()
    };
    let candidate = registry
        .prepare_cached_replacement(
            &memory,
            GuestAddress(0x1000),
            limits,
            7,
            cs,
            DecodeAuthority::default(),
            &|_| true,
            &|_| false,
        )
        .unwrap()
        .unwrap();
    let coverage = candidate.coverage();
    assert_eq!(COHORT_INSTRUCTIONS, 256);
    assert_eq!(coverage.owner_count, 4);
    assert_eq!(coverage.address_count, 256);
    assert_eq!(coverage.owners(), owners.map(UnitId::get));
    for (entry, owner) in entries.into_iter().zip(owners) {
        for offset in 0..64 {
            assert!(coverage.addresses[..256].contains(&(entry + offset)));
            assert_eq!(
                registry.lookup_bound(&memory, GuestAddress(entry + offset), cs),
                Ok(owner)
            );
        }
    }
    assert_eq!(registry.usage(), usage);
    for owner in owners {
        registry
            .discard_quiescent_raw(&memory, owner.get())
            .unwrap();
    }
    let fused = registry
        .publish_cached_replacement(&memory, cs, candidate, &|_| false)
        .unwrap();
    let region = registry.get(&memory, fused).unwrap();
    assert_eq!(region.metadata().instructions, 256);
    assert_eq!(region.metadata().blocks, 4);
    assert_eq!(region.tier(), CompilationTier32::Optimized);
    assert!(coverage.covers(region));
    assert!(region.wasm_bytes(&memory).unwrap().len() <= 262144);
    assert_eq!(registry.usage().units, 1);
    for entry in entries {
        for offset in 0..64 {
            assert_eq!(
                registry.lookup_bound(&memory, GuestAddress(entry + offset), cs),
                Ok(fused)
            );
        }
    }
}

#[test]
fn larger_unit_keeps_the_existing_nonindexed_span_baseline_fallback() {
    for instructions in [128, 256] {
        let (mut memory, mut registry, cs) = fixture();
        let mut bytes = [0x66, 0x0f, 0x1f, 0x84, 0, 0, 0, 0, 0].repeat(instructions - 1);
        bytes.extend_from_slice(&[0xff, 0xe2]);
        assert!(bytes.len() > 1024);
        memory.write(GuestAddress(0x1000), &bytes).unwrap();
        let wasm_bytes = instructions * 1024;
        let id = cached(
            &mut registry,
            &memory,
            0x1000,
            cs,
            CompilationTier32::Optimized,
            CompileLimits {
                instructions,
                wasm_bytes,
                ..CompileLimits::default()
            },
            &|_| false,
        );
        let region = registry.get(&memory, id).unwrap();
        assert_eq!(region.metadata().instructions, instructions);
        assert_eq!(region.tier(), CompilationTier32::Baseline);
        assert!(!region.supports_indexed_entry());
        assert_eq!(region.executable_addresses().count(), instructions);
        assert!(region.wasm_bytes(&memory).unwrap().len() <= wasm_bytes);
    }
}
