use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::x86::{X87Operation, decode::decode_one, ir::Operation},
    memory::GuestAddress,
    process::EngineInstance,
};

#[test]
fn integer_flags_comparison_and_preserving_x87_calls_compile_in_all_bound_profiles() {
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = EngineInstance::new(1, 0x5838_3750).unwrap();
            engine.map(0x1000, 1, 7).unwrap();
            let bytes = [0xd9, 0xe8, 0xdb, 0xf1, 0xeb, 0xfe];
            engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
                .copy_from_slice(&bytes);
            engine.upload(0x1000, bytes.len() as u32).unwrap();
            engine.protect(0x1000, 1, 4).unwrap();
            for (offset, expected) in [
                (0, X87Operation::LoadConstant { index: 0 }),
                (
                    2,
                    X87Operation::Compare {
                        index: 1,
                        quiet: false,
                        pop: 0,
                        integer_flags: true,
                    },
                ),
            ] {
                let decoded =
                    decode_one(engine.memory().unwrap(), GuestAddress(0x1000 + offset)).unwrap();
                assert!(
                    matches!(decoded.operation(), Operation::X87 { operation, .. } if *operation == expected)
                );
                assert_eq!(decoded.length(), 2);
            }
            let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
            request[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
            request[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
            let before = engine.arena().to_vec();
            match (resident, entries) {
                (false, false) => {
                    engine.compile(1).unwrap();
                }
                (false, true) => {
                    engine.compile_entries(1, 0).unwrap();
                }
                (true, false) => {
                    engine.compile_resident(1).unwrap();
                }
                (true, true) => {
                    engine.compile_resident_entries(1, 0).unwrap();
                }
            }
            assert_eq!(engine.arena(), before);
        }
    }
}
