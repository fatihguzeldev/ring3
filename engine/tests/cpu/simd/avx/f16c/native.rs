use ring3_engine::{
    cpu::x86::{
        decode::{DecodeError, decode_one},
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn code(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory
}

#[test]
fn all_four_forms_admit_register_aliases_and_exact_memory_operands() {
    for (map, control, opcode, narrow) in [
        (0xe2, 0x79, 0x13, false),
        (0xe2, 0x7d, 0x13, false),
        (0xe3, 0x79, 0x1d, true),
        (0xe3, 0x7d, 0x1d, true),
    ] {
        for first in 0..8 {
            for second in 0..8 {
                let mut bytes = vec![0xc4, map, control, opcode, 0xc0 | first << 3 | second];
                if narrow {
                    bytes.push(0xff);
                }
                let memory = code(&bytes);
                let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
                assert!(matches!(decoded.operation(), Operation::Avx(_)));
                assert_eq!(usize::from(decoded.length()), bytes.len());
            }
        }
        for addressing in [
            vec![0x08],
            vec![0x4c, 0x24, 0x7f],
            vec![0x0d, 0, 0x50, 0, 0],
        ] {
            let mut bytes = vec![0xc4, map, control, opcode];
            bytes.extend(addressing);
            if narrow {
                bytes.push(4);
            }
            let memory = code(&bytes);
            let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
            assert!(matches!(decoded.operation(), Operation::Avx(_)));
            assert_eq!(usize::from(decoded.length()), bytes.len());
        }
    }
}

#[test]
fn every_immediate_and_address_size_override_is_admitted() {
    for control in [0x79, 0x7d] {
        for immediate in 0..=255 {
            let bytes = [0xc4, 0xe3, control, 0x1d, 0xc9, immediate];
            assert!(matches!(
                decode_one(&code(&bytes), GuestAddress(0x1000))
                    .unwrap()
                    .operation(),
                Operation::Avx(_)
            ));
        }
        for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0x67] {
            let bytes = [prefix, 0xc4, 0xe2, control, 0x13, 0x08];
            assert!(matches!(
                decode_one(&code(&bytes), GuestAddress(0x1000))
                    .unwrap()
                    .operation(),
                Operation::Avx(_)
            ));
        }
    }
}

#[test]
fn forbidden_prefixes_remain_strict_encoding_errors() {
    for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
        let bytes = [prefix, 0xc4, 0xe2, 0x79, 0x13, 0xcb];
        assert_eq!(
            decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
    }
}

#[test]
fn numerical_store_preflights_write_authority_before_sticky_publication() {
    use ring3_engine::{
        abi::arena::{HELPER_OFFSET, TRANSFER_OFFSET},
        process::{EngineInstance, StoreCompletion},
    };
    let mut engine = EngineInstance::new(2, 91).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x5000, 1, 3).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    for (offset, value) in [(4364 + 16, 0x1f00_u32), (4364 + 32 + 16, 0x7f80_0001)] {
        engine.arena_mut().unwrap()[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let metadata = TRANSFER_OFFSET + 544;
    engine.arena_mut().unwrap()[metadata..metadata + 24]
        .copy_from_slice(b"R3AM\x01\0\x01\0\x18\0\0\0\0\0\0\0\x03\0\0\0\xfc\x5f\0\0");
    let before = engine.arena().to_vec();
    let mut expected = before.clone();
    expected[HELPER_OFFSET..HELPER_OFFSET + 40].copy_from_slice(b"R3MH\x0f\0\x01\0\x28\0\0\0\0\0\0\0\x09\0\0\0\0\0\0\0\x06\0\0\0\0\x60\0\0\x02\0\0\0\x10\0\0\0");
    assert_eq!(
        engine.avx_execute(0x0100_1204, 1 << 6 | 1 << 13, 0, 0, 0, 2),
        Ok(StoreCompletion::Complete)
    );
    assert_eq!(engine.arena(), expected);
    // snapshotting mapped RAM uses its public read helper and restores the exact caller frame.
    let frame = engine.arena().to_vec();
    engine.cpu_read(0x5000, 512, 1).unwrap();
    assert!(
        engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + 512]
            .iter()
            .all(|byte| *byte == 0)
    );
    engine.arena_mut().unwrap().copy_from_slice(&frame);
    assert_eq!(engine.arena(), expected);
}
