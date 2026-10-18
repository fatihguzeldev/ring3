use super::*;
use crate::{
    cpu::x86::{SegmentContext, decode::DecodeAuthority},
    memory::{PageRange, Permissions},
};

use super::super::CompilationTier32;

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
fn automatic_cache_prefix_stops_before_a_host_owned_gate() {
    let (memory, mut registry, cs) = fixture(&[0x90, 0x0f, 0x0b]);
    let id = registry
        .compile_cached_bound(
            &memory,
            GuestAddress(0x1000),
            None,
            CompileLimits::default(),
            7,
            cs,
            DecodeAuthority::default(),
            super::super::CompilationTier32::Baseline,
            &|pc| pc == GuestAddress(0x1001),
        )
        .unwrap();
    let region = registry.get(&memory, id).unwrap();
    assert_eq!(region.metadata().instructions, 1);
    assert!(!region.contains_executable(0x1001));
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

fn diamond(memory: &mut AddressSpace) {
    for (pc, code) in [
        (0x1000, &[0xb8, 1, 0, 0, 0, 0x85, 0xc9, 0x74, 0x17][..]),
        (0x1009, &[0x83, 0xc0, 2, 0xeb, 0x22][..]),
        (0x1020, &[0x83, 0xc0, 4, 0xeb, 0x0b][..]),
        (0x1030, &[0x01, 0xd8, 0xc3][..]),
    ] {
        memory.write(GuestAddress(pc), code).unwrap();
    }
}

fn optimized(
    registry: &mut ResidentRegistry,
    memory: &AddressSpace,
    pc: u32,
    cs: CodeFetchContext,
    limits: CompileLimits,
    is_gate: &dyn Fn(GuestAddress) -> bool,
) -> Result<UnitId, RegistryError> {
    registry.compile_cached_bound(
        memory,
        GuestAddress(pc),
        None,
        limits,
        7,
        cs,
        DecodeAuthority::default(),
        CompilationTier32::Optimized,
        is_gate,
    )
}

#[test]
fn optimized_cache_formation_discovers_cfg_while_baseline_keeps_one_block() {
    let (mut memory, mut baseline, cs) = fixture(&[]);
    diamond(&mut memory);
    let baseline_id = cached(&mut baseline, &memory, 0x1000, cs);
    let baseline_region = baseline.get(&memory, baseline_id).unwrap();
    assert_eq!(baseline_region.tier(), CompilationTier32::Baseline);
    assert_eq!(
        (
            baseline_region.metadata().blocks,
            baseline_region.metadata().instructions,
        ),
        (1, 3)
    );
    assert!(!baseline_region.contains_executable(0x1009));
    assert!(!baseline_region.contains_executable(0x1020));

    let mut registry = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
    let id = optimized(
        &mut registry,
        &memory,
        0x1000,
        cs,
        CompileLimits::default(),
        &|_| false,
    )
    .unwrap();
    let region = registry.get(&memory, id).unwrap();
    assert_eq!(region.tier(), CompilationTier32::Optimized);
    assert_eq!(
        (region.metadata().blocks, region.metadata().instructions),
        (4, 9)
    );
    for pc in [
        0x1000, 0x1005, 0x1007, 0x1009, 0x100c, 0x1020, 0x1023, 0x1030, 0x1032,
    ] {
        assert_eq!(registry.lookup_bound(&memory, GuestAddress(pc), cs), Ok(id));
    }
    assert_eq!(registry.usage().units, 1);
}

#[test]
fn optimized_minimal_cfg_falls_back_to_mandatory_block_when_wasm_quota_cannot_fit_it() {
    let (mut memory, mut baseline, cs) = fixture(&[]);
    for index in 0..8 {
        let pc = GuestAddress(0x1000 + index * 16);
        memory
            .write(pc, &[0xeb, if index == 7 { 0xfe } else { 0x0e }])
            .unwrap();
    }
    let mandatory = cached(&mut baseline, &memory, 0x1000, cs);
    let mandatory_bytes = baseline
        .get(&memory, mandatory)
        .unwrap()
        .wasm_bytes(&memory)
        .unwrap()
        .len();
    let quota = mandatory_bytes + 64;

    let mut full = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
    let full_id = optimized(
        &mut full,
        &memory,
        0x1000,
        cs,
        CompileLimits::default(),
        &|_| false,
    )
    .unwrap();
    let full_region = full.get(&memory, full_id).unwrap();
    assert_eq!(
        (
            full_region.metadata().blocks,
            full_region.metadata().instructions
        ),
        (8, 8)
    );
    assert!(full_region.wasm_bytes(&memory).unwrap().len() > quota);

    let mut limited = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
    let fallback = optimized(
        &mut limited,
        &memory,
        0x1000,
        cs,
        CompileLimits {
            wasm_bytes: quota,
            ..CompileLimits::default()
        },
        &|_| false,
    )
    .unwrap();
    let region = limited.get(&memory, fallback).unwrap();
    assert_eq!(
        (region.metadata().blocks, region.metadata().instructions),
        (1, 1)
    );
    assert!(region.wasm_bytes(&memory).unwrap().len() <= quota);
    assert_eq!(region.tier(), CompilationTier32::Baseline);
    assert_eq!(
        limited.lookup_bound(&memory, GuestAddress(0x1000), cs),
        Ok(fallback)
    );
    assert_eq!(
        limited.lookup_bound(&memory, GuestAddress(0x1010), cs),
        Err(RegistryError::NotFound {
            pc: GuestAddress(0x1010)
        })
    );
    assert_eq!(limited.usage().units, 1);

    let mut too_small = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
    assert_eq!(
        optimized(
            &mut too_small,
            &memory,
            0x1000,
            cs,
            CompileLimits {
                wasm_bytes: 1,
                ..CompileLimits::default()
            },
            &|_| false,
        ),
        Err(RegistryError::Compile(CompileError::WasmLimit))
    );
    assert_eq!(
        too_small.usage(),
        RegistryUsage {
            units: 0,
            wasm_bytes: 0
        }
    );
}

#[test]
fn optimized_candidates_join_host_gates_without_decoding_gate_bytes() {
    let (mut memory, mut registry, cs) = fixture(&[]);
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x2000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .write(
            GuestAddress(0x1000),
            &[0xb8, 1, 0, 0, 0, 0x85, 0xc9, 0x0f, 0x84, 0xf3, 0x0f, 0, 0],
        )
        .unwrap();
    memory
        .write(
            GuestAddress(0x100d),
            &[0x83, 0xc0, 2, 0xe9, 0xeb, 0x10, 0, 0],
        )
        .unwrap();
    for pc in [0x2000, 0x2100] {
        memory.write(GuestAddress(pc), &[0x0f, 0x0b]).unwrap();
    }
    let id = optimized(
        &mut registry,
        &memory,
        0x1000,
        cs,
        CompileLimits::default(),
        &|pc| [GuestAddress(0x2000), GuestAddress(0x2100)].contains(&pc),
    )
    .unwrap();
    let region = registry.get(&memory, id).unwrap();
    assert_eq!(
        (region.metadata().blocks, region.metadata().instructions),
        (2, 5)
    );
    assert!(!region.contains_executable(0x2000));
    assert!(!region.contains_executable(0x2100));
    memory.write(GuestAddress(0x2000), &[0xcc]).unwrap();
    memory.write(GuestAddress(0x2100), &[0xcc]).unwrap();
    assert!(registry.get(&memory, id).is_ok());
}

#[test]
fn optimized_cfg_keeps_current_tail_and_repairs_only_invalidated_region() {
    let (mut memory, mut registry, cs) = fixture(&[]);
    diamond(&mut memory);
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x2000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x100c), &[0xe9, 0xef, 0x0f, 0, 0])
        .unwrap();
    memory
        .write(GuestAddress(0x1023), &[0xe9, 0xd8, 0x0f, 0, 0])
        .unwrap();
    memory
        .write(GuestAddress(0x2000), &[0x01, 0xd8, 0xc3])
        .unwrap();
    let tail = cached(&mut registry, &memory, 0x2000, cs);
    let tail_bytes = registry
        .get(&memory, tail)
        .unwrap()
        .wasm_bytes(&memory)
        .unwrap();
    let tail_pointer = tail_bytes.as_ptr();
    let tail_bytes = tail_bytes.to_vec();
    let prefix = optimized(
        &mut registry,
        &memory,
        0x1000,
        cs,
        CompileLimits::default(),
        &|_| false,
    )
    .unwrap();
    let region = registry.get(&memory, prefix).unwrap();
    assert_eq!(
        (region.metadata().blocks, region.metadata().instructions),
        (3, 7)
    );
    assert!(!region.contains_executable(0x2000));
    assert_eq!(
        registry.lookup_bound(&memory, GuestAddress(0x2000), cs),
        Ok(tail)
    );
    assert_eq!(
        registry.retire_stale(&memory, tail),
        Err(RegistryError::CurrentUnit)
    );
    let usage = registry.usage();
    assert_eq!(
        optimized(
            &mut registry,
            &memory,
            0x1000,
            cs,
            CompileLimits::default(),
            &|_| false
        ),
        Err(RegistryError::InstructionOverlap {
            pc: GuestAddress(0x1000)
        })
    );
    assert_eq!(registry.usage(), usage);

    memory.write(GuestAddress(0x1022), &[6]).unwrap();
    assert_eq!(
        registry.get(&memory, prefix).err(),
        Some(RegistryError::CodeInvalidated)
    );
    registry.retire_stale(&memory, prefix).unwrap();
    let replacement = optimized(
        &mut registry,
        &memory,
        0x1000,
        cs,
        CompileLimits::default(),
        &|_| false,
    )
    .unwrap();
    assert_ne!(replacement, prefix);
    assert_eq!(
        registry.lookup_bound(&memory, GuestAddress(0x1020), cs),
        Ok(replacement)
    );
    assert_eq!(
        registry.lookup_bound(&memory, GuestAddress(0x2000), cs),
        Ok(tail)
    );
    let retained_tail = registry
        .get(&memory, tail)
        .unwrap()
        .wasm_bytes(&memory)
        .unwrap();
    assert_eq!(retained_tail, tail_bytes);
    assert_eq!(retained_tail.as_ptr(), tail_pointer);
    assert_eq!(registry.usage().units, 2);
}
