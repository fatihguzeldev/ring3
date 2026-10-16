use ring3_engine::{
    abi::{
        arena::{HELPER_OFFSET, SIMD_OFFSET, TRANSFER_OFFSET, YMM_OFFSET},
        x86::{SIMD_SIZE, YMM_SIZE, encode_simd, encode_ymm},
    },
    cpu::x86::{
        decode::{DecodeError, decode_one},
        ir::Operation,
        simd::{SimdState, ymm::YmmUpperState},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, HostError, StoreCompletion},
};

const KEY: u64 = 91;
const FLAGS: u32 = 0x244cd7;

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

fn decoded(bytes: &[u8]) -> Operation {
    let memory = code(bytes);
    let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
    assert!(
        matches!(decoded.operation(), Operation::Avx(_)),
        "{bytes:02x?}"
    );
    assert_eq!(usize::from(decoded.length()), bytes.len());
    assert_eq!(decoded.next_pc(), GuestAddress(0x1000 + bytes.len() as u32));
    assert!(memory.is_code_current(decoded.code_snapshot()));
    *decoded.operation()
}

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn describe(engine: &mut EngineInstance, length: usize) {
    let arena = engine.arena_mut().unwrap();
    arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 4].copy_from_slice(&0x1000_u32.to_le_bytes());
    arena[TRANSFER_OFFSET + 4..TRANSFER_OFFSET + 8].copy_from_slice(&(length as u32).to_le_bytes());
}

fn helper_engine() -> (EngineInstance, u64) {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x5000, 1, 3).unwrap();
    upload(&mut engine, 0x1000, &[0xeb, 0xfe]);
    describe(&mut engine, 2);
    engine.compile(1).unwrap();
    describe(&mut engine, 2);
    let id = engine.compile_resident(1).unwrap().get();
    (engine, id)
}

fn packet(expected: &mut [u8], fields: [u32; 6]) {
    let output = &mut expected[HELPER_OFFSET..HELPER_OFFSET + 40];
    output.fill(0);
    output[..12].copy_from_slice(&[b'R', b'3', b'M', b'H', 7, 0, 1, 0, 40, 0, 0, 0]);
    for (index, field) in fields.into_iter().enumerate() {
        output[16 + index * 4..20 + index * 4].copy_from_slice(&field.to_le_bytes());
    }
}

fn ram(engine: &EngineInstance) -> Vec<u8> {
    let mut bytes = vec![0; 8192];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x1000), &mut bytes[..4096])
        .unwrap();
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x5000), &mut bytes[4096..])
        .unwrap();
    bytes
}

fn encoding(destination: u8, source: u8, wide: bool, w: bool, c5: bool) -> Vec<u8> {
    let control = 0x79 | if wide { 4 } else { 0 };
    if c5 {
        assert!(!w);
        vec![
            0xc5,
            0x80 | control,
            0xd7,
            0xc0 | (destination << 3) | source,
        ]
    } else {
        vec![
            0xc4,
            0xe1,
            control | if w { 0x80 } else { 0 },
            0xd7,
            0xc0 | (destination << 3) | source,
        ]
    }
}

#[test]
fn ymm_guest_masks_bind_all_general_destinations_and_vector_sources_with_ignored_w() {
    for destination in 0..8 {
        for source in 0..8 {
            let operation = decoded(&encoding(destination, source, true, false, false));
            assert_eq!(
                operation,
                decoded(&encoding(destination, source, true, true, false))
            );
            assert_eq!(
                operation,
                decoded(&encoding(destination, source, true, false, true))
            );
            assert_ne!(
                operation,
                decoded(&encoding(destination, source, false, false, false))
            );
            assert_ne!(
                operation,
                decoded(&encoding((destination + 1) % 8, source, true, false, false))
            );
            assert_ne!(
                operation,
                decoded(&encoding(destination, (source + 1) % 8, true, false, false))
            );
        }
    }
}

fn extract(bytes: &[u8]) -> u32 {
    bytes.iter().enumerate().fold(0, |mask, (index, byte)| {
        mask | u32::from(byte & 0x80 != 0) << index
    })
}

fn seed(engine: &mut EngineInstance, source: usize, pattern: u32) {
    let mut simd = SimdState {
        mxcsr: 0xffff,
        xmm: std::array::from_fn(|r| [r as u8 * 29; 16]),
    };
    let mut upper = YmmUpperState {
        registers: std::array::from_fn(|r| [r as u8 * 31 + 3; 16]),
    };
    let bytes: [u8; 32] = std::array::from_fn(|byte| {
        ((byte * 7 + source * 11) as u8 & 0x7f) | if pattern & (1 << byte) != 0 { 0x80 } else { 0 }
    });
    assert_eq!(extract(&bytes), pattern);
    simd.xmm[source].copy_from_slice(&bytes[..16]);
    upper.registers[source].copy_from_slice(&bytes[16..]);
    let arena = engine.arena_mut().unwrap();
    encode_simd(&simd, &mut arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
    encode_ymm(&upper, &mut arena[YMM_OFFSET..YMM_OFFSET + YMM_SIZE]).unwrap();
    arena[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 544].fill(0xa5);
    arena[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568].fill(0xff);
}

fn helpers(wide: bool) {
    let (mut engine, id) = helper_engine();
    let before_ram = ram(&engine);
    for destination in 0..8_u32 {
        for source in 0..8_u32 {
            for pattern in [
                0,
                u32::MAX,
                1,
                0x8000,
                0x10000,
                0x80000000,
                0xaaaa5555,
                0x12345678,
            ] {
                for resident in [false, true] {
                    seed(&mut engine, source as usize, pattern);
                    let before = engine.arena().to_vec();
                    let mut expected = before;
                    packet(&mut expected, [0; 6]);
                    expected[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 544].fill(0);
                    expected[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 516].copy_from_slice(
                        &(if wide { pattern } else { pattern & 0xffff }).to_le_bytes(),
                    );
                    expected[TRANSFER_OFFSET + 516..TRANSFER_OFFSET + 520]
                        .copy_from_slice(&FLAGS.to_le_bytes());
                    let descriptor = if wide { 0x01000983 } else { 0x01000903 };
                    let operands = destination | (source << 6);
                    let result = if resident {
                        engine.avx_execute_resident(KEY, id, descriptor, operands, 0, 0, 0, FLAGS)
                    } else {
                        engine.avx_execute(descriptor, operands, 0, 0, 0, FLAGS)
                    };
                    assert_eq!(result, Ok(StoreCompletion::Complete));
                    assert_eq!(
                        engine.arena(),
                        expected,
                        "wide{wide} D{destination} B{source} bits{pattern:08x}"
                    );
                    assert_eq!(ram(&engine), before_ram);
                }
            }
        }
    }
}

#[test]
fn ymm_helpers_extract_all_thirty_two_sign_bits_and_preserve_vectors_flags_and_ram() {
    helpers(true);
}

#[test]
fn existing_xmm_mask_keeps_zero_extended_sixteen_bit_result_and_vector_neutrality() {
    for w in [false, true] {
        decoded(&encoding(7, 6, false, w, false));
    }
    helpers(false);
}

#[test]
fn reserved_vvvv_memory_encoding_and_legacy_prefixes_remain_invalid() {
    for wide in [false, true] {
        for w in [false, true] {
            let valid = encoding(0, 1, wide, w, false);
            for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
                let mut bytes = vec![prefix];
                bytes.extend(&valid);
                crate::support_fetch::assert_decode_contract(
                    decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                    DecodeError::InvalidEncoding,
                );
            }
            for reserved in 0..15 {
                let mut bytes = valid.clone();
                bytes[2] = bytes[2] & !0x78 | (reserved << 3);
                crate::support_fetch::assert_decode_contract(
                    decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                    DecodeError::InvalidEncoding,
                );
            }
            let mut memory = valid;
            memory[4] = 0;
            crate::support_fetch::assert_decode_contract(
                decode_one(&code(&memory), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding,
            );
        }
    }
}

#[test]
fn malformed_wide_mask_descriptors_refuse_without_any_publication() {
    let (mut engine, _) = helper_engine();
    for (descriptor, operands, immediate) in [
        (0x01000983, 1 << 3, 0),
        (0x01000983, 1 << 9, 0),
        (0x01000983, 1 << 12, 0),
        (0x01000983, 1 << 13, 0),
        (0x01000983, 0, 1),
        (0x01000983, 0, 256),
        (0x01000981, 0, 0),
        (0x01000982, 0, 0),
        (0x01000984, 0, 0),
        (0x010009c3, 0, 0),
    ] {
        seed(&mut engine, 0, 0x80010001);
        let before = engine.arena().to_vec();
        let before_ram = ram(&engine);
        assert_eq!(
            engine.avx_execute(descriptor, operands, immediate, 0, 0, FLAGS),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(ram(&engine), before_ram);
    }
}
