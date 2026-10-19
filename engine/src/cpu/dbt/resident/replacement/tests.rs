use super::*;
use crate::{
    cpu::x86::{SegmentContext, decode::DecodeAuthority},
    memory::{PageRange, Permissions},
};

#[test]
fn prospective_quota_subtracts_exact_victims_before_publishing_the_owned_candidate() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    for (pc, code) in [
        (0x1000, &[0x90, 0xeb, 0x1d][..]),
        (0x1020, &[0x90, 0xeb, 0xfd][..]),
    ] {
        memory.write(GuestAddress(pc), code).unwrap();
    }
    let mut registry = ResidentRegistry::new(
        &memory,
        RegistryLimits {
            units: 2,
            wasm_bytes: 131072,
        },
    )
    .unwrap();
    let cs = CodeFetchContext::from_segments(&SegmentContext::default()).unwrap();
    for (pc, tier) in [
        (0x1000, super::super::super::CompilationTier32::Baseline),
        (0x1020, super::super::super::CompilationTier32::Optimized),
    ] {
        registry
            .compile_cached_bound(
                &memory,
                GuestAddress(pc),
                None,
                CompileLimits::default(),
                7,
                cs,
                DecodeAuthority::default(),
                tier,
                &|_| false,
            )
            .unwrap();
    }
    let prepare = |registry: &mut ResidentRegistry| {
        registry
            .prepare_cached_replacement(
                &memory,
                GuestAddress(0x1000),
                CompileLimits::default(),
                7,
                cs,
                DecodeAuthority::default(),
                &|_| true,
                &|_| false,
            )
            .unwrap()
            .unwrap()
    };
    let first = prepare(&mut registry);
    let usage = registry.usage();
    let bytes = first.region.retained_wasm_byte_len();
    registry.limits.wasm_bytes = usage.wasm_bytes.max(bytes) + 32;
    assert!(usage.wasm_bytes + bytes > registry.limits.wasm_bytes);
    drop(first);
    let candidate = prepare(&mut registry);
    let bytes = candidate.region.retained_wasm_byte_len();
    assert_eq!(registry.usage(), usage);
    assert_eq!(
        registry.get_raw(&memory, candidate.id.get()).unwrap_err(),
        RegistryError::InvalidUnit
    );
    assert_eq!(
        registry.validate_cached_replacement(&memory, cs, &candidate, &|pc| pc.0 == 0x1001),
        Err(RegistryError::InstructionOverlap {
            pc: GuestAddress(0x1001)
        })
    );
    assert_eq!(registry.usage(), usage);
    for &id in candidate.coverage.owners() {
        registry.discard_quiescent_raw(&memory, id).unwrap();
    }
    assert!(registry.entry_index.candidates(0x1000).is_empty());
    assert!(registry.entry_index.candidates(0x1020).is_empty());
    let id = registry
        .publish_cached_replacement(&memory, cs, candidate, &|_| false)
        .unwrap();
    assert_eq!(
        registry.usage(),
        RegistryUsage {
            units: 1,
            wasm_bytes: bytes
        }
    );
    for pc in [0x1000, 0x1001, 0x1020, 0x1021] {
        assert_eq!(registry.lookup_bound(&memory, GuestAddress(pc), cs), Ok(id));
    }
}

#[test]
fn a_new_live_gate_at_a_prepared_context_fault_start_prevents_retirement() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x1000), &[0x90, 0xe9, 0xfa, 0x0f, 0, 0])
        .unwrap();
    let mut registry = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
    let cs = CodeFetchContext::from_segments(&SegmentContext::default()).unwrap();
    let root = registry
        .compile_cached_bound(
            &memory,
            GuestAddress(0x1000),
            None,
            CompileLimits::default(),
            7,
            cs,
            DecodeAuthority::default(),
            super::super::super::CompilationTier32::Baseline,
            &|_| false,
        )
        .unwrap();
    let usage = registry.usage();
    let candidate = registry
        .prepare_cached_replacement(
            &memory,
            GuestAddress(0x1000),
            CompileLimits::default(),
            7,
            cs,
            DecodeAuthority::default(),
            &|_| true,
            &|_| false,
        )
        .unwrap()
        .unwrap();
    assert!(candidate.region.context_fault(0x2000).is_some());
    assert_eq!(
        registry.validate_cached_replacement(&memory, cs, &candidate, &|pc| pc.0 == 0x2000),
        Err(RegistryError::InstructionOverlap {
            pc: GuestAddress(0x2000)
        })
    );
    assert_eq!(registry.usage(), usage);
    assert_eq!(
        registry.lookup_bound(&memory, GuestAddress(0x1000), cs),
        Ok(root)
    );
}
