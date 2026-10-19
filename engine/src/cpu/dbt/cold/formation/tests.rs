use super::*;
use crate::{
    cpu::x86::{SegmentContext, decode::DecodeAuthority},
    memory::{PageRange, Permissions},
};

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
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
fn prepare(
    memory: &AddressSpace,
    entry: u32,
    limits: CompileLimits,
    joins: &dyn Fn(GuestAddress) -> bool,
) -> Result<PreparedRegion, CompileError> {
    prepare_runtime_region_until(
        memory,
        GuestAddress(entry),
        limits,
        Some(CodeFetchContext::from_segments(&SegmentContext::default()).unwrap()),
        DecodeAuthority::default(),
        joins,
    )
}

#[test]
fn direct_diamond_discovers_both_arms_and_shared_tail_once() {
    let mut memory = memory();
    diamond(&mut memory);
    let prepared = prepare(&memory, 0x1000, CompileLimits::default(), &|_| false).unwrap();
    assert_eq!(prepared.block_count(), 4);
    assert_eq!(prepared.instruction_count(), 9);
    assert_eq!(
        prepared
            .blocks
            .iter()
            .map(|block| block.instructions[0].pc().0)
            .collect::<Vec<_>>(),
        [0x1000, 0x1009, 0x1020, 0x1030]
    );
}

#[test]
fn discovered_regions_preserve_global_block_and_instruction_bounds() {
    let mut memory = memory();
    diamond(&mut memory);
    for (blocks, instructions) in [(1, 64), (2, 64), (8, 4), (3, 6)] {
        let prepared = prepare(
            &memory,
            0x1000,
            CompileLimits {
                blocks,
                instructions,
                ..CompileLimits::default()
            },
            &|_| false,
        )
        .unwrap();
        assert!(prepared.block_count() <= blocks);
        assert!(prepared.instruction_count() <= instructions);
        assert_eq!(
            prepared.blocks[0].instructions[0].pc(),
            GuestAddress(0x1000)
        );
    }
}

#[test]
fn loops_reuse_existing_instruction_starts_and_never_decode_interior_targets() {
    let mut memory = memory();
    memory
        .write(
            GuestAddress(0x1000),
            &[0xb9, 3, 0, 0, 0, 0x40, 0xe2, 0xfd, 0xc3],
        )
        .unwrap();
    let region = prepare(&memory, 0x1000, CompileLimits::default(), &|_| false).unwrap();
    assert_eq!((region.block_count(), region.instruction_count()), (2, 4));
    memory
        .write(
            GuestAddress(0x1000),
            &[0xb8, 0x90, 0x90, 0x90, 0x90, 0xeb, 0xfa],
        )
        .unwrap();
    let interior = prepare(&memory, 0x1000, CompileLimits::default(), &|_| false).unwrap();
    assert_eq!(
        (interior.block_count(), interior.instruction_count()),
        (1, 2)
    );
    assert!(!covers(&interior, GuestAddress(0x1007)));
}

#[test]
fn indirect_jumps_remain_region_boundaries() {
    let mut memory = memory();
    memory.write(GuestAddress(0x1000), &[0xff, 0xe0]).unwrap();
    memory.write(GuestAddress(0x1020), &[0x40, 0xc3]).unwrap();
    let region = prepare(&memory, 0x1000, CompileLimits::default(), &|_| false).unwrap();
    assert_eq!((region.block_count(), region.instruction_count()), (1, 1));
}

mod calls;

#[test]
fn external_unit_and_gate_joins_are_not_fetched_or_snapshotted() {
    let mut memory = memory();
    diamond(&mut memory);
    let region = prepare(&memory, 0x1000, CompileLimits::default(), &|pc| {
        [0x1020, 0x1030].contains(&pc.0)
    })
    .unwrap();
    assert_eq!((region.block_count(), region.instruction_count()), (2, 5));
    assert!(!covers(&region, GuestAddress(0x1020)));
    assert!(!covers(&region, GuestAddress(0x1030)));
}

#[test]
fn speculative_decode_errors_keep_a_valid_prefix_but_mandatory_errors_stay_visible() {
    let mut memory = memory();
    memory.write(GuestAddress(0x1000), &[0xeb, 0x0e]).unwrap();
    memory
        .write(GuestAddress(0x1010), &[0x90, 0x0f, 0x00, 0xc0])
        .unwrap();
    let region = prepare(&memory, 0x1000, CompileLimits::default(), &|_| false).unwrap();
    assert_eq!((region.block_count(), region.instruction_count()), (2, 2));
    assert!(covers(&region, GuestAddress(0x1010)));
    assert!(!covers(&region, GuestAddress(0x1011)));
    assert_eq!(
        prepare(&memory, 0x1010, CompileLimits::default(), &|_| false).unwrap_err(),
        super::super::prepare_runtime_entry_until(
            &memory,
            GuestAddress(0x1010),
            CompileLimits::default(),
            Some(CodeFetchContext::from_segments(&SegmentContext::default()).unwrap()),
            DecodeAuthority::default(),
            &|_| false
        )
        .unwrap_err(),
    );
    assert!(!speculative_boundary(CompileError::Allocation));
    assert!(!speculative_boundary(CompileError::Instruction {
        pc: GuestAddress(0x1010),
        cause: InstructionError::Decode(crate::cpu::x86::decode::DecodeError::Infrastructure(
            crate::memory::MemoryError::Allocation
        )),
    }));
}

#[test]
fn discovered_material_and_context_fault_prefixes_keep_snapshot_guards() {
    let mut memory = memory();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x2000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x1000), &[0xe9, 0xfb, 0x0f, 0, 0])
        .unwrap();
    memory
        .write(GuestAddress(0x2000), &[0xb8, 0x34, 0x12, 0, 0])
        .unwrap();
    let mut context = SegmentContext::default();
    context.caches[1] = crate::cpu::x86::SegmentCache::from_bits(0xf2b02001).unwrap();
    let region = prepare_runtime_region_until(
        &memory,
        GuestAddress(0x1000),
        CompileLimits::default(),
        Some(CodeFetchContext::from_segments(&context).unwrap()),
        DecodeAuthority::default(),
        &|_| false,
    )
    .unwrap();
    assert_eq!(region.block_count(), 2);
    assert_eq!(
        region.blocks[1]
            .context_fault()
            .unwrap()
            .consumed_prefix_length,
        2
    );
    assert!(region.is_current(&memory));
    memory.write(GuestAddress(0x2001), &[0x34]).unwrap();
    assert!(!region.is_current(&memory));
    let mut second = self::memory();
    diamond(&mut second);
    let region = prepare(&second, 0x1000, CompileLimits::default(), &|_| false).unwrap();
    assert!(region.is_current(&second));
    second.write(GuestAddress(0x1020), &[0x83]).unwrap();
    assert!(!region.is_current(&second));
}

#[test]
fn generated_regions_execute_real_wasm_arithmetic_branches_budgets_and_cancellation() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let mut diamond_memory = memory();
    diamond(&mut diamond_memory);
    diamond_memory
        .write(GuestAddress(0x1030), &[0x01, 0xd8, 0xe9, 0xc9, 7, 0, 0])
        .unwrap();
    let mut loop_memory = memory();
    loop_memory
        .write(
            GuestAddress(0x1000),
            &[0xb9, 3, 0, 0, 0, 0x40, 0xe2, 0xfd, 0xe9, 0xf3, 7, 0, 0],
        )
        .unwrap();
    let mut prefix_memory = memory();
    prefix_memory
        .write(GuestAddress(0x1000), &[0xeb, 0x0e])
        .unwrap();
    prefix_memory
        .write(GuestAddress(0x1010), &[0x90, 0x0f, 0x00, 0xc0])
        .unwrap();
    let mut modules = Vec::new();
    for memory in [&diamond_memory, &loop_memory, &prefix_memory] {
        let limits = CompileLimits::default();
        let prepared = prepare_runtime_region_until(
            memory,
            GuestAddress(0x1000),
            limits,
            None,
            DecodeAuthority::default(),
            &|pc| pc.0 == 0x1800,
        )
        .unwrap();
        let region = crate::cpu::dbt::artifact::emit_entry_prepared_tier(
            memory,
            prepared,
            limits,
            None,
            None,
            crate::cpu::dbt::CompilationTier32::Optimized,
        )
        .unwrap();
        let hex = region
            .wasm_bytes(memory)
            .unwrap()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        modules.push(format!("\"{hex}\""));
    }
    let mut child = Command::new("node")
        .args([
            "--input-type=module",
            "-e",
            include_str!("fixtures/run.mjs"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("node available for generated region proof");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("[{}]", modules.join(",")).as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("18 actual Wasm"));
}
