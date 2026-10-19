use super::*;

fn call_loop() -> AddressSpace {
    let mut memory = memory();
    for (pc, code) in [
        (0x1000, &[0x8d, 0x7f, 1, 0xe8, 0xf8, 0, 0, 0][..]),
        (0x1008, &[0x49, 0x75, 0xf5, 0xe9, 0xf0, 7, 0, 0][..]),
        (0x1100, &[0x8b, 0x46, 4, 0xc3][..]),
    ] {
        memory.write(GuestAddress(pc), code).unwrap();
    }
    memory
}

fn starts(region: &PreparedRegion) -> Vec<u32> {
    region
        .blocks
        .iter()
        .map(|block| block.instructions[0].pc().0)
        .collect()
}

#[test]
fn direct_call_discovers_callee_and_return_continuation() {
    let region = prepare(&call_loop(), 0x1000, CompileLimits::default(), &|pc| {
        pc.0 == 0x1800
    })
    .unwrap();
    assert_eq!(starts(&region), [0x1000, 0x1100, 0x1008, 0x100b]);
    assert_eq!(region.instruction_count(), 7);
    for pc in [0x1000, 0x1003, 0x1008, 0x1009, 0x100b, 0x1100, 0x1103] {
        assert!(covers(&region, GuestAddress(pc)));
    }
    assert!(!covers(&region, GuestAddress(0x1800)));
}

#[test]
fn direct_calls_keep_selected_global_block_and_instruction_bounds() {
    for (blocks, instructions) in [(1, 128), (2, 128), (3, 128), (8, 2), (8, 3), (8, 6)] {
        let region = prepare(
            &call_loop(),
            0x1000,
            CompileLimits {
                blocks,
                instructions,
                ..CompileLimits::default()
            },
            &|pc| pc.0 == 0x1800,
        )
        .unwrap();
        assert!(region.block_count() <= blocks);
        assert!(region.instruction_count() <= instructions);
        assert_eq!(starts(&region)[0], 0x1000);
    }
}

#[test]
fn recursive_and_nonreturning_callees_do_not_expand_the_frontier_forever() {
    for (pc, bytes) in [
        (0x1003, &[0xe8, 0xf8, 0xff, 0xff, 0xff][..]),
        (0x1100, &[0xeb, 0xfe][..]),
    ] {
        let mut memory = call_loop();
        memory.write(GuestAddress(pc), bytes).unwrap();
        let region = prepare(&memory, 0x1000, CompileLimits::default(), &|pc| {
            pc.0 == 0x1800
        })
        .unwrap();
        let entries = starts(&region);
        let mut unique = entries.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(entries.len(), unique.len());
        assert!(entries.contains(&0x1008));
        assert!(region.block_count() <= CompileLimits::default().blocks);
        assert!(region.instruction_count() <= CompileLimits::default().instructions);
    }
}

#[test]
fn callee_and_continuation_joins_are_not_fetched_or_snapshotted() {
    for join in [0x1100, 0x1008] {
        let memory = call_loop();
        let region = prepare(&memory, 0x1000, CompileLimits::default(), &|pc| {
            pc.0 == join || pc.0 == 0x1800
        })
        .unwrap();
        assert!(!covers(&region, GuestAddress(join)));
        assert!(covers(
            &region,
            GuestAddress(if join == 0x1100 { 0x1008 } else { 0x1100 })
        ));
    }
}

#[test]
fn discovered_callee_and_continuation_bytes_remain_currentness_dependencies() {
    for changed in [0x1100, 0x1008] {
        let mut memory = call_loop();
        let region = prepare(&memory, 0x1000, CompileLimits::default(), &|pc| {
            pc.0 == 0x1800
        })
        .unwrap();
        assert!(region.is_current(&memory));
        memory.write(GuestAddress(changed), &[0x90]).unwrap();
        assert!(!region.is_current(&memory));
    }
}

#[test]
fn indirect_word_calls_and_returns_remain_unchased_boundaries() {
    for code in [&[0xff, 0xd0][..], &[0x66, 0xe8, 0x1c, 0][..], &[0xc3][..]] {
        let mut memory = memory();
        memory.write(GuestAddress(0x1000), code).unwrap();
        memory.write(GuestAddress(0x1020), &[0x40, 0xc3]).unwrap();
        let region = prepare(&memory, 0x1000, CompileLimits::default(), &|_| false).unwrap();
        assert_eq!((region.block_count(), region.instruction_count()), (1, 1));
    }
}
