use ring3_engine::{
    abi::arena::{ARENA_SIZE, TRANSFER_OFFSET, X87_OFFSET},
    cpu::x86::decode::decode_one,
    memory::{Access, GuestAddress},
    process::EngineInstance,
};

const CODE: u32 = 0x2000;
const KEY: u64 = 0x4c58_4144_3136_4f52;
const ORIGINAL: &[u8; 15] = include_bytes!("../../fixtures/p2-locked-xadd16/kernel.text.bin");

#[test]
fn original_word_fetch_add_keeps_the_complete_observed_function() {
    assert_eq!(
        ORIGINAL,
        &[
            0x8b, 0x4c, 0x24, 0x04, 0x0f, 0xb7, 0x44, 0x24, 0x08, 0x66, 0xf0, 0x0f, 0xc1, 0x01,
            0xc3
        ]
    );
    let starts = [0_u32, 4, 9, 14, 15];
    let mut engine = source();
    let before = engine.arena().to_vec();
    for pair in starts.windows(2) {
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE + pair[0])).unwrap();
        assert_eq!(u32::from(decoded.length()), pair[1] - pair[0]);
        assert_eq!(decoded.next_pc(), GuestAddress(CODE + pair[1]));
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
    }
    assert_eq!(&ORIGINAL[9..14], &[0x66, 0xf0, 0x0f, 0xc1, 0x01]);
    assert_eq!(ORIGINAL[14], 0xc3);
    assert_eq!(engine.arena(), before);
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0), Access::Read)
            .is_err()
    );
    engine.close();
}

#[test]
fn complete_original_word_fetch_add_compiles_in_four_bound_profiles_without_operand_reads() {
    for (resident, entries) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut engine = source();
        let request = if entries {
            vec![CODE]
        } else {
            vec![CODE, ORIGINAL.len() as u32]
        };
        for (index, value) in request.iter().enumerate() {
            let offset = TRANSFER_OFFSET + index * 4;
            engine.arena_mut().unwrap()[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        let before = engine.arena().to_vec();
        let id = match (resident, entries) {
            (false, false) => u64::from(engine.compile(1).unwrap()),
            (false, true) => u64::from(engine.compile_entries(1, 0).unwrap()),
            (true, false) => engine.compile_resident(1).unwrap().get(),
            (true, true) => engine.compile_resident_entries(1, 0).unwrap().get(),
        };
        let bytes = if resident {
            engine.resident_bytes(id).unwrap()
        } else {
            engine.artifact_bytes().unwrap()
        };
        assert_eq!(&bytes[..8], b"\0asm\x01\0\0\0");
        assert!(bytes.len() <= 65536);
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
        assert!(
            engine
                .memory()
                .unwrap()
                .resolve(GuestAddress(0), Access::Read)
                .is_err()
        );
    }
}

fn source() -> EngineInstance {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + ORIGINAL.len()]
        .copy_from_slice(ORIGINAL);
    engine.upload(CODE, ORIGINAL.len() as u32).unwrap();
    engine.protect(CODE, 1, 4).unwrap();
    let arena = engine.arena_mut().unwrap();
    assert_eq!(arena.len(), ARENA_SIZE);
    for index in 0..8 {
        let start = 16 + index * 4;
        let value = 0xa100_1234_u32.wrapping_add(index as u32 * 0x0001_0001);
        arena[start..start + 4].copy_from_slice(&value.to_le_bytes());
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
