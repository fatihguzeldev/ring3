use ring3_engine::{
    abi::arena::{ARENA_SIZE, TRANSFER_OFFSET, X87_OFFSET},
    cpu::{
        UnsupportedFeature,
        dbt::CompileLimits,
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{BitScanKind, BranchTarget, Condition, Operation},
        },
    },
    memory::{Access, GuestAddress},
    process::EngineInstance,
};

const CODE: u32 = 0x2000;
const KEY: u64 = 0x4249_544d_4150_4354;
const ORIGINAL: &[u8; 40] = include_bytes!("../../fixtures/p2-bitmap-dispatch32/kernel.text.bin");
const STARTS: [u32; 19] = [
    0, 1, 2, 6, 8, 12, 14, 16, 20, 22, 24, 26, 28, 30, 32, 37, 38, 39, 40,
];
const BLOCKS: [(u32, u32, usize); 4] = [(0, 16, 7), (16, 16, 7), (32, 5, 1), (37, 3, 3)];

#[test]
fn original_bitmap_dispatch_admits_observed_count_and_complete_function_in_four_bound_profiles() {
    assert_eq!(
        ORIGINAL.as_slice(),
        &[
            0x57, 0x56, 0x8b, 0x4c, 0x24, 0x0c, 0x8b, 0x11, 0x8b, 0x74, 0x24, 0x10, 0x21, 0xd6,
            0x74, 0x10, 0xf3, 0x0f, 0xbc, 0xc6, 0x89, 0xf7, 0xf7, 0xdf, 0x21, 0xf7, 0x31, 0xd7,
            0x89, 0x39, 0xeb, 0x05, 0xb8, 0x20, 0, 0, 0, 0x5e, 0x5f, 0xc3,
        ]
    );
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65536)
    );
    assert_eq!(BLOCKS.iter().map(|block| block.2).sum::<usize>(), 18);
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
            BLOCKS.get(index + 1).map_or(40, |block| block.0)
        );
    }
    let mut failures = Vec::new();
    let exact = source(KEY, ORIGINAL);
    let before = exact.arena().to_vec();
    let mut neighbors = 0;
    for pair in STARTS.windows(2) {
        let result = decode_one(exact.memory().unwrap(), GuestAddress(CODE + pair[0]));
        if pair[0] == 16 {
            match result {
                Ok(decoded) => {
                    assert_eq!(decoded.length(), 4);
                    assert_eq!(decoded.next_pc(), GuestAddress(CODE + 20));
                    assert!(
                        exact
                            .memory()
                            .unwrap()
                            .is_code_current(decoded.code_snapshot())
                    );
                }
                Err(error) => failures.push(format!(
                    "exact original +16 at {:08x}: {error:?}",
                    CODE + 16
                )),
            }
        } else {
            let decoded = result
                .unwrap_or_else(|error| panic!("original neighbor +{:x}: {error:?}", pair[0]));
            assert_eq!(u32::from(decoded.length()), pair[1] - pair[0]);
            assert_eq!(decoded.next_pc(), GuestAddress(CODE + pair[1]));
            assert!(
                exact
                    .memory()
                    .unwrap()
                    .is_code_current(decoded.code_snapshot())
            );
            if pair[0] == 14 {
                assert_eq!(
                    decoded.operation(),
                    &Operation::ConditionalJump {
                        condition: Condition::Equal,
                        target: GuestAddress(CODE + 32),
                    }
                );
            }
            if pair[0] == 30 {
                assert_eq!(
                    decoded.operation(),
                    &Operation::Jump {
                        target: BranchTarget::Direct(GuestAddress(CODE + 37)),
                    }
                );
            }
            neighbors += 1;
        }
    }
    assert_eq!(neighbors, 17);
    assert_eq!(exact.arena(), before);
    assert_unmapped_operands(&exact);
    for (ordinal, (resident, entries)) in
        [(false, false), (false, true), (true, false), (true, true)]
            .into_iter()
            .enumerate()
    {
        let mut engine = source(KEY + 1 + ordinal as u64, ORIGINAL);
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
        let result = match (resident, entries) {
            (false, false) => engine.compile(4).map(u64::from),
            (false, true) => engine.compile_entries(4, 0).map(u64::from),
            (true, false) => engine.compile_resident(4).map(|id| id.get()),
            (true, true) => engine.compile_resident_entries(4, 0).map(|id| id.get()),
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
            Err(error) => {
                failures.push(format!(
                    "whole original resident={resident} entries={entries}: {error:?}"
                ));
                assert_eq!(engine.generation(), 0);
            }
        }
        assert_eq!(engine.arena(), before);
        assert_unmapped_operands(&engine);
    }
    for (opcode, kind) in [(0xbc, BitScanKind::Forward), (0xbd, BitScanKind::Reverse)] {
        let engine = source(KEY + 8 + u64::from(opcode), &[0x0f, opcode, 0xc6]);
        let before = engine.arena().to_vec();
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
        assert_eq!(decoded.length(), 3);
        assert_eq!(
            decoded.operation(),
            &Operation::BitScan {
                kind,
                destination: Register32::Eax,
                source: Register32::Esi,
            }
        );
        assert_eq!(engine.arena(), before);
    }
    for bytes in [
        &[0x66, 0xf3, 0x0f, 0xbc, 0xc6][..],
        &[0xf3, 0x66, 0x0f, 0xbc, 0xc6],
        &[0xf3, 0x0f, 0xbd, 0xc6],
    ] {
        let engine = source(KEY + 20, bytes);
        let before = engine.arena().to_vec();
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
        assert!(matches!(decoded.operation(), Operation::BitCount { .. }));
        assert_eq!(usize::from(decoded.length()), bytes.len());
        assert_eq!(engine.arena(), before);
    }
    for bytes in [
        &[0x66, 0x66, 0xf3, 0x0f, 0xbc, 0xc6][..],
        &[0xf2, 0x0f, 0xbc, 0xc6],
    ] {
        let engine = source(KEY + 20, bytes);
        let before = engine.arena().to_vec();
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
            "{bytes:02x?}"
        );
        assert_eq!(engine.arena(), before);
    }
    println!("observed original count admission failures: {failures:?}");
    assert!(
        failures.is_empty(),
        "observed original bitmap dispatcher must admit exact +16 and four complete profiles: {failures:?}"
    );
}

fn assert_unmapped_operands(engine: &EngineInstance) {
    let memory = engine.memory().unwrap();
    assert_eq!(memory.mapped_pages(), 1);
    assert!(memory.resolve(GuestAddress(CODE), Access::Read).is_err());
    for address in [0, 0x3000, 0xa100_1234, 0xa101_1235, 0xa104_1238] {
        assert!(memory.resolve(GuestAddress(address), Access::Read).is_err());
    }
}

fn source(key: u64, bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, key).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(CODE, bytes.len() as u32).unwrap();
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
