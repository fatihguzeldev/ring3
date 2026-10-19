use super::*;
use crate::cpu::{
    dbt::{cold::prepare_runtime_entry, resident::EmissionPolicy},
    x86::SegmentCache,
};

fn limited_context(limit: u32) -> CodeFetchContext {
    let mut segments = SegmentContext::default();
    segments.caches[1] = SegmentCache::from_bits(0xf2b0_0000 | limit).unwrap();
    CodeFetchContext::from_segments(&segments).unwrap()
}

fn prepare(memory: &AddressSpace, pc: GuestAddress) -> PreparedRegion {
    prepare_runtime_entry(
        memory,
        pc,
        CompileLimits::default(),
        Some(context()),
        DecodeAuthority::default(),
    )
    .unwrap()
}

#[test]
fn bound_current_material_precedes_older_live_denial_at_the_same_pc() {
    let (mut memory, mut registry) = fixture();
    let page = PageRange::new(GuestAddress(0x1000), 1).unwrap();
    memory.protect(page, Permissions::READ_WRITE).unwrap();
    let pc = GuestAddress(0x1000);
    let denied = registry
        .compile_runtime_bound(
            &memory,
            pc,
            CompileLimits::default(),
            7,
            context(),
            DecodeAuthority::default(),
        )
        .unwrap();
    memory.protect(page, Permissions::ALL).unwrap();
    assert_eq!(
        registry.compile(
            &memory,
            &[BlockSpec {
                entry: pc,
                byte_length: 3,
            }],
            CompileLimits::default(),
        ),
        Err(RegistryError::InstructionOverlap { pc })
    );
    assert_eq!(registry.entry_index.candidates(pc.0), &[denied]);
    let material = registry
        .compile_runtime_bound(
            &memory,
            pc,
            CompileLimits::default(),
            7,
            context(),
            DecodeAuthority::default(),
        )
        .unwrap();
    assert_eq!(registry.entry_index.candidates(pc.0), &[denied, material]);
    let limited = limited_context(pc.0 - 1);
    assert_eq!(limited.decode_key(), context().decode_key());
    assert!(
        registry
            .get_raw(&memory, denied.get())
            .unwrap()
            .context_fault(pc.0)
            .unwrap()
            .remains_denied(&memory, limited)
    );
    assert_eq!(registry.lookup_bound(&memory, pc, limited), Ok(material));
    assert_eq!(registry.lookup_bound(&memory, pc, context()), Ok(material));

    memory.protect(page, Permissions::READ_WRITE).unwrap();
    assert_eq!(registry.lookup_bound(&memory, pc, context()), Ok(denied));
    registry
        .discard_quiescent_raw(&memory, denied.get())
        .unwrap();
    assert_eq!(
        registry.lookup_bound(&memory, pc, context()),
        Err(RegistryError::CodeInvalidated)
    );
    registry.retire_stale(&memory, material).unwrap();
    assert!(registry.entry_index.candidates(pc.0).is_empty());
    assert_eq!(
        registry.lookup_bound(&memory, pc, context()),
        Err(RegistryError::NotFound { pc })
    );
}

#[test]
fn one_final_region_may_retain_both_material_and_a_repaired_fault_start_at_one_pc() {
    for fault_first in [false, true] {
        let (mut memory, mut registry) = fixture();
        let page = PageRange::new(GuestAddress(0x1000), 1).unwrap();
        let pc = GuestAddress(0x1000);
        memory.protect(page, Permissions::READ_WRITE).unwrap();
        let mut fault = prepare(&memory, pc);
        memory.protect(page, Permissions::ALL).unwrap();
        let mut material = prepare(&memory, pc);
        // internal final-ir composition retains both meanings; index deduplication
        // must not let the repaired fault hide the material entry.
        let combined = if fault_first {
            fault.blocks.append(&mut material.blocks);
            fault
        } else {
            material.blocks.append(&mut fault.blocks);
            material
        };
        let id = registry
            .compile_prepared_bound(
                &memory,
                combined,
                EmissionPolicy::WholeUnit(CompileLimits::default()),
                7,
                Some(context()),
                &AtomicU64::new(0x1234_5678_9abc_def0),
            )
            .unwrap();
        assert_eq!(id.get(), 0x1234_5678_9abc_def0);
        assert_eq!(registry.entry_index.candidates(pc.0), &[id]);
        let region = registry.get_raw(&memory, id.get()).unwrap();
        assert!(region.contains_material(pc.0));
        assert!(
            !region
                .context_fault(pc.0)
                .unwrap()
                .remains_denied(&memory, context())
        );
        assert_eq!(registry.lookup_bound(&memory, pc, context()), Ok(id));
        assert_eq!(registry.lookup(&memory, pc), Ok(id));
        for hole in [0x1002, 0x1fff] {
            let pc = GuestAddress(hole);
            assert_eq!(
                registry.lookup_bound(&memory, pc, context()),
                Err(RegistryError::NotFound { pc })
            );
            assert_eq!(
                registry.lookup(&memory, pc),
                Err(RegistryError::NotFound { pc })
            );
        }
        registry.discard_quiescent_raw(&memory, id.get()).unwrap();
        assert!(registry.entry_index.candidates(pc.0).is_empty());
        assert!(registry.entry_index.candidates(pc.0 + 1).is_empty());
        assert_eq!(
            registry.get_raw(&memory, id.get()).unwrap_err(),
            RegistryError::InvalidUnit
        );
        let replacement = compile(&mut registry, &memory, pc.0);
        assert_eq!(registry.entry_index.candidates(pc.0), &[replacement]);
        assert_eq!(
            registry.lookup_bound(&memory, pc, context()),
            Ok(replacement)
        );
    }
}
