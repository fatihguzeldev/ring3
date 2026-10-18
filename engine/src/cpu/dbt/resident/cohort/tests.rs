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
