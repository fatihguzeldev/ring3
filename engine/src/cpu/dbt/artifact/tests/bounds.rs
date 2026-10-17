use super::*;
use crate::{
    cpu::x86::{SegmentContext, decode::DecodeAuthority},
    memory::{PageRange, Permissions},
};
fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(4).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 2).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
}
fn assert_exact_bounds(region: &CompiledRegion) {
    let addresses = region
        .executable_addresses()
        .map(|entry| entry.0)
        .collect::<Vec<_>>();
    assert_eq!(region.entry_start, *addresses.iter().min().unwrap());
    assert_eq!(region.entry_end, *addresses.iter().max().unwrap());
    for pc in addresses {
        assert!(region.may_contain_entry(pc));
        assert!(region.contains_executable(pc));
    }
}
#[test]
fn unordered_material_blocks_reject_outside_bounds_but_keep_exact_hole_checks() {
    let mut memory = memory();
    for pc in [0x1000, 0x2000] {
        memory.write(GuestAddress(pc), &[0x90, 0xeb, 0]).unwrap();
    }
    let region = compile_region(
        &memory,
        &[0x2000, 0x1000].map(|pc| BlockSpec {
            entry: GuestAddress(pc),
            byte_length: 3,
        }),
        CompileLimits::default(),
    )
    .unwrap();
    assert_exact_bounds(&region);
    assert_eq!((region.entry_start, region.entry_end), (0x1000, 0x2001));
    for pc in [0, 0xfff, 0x1002, 0x1800, 0x2002, u32::MAX] {
        assert!(!region.contains_material(pc));
        assert!(!region.contains_executable(pc));
        assert!(region.context_fault(pc).is_none());
    }
    assert!(region.contains_material_in(0x1000, CodeDecodeKey::FLAT));
    assert!(!region.contains_material_in(
        0x1000,
        CodeDecodeKey {
            cs_base: 0x1000,
            default32: true
        }
    ));
}
#[test]
fn gate_only_unsigned_endpoints_keep_conservative_bounds_across_address_wrap() {
    let mut memory = AddressSpace::new(2).unwrap();
    for page in [0, 0xffff_f000] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(page), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    let entries = [GuestAddress(u32::MAX - 1), GuestAddress(0)];
    for entry in entries {
        memory.write(entry, &[0x0f, 0x0b]).unwrap();
    }
    let gates = [
        GateSpec {
            entry: entries[1],
            id: 17,
        },
        GateSpec {
            entry: entries[0],
            id: 18,
        },
    ];
    let prepared = super::super::region::prepare_resident_region(
        &memory,
        &entries.map(|entry| BlockSpec {
            entry,
            byte_length: 2,
        }),
        CompileLimits::default(),
        &gates,
        None,
        DecodeAuthority::default(),
    )
    .unwrap();
    let region = emit_prepared(
        prepared,
        CompileLimits::default(),
        Some(wasm::EmbeddedBinding::Resident { key: 7, id: 1 }),
    )
    .unwrap();
    assert_exact_bounds(&region);
    assert_eq!((region.entry_start, region.entry_end), (0, u32::MAX - 1));
    for entry in entries {
        assert!(region.contains_material(entry.0));
    }
    for pc in [1, 0x8000_0000, u32::MAX] {
        assert!(!region.contains_executable(pc));
    }
    assert!(region.matches_gate(0, 17));
    assert!(region.matches_gate(u32::MAX - 1, 18));
}
#[test]
fn captured_fault_bounds_include_consumed_material_and_source_free_denials() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1ffe), &[0x90, 0xb8]).unwrap();
    let cs = CodeFetchContext::from_segments(&SegmentContext::default()).unwrap();
    let region = compile_runtime_entry(
        &memory,
        GuestAddress(0x1ffe),
        CompileLimits::default(),
        7,
        1,
        cs,
        DecodeAuthority::default(),
    )
    .unwrap();
    assert_exact_bounds(&region);
    assert_eq!((region.entry_start, region.entry_end), (0x1ffe, 0x1fff));
    assert!(region.contains_material(0x1ffe));
    assert!(!region.contains_material(0x1fff));
    assert!(region.context_fault(0x1fff).is_some());
    assert!(region.context_fault(0x2000).is_none());
    let denied = compile_runtime_entry(
        &memory,
        GuestAddress(0x3000),
        CompileLimits::default(),
        7,
        2,
        cs,
        DecodeAuthority::default(),
    )
    .unwrap();
    assert_exact_bounds(&denied);
    assert_eq!((denied.entry_start, denied.entry_end), (0x3000, 0x3000));
    assert!(!denied.contains_material(0x3000));
    assert!(denied.context_fault(0x3000).is_some());
}
#[test]
fn emitted_prefix_recomputes_bounds_after_wasm_budget_shortening() {
    let mut memory = memory();
    let mut code = vec![0x90; 16];
    code.extend_from_slice(&[0xeb, 0xfe]);
    memory.write(GuestAddress(0x1000), &code).unwrap();
    let cs = CodeFetchContext::from_segments(&SegmentContext::default()).unwrap();
    let full = compile_runtime_entry(
        &memory,
        GuestAddress(0x1000),
        CompileLimits::default(),
        7,
        1,
        cs,
        DecodeAuthority::default(),
    )
    .unwrap();
    let shortened = compile_runtime_entry(
        &memory,
        GuestAddress(0x1000),
        CompileLimits {
            wasm_bytes: full.wasm_bytes(&memory).unwrap().len() - 1,
            ..CompileLimits::default()
        },
        7,
        1,
        cs,
        DecodeAuthority::default(),
    )
    .unwrap();
    assert_exact_bounds(&full);
    assert_exact_bounds(&shortened);
    assert!(shortened.metadata().instructions < full.metadata().instructions);
    assert!(shortened.entry_end < full.entry_end);
    assert!(!shortened.contains_executable(full.entry_end));
}
