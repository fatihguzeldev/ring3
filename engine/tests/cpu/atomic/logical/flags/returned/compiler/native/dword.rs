use ring3_engine::{
    abi::arena::{ARENA_SIZE, TRANSFER_OFFSET, X87_OFFSET},
    cpu::{
        dbt::{CompileError, CompileLimits, InstructionError},
        x86::{
            Register32,
            decode::decode_one,
            ir::{Condition, EffectiveAddress, Operation},
        },
    },
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError},
};

const CODE: u32 = 0x2000;
const KEY: u64 = 0x5245_5455_524e_4341;
const ORIGINAL: &[u8; 71] = include_bytes!(
    "../../../../../../../programs/atomic/flags/fixtures/p2-returned-atomic-flags32/kernel.text.bin"
);
const STARTS: [u32; 33] = [
    0, 1, 2, 3, 7, 11, 15, 19, 21, 23, 25, 29, 31, 33, 35, 37, 39, 43, 45, 47, 49, 51, 53, 57, 59,
    61, 63, 65, 67, 68, 69, 70, 71,
];
const BLOCKS: [(u32, u32, usize); 7] = [
    (0, 21, 8),
    (21, 10, 4),
    (31, 4, 2),
    (35, 10, 4),
    (45, 4, 2),
    (49, 10, 4),
    (59, 12, 8),
];

#[test]
fn whole_original_returned_atomic_flags_admits_seven_blocks_and_thirty_two_instructions_in_four_bound_profiles()
 {
    assert_eq!(
        ORIGINAL.as_slice(),
        &[
            0x53, 0x57, 0x56, 0x8b, 0x4c, 0x24, 0x1c, 0x8b, 0x74, 0x24, 0x18, 0x8b, 0x7c, 0x24,
            0x14, 0x8b, 0x54, 0x24, 0x10, 0x8b, 0x02, 0x89, 0xc3, 0x09, 0xfb, 0xf0, 0x0f, 0xb1,
            0x1a, 0x75, 0xf6, 0x89, 0xc7, 0x8b, 0x02, 0x89, 0xc3, 0x21, 0xf3, 0xf0, 0x0f, 0xb1,
            0x1a, 0x75, 0xf6, 0x89, 0xc6, 0x8b, 0x02, 0x89, 0xc3, 0x31, 0xcb, 0xf0, 0x0f, 0xb1,
            0x1a, 0x75, 0xf6, 0x89, 0xf1, 0x01, 0xf9, 0x01, 0xc1, 0x89, 0xc8, 0x5e, 0x5f, 0x5b,
            0xc3,
        ]
    );
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65536)
    );
    assert_eq!(BLOCKS.len(), 7);
    assert_eq!(BLOCKS.iter().map(|block| block.2).sum::<usize>(), 32);
    assert_eq!(STARTS.len() - 1, 32);
    assert_eq!(
        BLOCKS.iter().map(|block| block.1).sum::<u32>(),
        ORIGINAL.len() as u32
    );
    for (index, &(offset, length, instructions)) in BLOCKS.iter().enumerate() {
        assert_eq!(
            STARTS
                .iter()
                .filter(|&&pc| pc >= offset && pc < offset + length)
                .count(),
            instructions
        );
        assert_eq!(
            offset + length,
            BLOCKS.get(index + 1).map_or(71, |block| block.0)
        );
    }
    let address = EffectiveAddress {
        base: Some(Register32::Edx),
        index: None,
        scale: 1,
        displacement: 0,
        ..Default::default()
    };
    let mut failures = Vec::new();
    for (ordinal, (resident, entries)) in
        [(false, false), (false, true), (true, false), (true, true)]
            .into_iter()
            .enumerate()
    {
        let mut engine = source(KEY + ordinal as u64);
        let request: Vec<u32> = BLOCKS
            .iter()
            .flat_map(|&(offset, length, _)| {
                if entries {
                    vec![CODE + offset]
                } else {
                    vec![CODE + offset, length]
                }
            })
            .collect();
        for (index, word) in request.into_iter().enumerate() {
            let at = TRANSFER_OFFSET + index * 4;
            engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&word.to_le_bytes());
        }
        let before = engine.arena().to_vec();
        let mut cas = 0;
        let mut backedges = 0;
        for pair in STARTS.windows(2) {
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE + pair[0]))
                .unwrap_or_else(|error| {
                    panic!(
                        "original +{:x} resident={resident} entries={entries}: {error:?}",
                        pair[0]
                    )
                });
            assert_eq!(u32::from(decoded.length()), pair[1] - pair[0]);
            assert_eq!(decoded.next_pc(), GuestAddress(CODE + pair[1]));
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .is_code_current(decoded.code_snapshot())
            );
            if matches!(pair[0], 25 | 39 | 53) {
                assert_eq!(
                    decoded.operation(),
                    &Operation::LockedMemoryCompareExchange32 {
                        address,
                        source: Register32::Ebx,
                    }
                );
                cas += 1;
            } else {
                assert!(!matches!(
                    decoded.operation(),
                    Operation::LockedMemoryCompareExchange32 { .. }
                ));
            }
            if let Some(target) = match pair[0] {
                29 => Some(21),
                43 => Some(35),
                57 => Some(49),
                _ => None,
            } {
                assert_eq!(
                    decoded.operation(),
                    &Operation::ConditionalJump {
                        condition: Condition::NotEqual,
                        target: GuestAddress(CODE + target),
                    }
                );
                assert!(BLOCKS.iter().any(|block| block.0 == target));
                assert!(BLOCKS.iter().any(|block| block.0 == pair[1]));
                backedges += 1;
            }
        }
        assert_eq!((cas, backedges), (3, 3));
        let result = match (resident, entries) {
            (false, false) => engine.compile(7).map(u64::from),
            (false, true) => engine.compile_entries(7, 0).map(u64::from),
            (true, false) => engine.compile_resident(7).map(|id| id.get()),
            (true, true) => engine.compile_resident_entries(7, 0).map(|id| id.get()),
        };
        match result {
            Ok(id) => {
                let bytes = if resident {
                    engine.resident_bytes(id).unwrap()
                } else {
                    engine.artifact_bytes().unwrap()
                };
                assert_eq!(&bytes[..8], b"\0asm\x01\0\0\0");
                assert!(bytes.len() <= limits.wasm_bytes);
            }
            Err(error) => failures.push(format!(
                "whole original resident={resident} entries={entries}: {error:?}"
            )),
        }
        assert_eq!(engine.arena(), before);
        assert_unmapped_operands(&engine);
    }
    let mut incorrect = source(KEY + 4);
    incorrect.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[CODE.to_le_bytes(), (ORIGINAL.len() as u32).to_le_bytes()].concat());
    let before = incorrect.arena().to_vec();
    assert_eq!(
        incorrect.compile(1),
        Err(HostError::Compile(CompileError::Instruction {
            pc: GuestAddress(CODE + 29),
            cause: InstructionError::InvalidBlockEnd,
        }))
    );
    assert_eq!(incorrect.generation(), 0);
    assert_eq!(incorrect.arena(), before);
    assert_unmapped_operands(&incorrect);
    assert!(
        failures.is_empty(),
        "complete returned-old consumer must admit in all four existing bound profiles: {failures:?}"
    );
}

fn assert_unmapped_operands(engine: &EngineInstance) {
    let memory = engine.memory().unwrap();
    assert_eq!(memory.mapped_pages(), 1);
    assert!(memory.resolve(GuestAddress(CODE), Access::Read).is_err());
    for address in [0, 0x3000, 0xa100_1234, 0xa104_1238] {
        assert!(memory.resolve(GuestAddress(address), Access::Read).is_err());
    }
}

fn source(key: u64) -> EngineInstance {
    let mut engine = EngineInstance::new(1, key).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + ORIGINAL.len()]
        .copy_from_slice(ORIGINAL);
    engine.upload(CODE, ORIGINAL.len() as u32).unwrap();
    engine.protect(CODE, 1, 4).unwrap();
    let arena = engine.arena_mut().unwrap();
    assert_eq!(arena.len(), ARENA_SIZE);
    assert_eq!(ARENA_SIZE, 4612);
    for index in 0..8 {
        let at = 16 + index * 4;
        let value = 0xa100_1234_u32.wrapping_add(index as u32 * 0x0001_0001);
        arena[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    arena[48..52].copy_from_slice(&0xdead_beef_u32.to_le_bytes());
    arena[52..56].copy_from_slice(&0x0cd7_u32.to_le_bytes());
    arena[X87_OFFSET + 16..X87_OFFSET + 18].copy_from_slice(&0x027f_u16.to_le_bytes());
    arena[X87_OFFSET + 18..X87_OFFSET + 20].copy_from_slice(&0x81a5_u16.to_le_bytes());
    for (index, byte) in arena[X87_OFFSET + 40..X87_OFFSET + 120]
        .iter_mut()
        .enumerate()
    {
        *byte = (index as u8).wrapping_mul(29).wrapping_add(17);
    }
    engine
}
