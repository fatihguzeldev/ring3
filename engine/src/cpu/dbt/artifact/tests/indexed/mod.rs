use super::*;
use crate::memory::{PageRange, Permissions};

mod currentness;
mod shortening;

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 2).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
}

fn spec(pc: u32, length: u32) -> BlockSpec {
    BlockSpec {
        entry: GuestAddress(pc),
        byte_length: length,
    }
}

fn region(memory: &AddressSpace, specs: &[BlockSpec], instructions: usize) -> CompiledRegion {
    let prepared = prepare_region(
        memory,
        specs,
        CompileLimits {
            instructions,
            ..CompileLimits::default()
        },
    )
    .unwrap();
    CompiledRegion::new(
        prepared,
        Vec::new(),
        crate::cpu::dbt::CompilationTier32::Baseline,
        None,
    )
    .unwrap()
}

#[test]
fn indexed_eligibility_uses_each_final_block_without_joining_sparse_entries() {
    let mut memory = memory();
    for pc in [0x1000, 0x1100] {
        memory.write(GuestAddress(pc), &[0x90; 2]).unwrap();
    }
    assert!(!region(&memory, &[spec(0x1000, 1)], 2).supports_indexed_entry());
    assert!(!region(&memory, &[spec(0x1000, 1), spec(0x1100, 1)], 2).supports_indexed_entry());
    assert!(region(&memory, &[spec(0x1000, 2)], 2).supports_indexed_entry());
    assert!(region(&memory, &[spec(0x1000, 1), spec(0x1100, 2)], 3).supports_indexed_entry());

    let mut code = [0x0f, 0x1f, 0x40, 0].repeat(255);
    code.extend_from_slice(&[0x0f, 0x1f, 0x40, 0]);
    memory.write(GuestAddress(0x1000), &code).unwrap();
    assert!(region(&memory, &[spec(0x1000, 1024)], 256).supports_indexed_entry());
    code.truncate(1020);
    code.extend_from_slice(&[0x0f, 0x1f, 0x44, 0, 0]);
    memory.write(GuestAddress(0x1000), &code).unwrap();
    assert!(!region(&memory, &[spec(0x1000, 1025)], 256).supports_indexed_entry());
}
