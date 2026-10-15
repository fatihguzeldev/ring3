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
    process::{EngineInstance, StoreCompletion},
};

const FORMS: [(u8, u8, usize, bool); 8] = [
    (0xfc, 26, 1, false),
    (0xfd, 27, 2, false),
    (0xfe, 28, 4, false),
    (0xd4, 7, 8, false),
    (0xf8, 22, 1, true),
    (0xf9, 23, 2, true),
    (0xfa, 24, 4, true),
    (0xfb, 25, 8, true),
];

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

fn aliases(register: u8) -> [(u8, u8, u8); 5] {
    let next = (register + 1) % 8;
    let third = (register + 2) % 8;
    [
        (register, next, third),
        (register, register, next),
        (register, next, register),
        (register, next, next),
        (register, register, register),
    ]
}

#[test]
fn ymm_forms_preserve_three_operand_aliases_and_ignored_w() {
    let mut operations = Vec::new();
    for (opcode, _, _, _) in FORMS {
        let operation = decoded(&[0xc5, 0xed, opcode, 0xcb]);
        assert!(!operations.contains(&operation));
        operations.push(operation);
        for register in 0..8 {
            for (destination, first, second) in aliases(register) {
                let control = 0x85 | (15 - first) << 3;
                let modrm = 0xc0 | destination << 3 | second;
                let two_byte = decoded(&[0xc5, control, opcode, modrm]);
                for w in [0, 0x80] {
                    let three_byte = decoded(&[0xc4, 0xe1, (control & 0x7f) | w, opcode, modrm]);
                    assert_eq!(two_byte, three_byte);
                }
                assert_ne!(two_byte, decoded(&[0xc5, control & !4, opcode, modrm]));
                let next_destination = 0xc0 | ((destination + 1) % 8) << 3 | second;
                assert_ne!(
                    two_byte,
                    decoded(&[0xc5, control, opcode, next_destination])
                );
                let next_first = 0x85 | (15 - (first + 1) % 8) << 3;
                assert_ne!(two_byte, decoded(&[0xc5, next_first, opcode, modrm]));
                let next_second = 0xc0 | destination << 3 | ((second + 1) % 8);
                assert_ne!(two_byte, decoded(&[0xc5, control, opcode, next_second]));
            }
        }
    }
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

#[test]
fn memory_forms_capture_addresses_without_reading_guest_operands() {
    for (opcode, _, _, _) in FORMS {
        let mut addresses = Vec::new();
        for (prefix, address) in [
            (None, &[0x08][..]),
            (None, &[0x4c, 0x24, 0x7f][..]),
            (None, &[0x0d, 0xef, 0xbe, 0xad, 0xde][..]),
            (Some(0x64), &[0x08][..]),
            (Some(0x67), &[0x0a][..]),
        ] {
            let mut bytes = prefix.into_iter().collect::<Vec<_>>();
            bytes.extend([0xc4, 0xe1, 0x6d, opcode]);
            bytes.extend(address);
            let operation = decoded(&bytes);
            let mut w_alias = bytes.clone();
            w_alias[usize::from(prefix.is_some()) + 2] |= 0x80;
            assert_eq!(operation, decoded(&w_alias));
            assert!(!addresses.contains(&operation));
            addresses.push(operation);
        }
        let mut bytes = vec![0xc5, 0xed, opcode, 0x0d, 0xef, 0xbe, 0xad, 0xde];
        bytes.extend([0xeb, 0xfe]);
        let mut engine = EngineInstance::new(1, 91).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        upload(&mut engine, 0x1000, &bytes);
        describe(&mut engine, bytes.len());
        let before = engine.arena().to_vec();
        assert_eq!(engine.compile(1), Ok(1));
        assert_eq!(engine.arena(), before);
        assert_eq!(&engine.artifact_bytes().unwrap()[..8], b"\0asm\x01\0\0\0");
        assert_eq!(engine.guard(91, 1), Ok(()));
        upload(&mut engine, 0x1000, &[0x90]);
        assert!(engine.guard(91, 1).is_err());
    }
}

fn helper_engine() -> EngineInstance {
    let mut engine = EngineInstance::new(2, 91).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    upload(&mut engine, 0x1000, &[0xeb, 0xfe]);
    describe(&mut engine, 2);
    engine.compile(1).unwrap();
    engine
}

fn pattern(width: usize, register: usize) -> [u8; 32] {
    let mask = u64::MAX >> (64 - width * 8);
    let sign = 1_u64 << (width * 8 - 1);
    let values = [0, mask, sign, sign - 1, 1, mask - 1, sign + 1, mask / 3];
    std::array::from_fn(|byte| {
        values[(byte / width + register * 3) % values.len()].to_le_bytes()[byte % width]
    })
}

fn wrapping_oracle(width: usize, subtract: bool, first: &[u8], second: &[u8]) -> Vec<u8> {
    let modulus = 1_u128 << (width * 8);
    let mut result = vec![0; first.len()];
    for offset in (0..first.len()).step_by(width) {
        let read = |bytes: &[u8]| {
            bytes
                .iter()
                .rev()
                .fold(0_u128, |value, &byte| value * 256 + u128::from(byte))
        };
        let a = read(&first[offset..offset + width]);
        let b = read(&second[offset..offset + width]);
        let value = if subtract { a + modulus - b } else { a + b } % modulus;
        result[offset..offset + width].copy_from_slice(&value.to_le_bytes()[..width]);
    }
    result
}

fn helper_wraps(width256: bool) {
    let mut engine = helper_engine();
    let untouched = engine.arena().to_vec();
    for (_, kind, width, subtract) in FORMS {
        for register in 0..8 {
            for (destination, first, second) in aliases(register) {
                let vectors: [[u8; 32]; 8] = std::array::from_fn(|r| pattern(width, r));
                let mut simd = SimdState {
                    mxcsr: 0xffff,
                    xmm: std::array::from_fn(|r| vectors[r][..16].try_into().unwrap()),
                };
                let mut upper = YmmUpperState {
                    registers: std::array::from_fn(|r| vectors[r][16..].try_into().unwrap()),
                };
                let mut before = untouched.clone();
                encode_simd(&simd, &mut before[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
                encode_ymm(&upper, &mut before[YMM_OFFSET..YMM_OFFSET + YMM_SIZE]).unwrap();
                engine.arena_mut().unwrap().copy_from_slice(&before);
                let mut expected = before;
                let bytes = if width256 { 32 } else { 16 };
                let result = wrapping_oracle(
                    width,
                    subtract,
                    &vectors[usize::from(first)][..bytes],
                    &vectors[usize::from(second)][..bytes],
                );
                simd.xmm[usize::from(destination)].copy_from_slice(&result[..16]);
                upper.registers[usize::from(destination)] = if width256 {
                    result[16..].try_into().unwrap()
                } else {
                    [0; 16]
                };
                encode_simd(&simd, &mut expected[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
                encode_ymm(&upper, &mut expected[YMM_OFFSET..YMM_OFFSET + YMM_SIZE]).unwrap();
                expected[HELPER_OFFSET..HELPER_OFFSET + 40].fill(0);
                expected[HELPER_OFFSET..HELPER_OFFSET + 4].copy_from_slice(b"R3MH");
                expected[HELPER_OFFSET + 4..HELPER_OFFSET + 8].copy_from_slice(&[7, 0, 1, 0]);
                expected[HELPER_OFFSET + 8..HELPER_OFFSET + 12]
                    .copy_from_slice(&40_u32.to_le_bytes());
                expected[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 544].fill(0);
                expected[TRANSFER_OFFSET + 516..TRANSFER_OFFSET + 520]
                    .copy_from_slice(&0x244cd7_u32.to_le_bytes());
                let descriptor = 0x01000500 | u32::from(kind | if width256 { 0x80 } else { 0 });
                let operands =
                    u32::from(destination) | u32::from(first) << 3 | u32::from(second) << 6;
                assert_eq!(
                    engine.avx_execute(descriptor, operands, 0, 0, 0, 0x244cd7),
                    Ok(StoreCompletion::Complete),
                    "descriptor{descriptor:08x} operands{operands:04x}",
                );
                assert_eq!(
                    engine.arena(),
                    expected,
                    "full arena including flags and mxcsr"
                );
            }
        }
    }
}

#[test]
fn ymm_helper_wraps_all_lane_widths_and_source_aliases() {
    helper_wraps(true);
}

#[test]
fn existing_xmm_wraps_and_zeroes_upper_lanes() {
    for (opcode, _, _, _) in FORMS {
        decoded(&[0xc5, 0xe9, opcode, 0xcb]);
        decoded(&[0xc4, 0xe1, 0xe9, opcode, 0xcb]);
    }
    helper_wraps(false);
}

#[test]
fn forbidden_legacy_prefixes_stay_encoding_errors() {
    for (opcode, _, _, _) in FORMS {
        for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
            let bytes = [prefix, 0xc4, 0xe1, 0x6d, opcode, 0xcb];
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding,
                "{bytes:02x?}",
            );
        }
    }
}

#[test]
fn ymm_memory_reads_all_bytes_before_publishing_an_aliased_destination() {
    let mut engine = helper_engine();
    engine.map(0x4000, 1, 3).unwrap();
    let data: [u8; 32] = std::array::from_fn(|byte| (byte * 37 + 19) as u8);
    upload(&mut engine, 0x4fe0, &data);
    let untouched = engine.arena().to_vec();
    for (_, kind, width, subtract) in FORMS {
        for (address, fault) in [(0x4fe0_u32, false), (0x4fe1, true)] {
            let first = pattern(width, 0);
            let mut simd = SimdState {
                mxcsr: 0xffff,
                xmm: std::array::from_fn(|r| pattern(width, r)[..16].try_into().unwrap()),
            };
            let mut upper = YmmUpperState {
                registers: std::array::from_fn(|r| pattern(width, r)[16..].try_into().unwrap()),
            };
            let mut before = untouched.clone();
            encode_simd(&simd, &mut before[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
            encode_ymm(&upper, &mut before[YMM_OFFSET..YMM_OFFSET + YMM_SIZE]).unwrap();
            let metadata = &mut before[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568];
            metadata.fill(0);
            metadata[..4].copy_from_slice(b"R3AM");
            metadata[4..8].copy_from_slice(&[1, 0, 1, 0]);
            metadata[8..12].copy_from_slice(&24_u32.to_le_bytes());
            metadata[16..20].copy_from_slice(&3_u32.to_le_bytes());
            metadata[20..24].copy_from_slice(&address.to_le_bytes());
            engine.arena_mut().unwrap().copy_from_slice(&before);
            let mut expected = before;
            if !fault {
                let result = wrapping_oracle(width, subtract, &first, &data);
                simd.xmm[0].copy_from_slice(&result[..16]);
                upper.registers[0].copy_from_slice(&result[16..]);
                encode_simd(&simd, &mut expected[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
                encode_ymm(&upper, &mut expected[YMM_OFFSET..YMM_OFFSET + YMM_SIZE]).unwrap();
                expected[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 544].fill(0);
                expected[TRANSFER_OFFSET + 516..TRANSFER_OFFSET + 520]
                    .copy_from_slice(&0x244cd7_u32.to_le_bytes());
            }
            let packet = &mut expected[HELPER_OFFSET..HELPER_OFFSET + 40];
            packet.fill(0);
            packet[..4].copy_from_slice(b"R3MH");
            let version = if fault { 15 } else { 7 };
            packet[4..8].copy_from_slice(&[version, 0, 1, 0]);
            packet[8..12].copy_from_slice(&40_u32.to_le_bytes());
            let fields = if fault {
                [9_u32, 0, 4, 0x5000, 1, 32]
            } else {
                [0, 0, 0, 0, 0, 32]
            };
            for (index, field) in fields.into_iter().enumerate() {
                packet[16 + index * 4..20 + index * 4].copy_from_slice(&field.to_le_bytes());
            }
            assert_eq!(
                engine.avx_execute(0x01000580 | u32::from(kind), 1 << 12, 0, 0, 0, 0x244cd7),
                Ok(StoreCompletion::Complete),
            );
            assert_eq!(engine.arena(), expected, "kind{kind} address{address:08x}");
        }
    }
}
