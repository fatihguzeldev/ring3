use super::*;
use crate::{
    cpu::x86::{SegmentContext, decode::DecodeAuthority},
    memory::{PageRange, Permissions},
};
fn fixture(code: &[u8]) -> (AddressSpace, ResidentRegistry, CodeFetchContext) {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), code).unwrap();
    let registry = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
    (
        memory,
        registry,
        CodeFetchContext::from_segments(&SegmentContext::default()).unwrap(),
    )
}
fn cached(
    registry: &mut ResidentRegistry,
    memory: &AddressSpace,
    pc: u32,
    cs: CodeFetchContext,
) -> UnitId {
    registry
        .compile_cached_bound(
            memory,
            GuestAddress(pc),
            None,
            CompileLimits::default(),
            7,
            cs,
            DecodeAuthority::default(),
            super::super::CompilationTier32::Baseline,
            &|_| false,
        )
        .unwrap()
}
#[test]
fn automatic_cache_prefix_joins_current_tail_without_duplicating_or_invalidating_it() {
    let (memory, mut registry, cs) = fixture(&[0x08, 0xcd, 0x90, 0x90, 0xc3]);
    let tail = cached(&mut registry, &memory, 0x1002, cs);
    let tail_bytes = registry
        .get(&memory, tail)
        .unwrap()
        .wasm_bytes(&memory)
        .unwrap()
        .to_vec();
    let prefix = cached(&mut registry, &memory, 0x1000, cs);
    let prefix_region = registry.get(&memory, prefix).unwrap();
    assert_eq!(prefix_region.metadata().instructions, 1);
    assert!(prefix_region.contains_instruction(0x1000));
    assert!(!prefix_region.contains_executable(0x1002));
    assert_eq!(
        registry.lookup_bound(&memory, GuestAddress(0x1002), cs),
        Ok(tail)
    );
    assert_eq!(
        registry
            .get(&memory, tail)
            .unwrap()
            .wasm_bytes(&memory)
            .unwrap(),
        tail_bytes
    );
    assert_eq!(registry.usage().units, 2);
    let before = registry.usage();
    assert_eq!(
        registry.compile_bound(
            &memory,
            &[BlockSpec {
                entry: GuestAddress(0x1000),
                byte_length: 5
            }],
            &[],
            CompileLimits::default(),
            7,
            Some(cs),
            DecodeAuthority::default()
        ),
        Err(RegistryError::InstructionOverlap {
            pc: GuestAddress(0x1000)
        })
    );
    assert_eq!(registry.usage(), before);
}
#[test]
fn joins_are_instruction_boundaries_and_stale_tail_code_does_not_truncate_discovery() {
    let (memory, mut registry, cs) = fixture(&[0xb8, 0x90, 0x90, 0x90, 0x90, 0xc3]);
    let tail = cached(&mut registry, &memory, 0x1002, cs);
    let prefix = cached(&mut registry, &memory, 0x1000, cs);
    assert_eq!(
        registry
            .get(&memory, prefix)
            .unwrap()
            .metadata()
            .instructions,
        1
    );
    assert_eq!(
        registry.lookup_bound(&memory, GuestAddress(0x1002), cs),
        Ok(tail)
    );
    let (mut second, mut registry, cs) = fixture(&[0x90, 0x90, 0x90, 0xc3]);
    let old = cached(&mut registry, &second, 0x1001, cs);
    second.write(GuestAddress(0x1001), &[0x40]).unwrap();
    let new = cached(&mut registry, &second, 0x1000, cs);
    assert_eq!(
        registry.get(&second, old).err(),
        Some(RegistryError::CodeInvalidated)
    );
    assert_eq!(
        registry.get(&second, new).unwrap().metadata().instructions,
        4
    );
    assert_eq!(
        registry.lookup_bound(&second, GuestAddress(0x1001), cs),
        Ok(new)
    );
}
