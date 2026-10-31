use super::*;
use crate::memory::{PageRange, Permissions};

fn page(address: u32) -> PageRange {
    PageRange::new(GuestAddress(address), 1).unwrap()
}

fn memory(pages: &[u32]) -> AddressSpace {
    let mut memory = AddressSpace::new(3).unwrap();
    for &address in pages {
        memory.map_zeroed(page(address), Permissions::ALL).unwrap();
    }
    memory
}

#[test]
fn covered_interior_excludes_numeric_operations_and_their_source_page() {
    let mut memory = memory(&[0x1000, 0x2000]);
    memory.write(GuestAddress(0x1fff), &[0x90]).unwrap();
    memory
        .write(GuestAddress(0x2000), &[0xd9, 0xe8, 0xc3])
        .unwrap();
    let code = compile_with_gates_until(&memory, GuestAddress(0x1fff), 64, &[], |pc| {
        pc == GuestAddress(0x2000)
    })
    .unwrap();
    assert_eq!(code.entries, [0x1fff]);
    assert_eq!(code.instruction_count, 1);
    assert_eq!(code.sources.len(), 1);
    assert!(code.services.is_empty());
    memory.write(GuestAddress(0x2000), &[0xcc]).unwrap();
    assert!(code.is_current(&memory));
    memory.write(GuestAddress(0x1fff), &[0x90]).unwrap();
    assert!(!code.is_current(&memory));
}

#[test]
fn covered_pending_callee_is_omitted_while_the_return_continuation_is_compiled() {
    let mut memory = memory(&[0x1000, 0x3000]);
    memory
        .write(GuestAddress(0x1000), &[0xe8, 0xfb, 0x1f, 0, 0, 0x90, 0xc3])
        .unwrap();
    memory
        .write(GuestAddress(0x3000), &[0xd9, 0xe8, 0xc3])
        .unwrap();
    let code = compile_with_gates_until(&memory, GuestAddress(0x1000), 64, &[], |pc| {
        pc == GuestAddress(0x3000)
    })
    .unwrap();
    assert_eq!(code.entries, [0x1000, 0x1005, 0x1006]);
    assert_eq!(code.instruction_count, 3);
    assert!(code.services.is_empty());
    memory.protect(page(0x3000), Permissions::READ).unwrap();
    assert!(code.is_current(&memory));
    memory.unmap(page(0x3000)).unwrap();
    memory.map_zeroed(page(0x3000), Permissions::ALL).unwrap();
    assert!(code.is_current(&memory));
    memory.write(GuestAddress(0x1005), &[0x90]).unwrap();
    assert!(!code.is_current(&memory));
}

#[test]
fn covered_interior_stops_before_gate_metadata_and_executable_fetch() {
    let mut memory = memory(&[0x1000]);
    memory.write(GuestAddress(0x1fff), &[0x90]).unwrap();
    // None of these uncovered gate errors belong to the retained prefix.
    for gates in [
        vec![(GuestAddress(0x2000), 17)],
        vec![(GuestAddress(0x2000), 0)],
        vec![(GuestAddress(0x2000), 17), (GuestAddress(0x2000), 18)],
    ] {
        let code = compile_with_gates_until(&memory, GuestAddress(0x1fff), 64, &gates, |pc| {
            pc == GuestAddress(0x2000)
        })
        .unwrap();
        assert_eq!(code.entries, [0x1fff]);
        assert_eq!(code.instruction_count, 1);
        assert!(code.services.is_empty());
        assert!(code.is_current(&memory));
        assert!(compile_with_gates(&memory, GuestAddress(0x1fff), 64, &gates).is_err());
    }
}

#[test]
fn covered_pending_target_is_not_validated_or_fetched() {
    let mut memory = memory(&[0x1000]);
    memory
        .write(GuestAddress(0x1000), &[0xe9, 0xfb, 0x1f, 0, 0])
        .unwrap();
    let gates = [(GuestAddress(0x3000), 0)];
    let code = compile_with_gates_until(&memory, GuestAddress(0x1000), 64, &gates, |pc| {
        pc == GuestAddress(0x3000)
    })
    .unwrap();
    assert_eq!(code.entries, [0x1000]);
    assert_eq!(code.instruction_count, 1);
    assert_eq!(code.sources.len(), 1);
    assert!(compile_with_gates(&memory, GuestAddress(0x1000), 64, &gates).is_err());
}

#[test]
fn covered_wrapped_successor_is_not_part_of_the_prefix_snapshot() {
    let mut memory = memory(&[0xfffff000, 0]);
    memory.write(GuestAddress(u32::MAX), &[0x90]).unwrap();
    memory.write(GuestAddress(0), &[0xc3]).unwrap();
    let code = compile_with_gates_until(&memory, GuestAddress(u32::MAX), 64, &[], |pc| {
        pc == GuestAddress(0)
    })
    .unwrap();
    assert_eq!(code.entries, [u32::MAX]);
    assert_eq!(code.instruction_count, 1);
    memory.write(GuestAddress(0), &[0x90]).unwrap();
    assert!(code.is_current(&memory));
    memory.write(GuestAddress(u32::MAX), &[0x90]).unwrap();
    assert!(!code.is_current(&memory));
}

#[test]
fn no_coverage_retains_the_public_compiler_and_first_fetch_fault_contract() {
    let mut memory = memory(&[0x1000, 0x3000]);
    memory
        .write(GuestAddress(0x1000), &[0xe8, 0xfb, 0x1f, 0, 0, 0xc3])
        .unwrap();
    memory.write(GuestAddress(0x3000), &[0x0f, 0x0b]).unwrap();
    let gates = [(GuestAddress(0x3000), 17)];
    let public = compile_with_gates(&memory, GuestAddress(0x1000), 64, &gates).unwrap();
    let private =
        compile_with_gates_until(&memory, GuestAddress(0x1000), 64, &gates, |_| false).unwrap();
    assert_eq!(private.wasm, public.wasm);
    assert_eq!(private.entries, public.entries);
    assert_eq!(private.services, public.services);
    assert_eq!(private.instruction_count, public.instruction_count);
    assert_eq!(private.sources.len(), public.sources.len());
    for result in [
        compile(&memory, GuestAddress(0x2000), 64),
        compile_with_gates_until(&memory, GuestAddress(0x2000), 64, &[], |_| false),
    ] {
        assert!(matches!(
            result,
            Err(CompileError::Decode(DecodeError::Paging {
                pc: GuestAddress(0x2000),
                ..
            }))
        ));
    }
}

#[test]
fn already_covered_root_is_rejected_before_decoding_or_creating_an_empty_unit() {
    let memory = memory(&[]);
    assert!(matches!(
        compile_with_gates_until(&memory, GuestAddress(0x2000), 64, &[], |_| true),
        Err(CompileError::InvalidLimits)
    ));
}
