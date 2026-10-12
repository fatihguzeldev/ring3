use ring3_engine::{
    abi::arena::{ARENA_SIZE, TRANSFER_OFFSET},
    cpu::{dbt::CompileLimits, x86::decode::decode_one},
    memory::{Access, GuestAddress},
    process::EngineInstance,
};

const CODE: u32 = 0x2000;

#[test]
fn whole_original_x87_function_admits_all_bound_profiles_without_operand_access() {
    admit(
        include_bytes!("fixtures/p2-fp-compiled/x87/kernel.text.bin"),
        &[(0, 21, 9), (21, 3, 1), (24, 16, 7), (40, 7, 4)],
    );
}

#[test]
fn whole_original_sse2_function_admits_all_bound_profiles_without_operand_access() {
    admit(
        include_bytes!("fixtures/p2-fp-compiled/sse2/kernel.text.bin"),
        &[(0, 24, 10), (24, 22, 6), (46, 32, 9), (78, 38, 13)],
    );
}

fn admit(original: &[u8], blocks: &[(u32, u32, usize)]) {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65536)
    );
    assert_eq!(
        blocks.iter().map(|block| block.1).sum::<u32>() as usize,
        original.len()
    );
    for (ordinal, (resident, entries)) in
        [(false, false), (false, true), (true, false), (true, true)]
            .into_iter()
            .enumerate()
    {
        let mut engine = EngineInstance::new(1, 0x4650_434f_4d50_0000 + ordinal as u64).unwrap();
        engine.map(CODE, 1, 7).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + original.len()]
            .copy_from_slice(original);
        engine.upload(CODE, original.len() as u32).unwrap();
        engine.protect(CODE, 1, 4).unwrap();
        let before_decode = engine.arena().to_vec();
        for &(offset, length, count) in blocks {
            let mut pc = CODE + offset;
            let mut observed = 0;
            while pc < CODE + offset + length {
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .is_code_current(decoded.code_snapshot())
                );
                pc = decoded.next_pc().0;
                observed += 1;
            }
            assert_eq!(pc, CODE + offset + length);
            assert_eq!(observed, count);
        }
        assert_eq!(engine.arena(), before_decode);
        let request: Vec<u32> = blocks
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
        let before_compile = engine.arena().to_vec();
        let count = blocks.len() as u32;
        let id = match (resident, entries) {
            (false, false) => u64::from(engine.compile(count).unwrap()),
            (false, true) => u64::from(engine.compile_entries(count, 0).unwrap()),
            (true, false) => engine.compile_resident(count).unwrap().get(),
            (true, true) => engine.compile_resident_entries(count, 0).unwrap().get(),
        };
        let bytes = if resident {
            engine.resident_bytes(id).unwrap()
        } else {
            engine.artifact_bytes().unwrap()
        };
        assert_eq!(&bytes[..8], b"\0asm\x01\0\0\0");
        assert!(bytes.len() <= limits.wasm_bytes);
        assert_eq!(engine.arena(), before_compile);
        assert_eq!(engine.arena().len(), ARENA_SIZE);
        assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
        for operand in [0, CODE, 0x3000, 0x4000, 0x8000, 0xffff_fffc] {
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(operand), Access::Read)
                    .is_err()
            );
        }
    }
}
