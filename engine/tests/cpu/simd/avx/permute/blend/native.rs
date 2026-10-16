use ring3_engine::{
    abi::{
        arena::{HELPER_OFFSET, SIMD_OFFSET, TRANSFER_OFFSET, YMM_OFFSET},
        x86::{SIMD_SIZE, YMM_SIZE, decode_simd, decode_ymm, encode_simd, encode_ymm},
    },
    cpu::x86::{
        decode::{DecodeError, decode_one},
        ir::Operation,
        simd::{SimdState, ymm::YmmUpperState},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, HostError, StoreCompletion},
};

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
    let instruction = decode_one(&memory, GuestAddress(0x1000)).unwrap();
    assert!(matches!(instruction.operation(), Operation::Avx(_)));
    assert_eq!(usize::from(instruction.length()), bytes.len());
    assert_eq!(
        instruction.next_pc(),
        GuestAddress(0x1000 + bytes.len() as u32)
    );
    assert!(memory.is_code_current(instruction.code_snapshot()));
    *instruction.operation()
}

fn aliases(register: u8) -> [(u8, u8, u8); 5] {
    let next = (register + 1) % 8;
    [
        (register, next, (register + 2) % 8),
        (register, register, next),
        (register, next, register),
        (register, next, next),
        (register, register, register),
    ]
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

fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(2, 91).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x4000, 1, 3).unwrap();
    upload(&mut engine, 0x1000, &[0xeb, 0xfe]);
    describe(&mut engine, 2);
    engine.compile(1).unwrap();
    engine
}

fn ram(engine: &EngineInstance) -> Vec<u8> {
    let mut result = vec![0; 8192];
    for (page, address) in [0x1000, 0x4000].into_iter().enumerate() {
        engine
            .memory()
            .unwrap()
            .read(
                GuestAddress(address),
                &mut result[page * 4096..(page + 1) * 4096],
            )
            .unwrap();
    }
    result
}

fn seed(engine: &EngineInstance, vectors: &[[u8; 32]; 8]) -> Vec<u8> {
    let simd = SimdState {
        mxcsr: 0,
        xmm: std::array::from_fn(|r| vectors[r][..16].try_into().unwrap()),
    };
    let upper = YmmUpperState {
        registers: std::array::from_fn(|r| vectors[r][16..].try_into().unwrap()),
    };
    let mut before = engine.arena().to_vec();
    encode_simd(&simd, &mut before[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
    encode_ymm(&upper, &mut before[YMM_OFFSET..YMM_OFFSET + YMM_SIZE]).unwrap();
    before
}

fn expected(
    before: &[u8],
    destination: usize,
    value: Option<[u8; 32]>,
    length: u32,
    fault: bool,
) -> Vec<u8> {
    let mut result = before.to_vec();
    if let Some(value) = value {
        let mut simd = decode_simd(&result[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
        let mut upper = decode_ymm(&result[YMM_OFFSET..YMM_OFFSET + YMM_SIZE]).unwrap();
        simd.xmm[destination].copy_from_slice(&value[..16]);
        upper.registers[destination].copy_from_slice(&value[16..]);
        encode_simd(&simd, &mut result[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
        encode_ymm(&upper, &mut result[YMM_OFFSET..YMM_OFFSET + YMM_SIZE]).unwrap();
        result[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 544].fill(0);
        result[TRANSFER_OFFSET + 516..TRANSFER_OFFSET + 520].copy_from_slice(&FLAGS.to_le_bytes());
    }
    let packet = &mut result[HELPER_OFFSET..HELPER_OFFSET + 40];
    packet.fill(0);
    let version = if fault { 15 } else { 7 };
    packet[..12].copy_from_slice(&[b'R', b'3', b'M', b'H', version, 0, 1, 0, 40, 0, 0, 0]);
    let fields = if fault {
        [9, 0, 4, 0x5000, 1, length]
    } else {
        [0, 0, 0, 0, 0, length]
    };
    for (index, field) in fields.into_iter().enumerate() {
        packet[16 + index * 4..20 + index * 4].copy_from_slice(&field.to_le_bytes());
    }
    result
}

fn words(values: &[u32]) -> [u8; 32] {
    let mut result = [0; 32];
    for (lane, value) in values.iter().enumerate() {
        result[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    result
}

fn request(
    engine: &mut EngineInstance,
    descriptor: u32,
    operands: u32,
    control: u32,
    value: Option<[u8; 32]>,
    length: u32,
    fault: bool,
) {
    let before = engine.arena().to_vec();
    let before_ram = ram(engine);
    let wanted = expected(&before, (operands & 7) as usize, value, length, fault);
    assert_eq!(
        engine.avx_execute(descriptor, operands, control, 0, 0, FLAGS),
        Ok(StoreCompletion::Complete)
    );
    assert_eq!(engine.arena(), wanted);
    assert_eq!(ram(engine), before_ram);
}
fn encoding(
    wide: bool,
    (destination, first, second): (u8, u8, u8),
    immediate: u8,
    memory: bool,
) -> Vec<u8> {
    vec![
        0xc4,
        0xe3,
        (15 - first) << 3 | if wide { 4 } else { 0 } | 1,
        2,
        destination << 3 | if memory { 0 } else { 0xc0 | second },
        immediate,
    ]
}
fn pattern(register: usize) -> [u8; 32] {
    let values = [
        0x7f800001, 0x80000000, 1, 0xdeadbeef, 0x7fc01234, 0xff800001, 0x3f800000, 0x12345678,
    ];
    words(&std::array::from_fn::<_, 8, _>(|lane| {
        values[(lane + register) % 8]
    }))
}
fn blend(first: [u8; 32], second: [u8; 32], control: u8, wide: bool) -> [u8; 32] {
    let mut output = [0; 32];
    for lane in 0..if wide { 8 } else { 4 } {
        let input = if control & (1 << lane) == 0 {
            &first
        } else {
            &second
        };
        output[lane * 4..lane * 4 + 4].copy_from_slice(&input[lane * 4..lane * 4 + 4]);
    }
    output
}
fn descriptor(wide: bool) -> u32 {
    if wide { 0x01000416 } else { 0x01000415 }
}

#[test]
fn vpblendd_encodings_match_existing_raw_vblendps_operations() {
    for wide in [false, true] {
        for register in 0..8 {
            for roles in aliases(register) {
                for control in [0, 1, 15, 16, 128, 255] {
                    let bytes = encoding(wide, roles, control, false);
                    let operation = decoded(&bytes);
                    let mut floating = bytes.clone();
                    floating[3] = 0x0c;
                    assert_eq!(operation, decoded(&floating));
                    let mut wrong_w = bytes;
                    wrong_w[2] |= 0x80;
                    crate::support_fetch::assert_decode_contract(
                        decode_one(&code(&wrong_w), GuestAddress(0x1000)).unwrap_err(),
                        DecodeError::InvalidEncoding,
                    );
                }
            }
        }
        let bytes = encoding(wide, (1, 2, 0), 0, true);
        let mut floating = bytes.clone();
        floating[3] = 0x0c;
        assert_eq!(decoded(&bytes), decoded(&floating));
    }
}

#[test]
fn vpblendd_memory_capture_ignores_operand_pages_and_tracks_code_currentness() {
    for wide in [false, true] {
        for control in [0, 255] {
            let mut bytes = encoding(wide, (1, 2, 0), control, true);
            bytes[4] = 0x0d;
            bytes.splice(5..5, [0xef, 0xbe, 0xad, 0xde]);
            bytes.extend([0xeb, 0xfe]);
            let mut capture = EngineInstance::new(1, 91).unwrap();
            capture.map(0x1000, 1, 7).unwrap();
            upload(&mut capture, 0x1000, &bytes);
            describe(&mut capture, bytes.len());
            let before = capture.arena().to_vec();
            assert_eq!(capture.compile(1), Ok(1));
            assert_eq!(capture.arena(), before);
            assert_eq!(capture.guard(91, 1), Ok(()));
            upload(&mut capture, 0x1000, &[0x90]);
            assert!(capture.guard(91, 1).is_err());
        }
    }
}

#[test]
fn old_vblendps_helpers_keep_raw_aliases_and_width_specific_immediate_bits() {
    let first = words(&[10, 11, 12, 13, 20, 21, 22, 23]);
    let second = words(&[30, 31, 32, 33, 40, 41, 42, 43]);
    assert_eq!(blend(first, second, 0xf0, false), words(&[10, 11, 12, 13]));
    assert_eq!(
        blend(first, second, 0xf0, true),
        words(&[10, 11, 12, 13, 40, 41, 42, 43])
    );
    let mut engine = engine();
    for wide in [false, true] {
        for register in 0..8 {
            for (destination, first, second) in aliases(register) {
                for control in [0, 1, 15, 16, 128, 0x55, 0xaa, 255] {
                    let vectors = std::array::from_fn(pattern);
                    let wanted = blend(
                        vectors[usize::from(first)],
                        vectors[usize::from(second)],
                        control,
                        wide,
                    );
                    let before = seed(&engine, &vectors);
                    engine.arena_mut().unwrap().copy_from_slice(&before);
                    let mut bytes = encoding(wide, (destination, first, second), control, false);
                    bytes[3] = 0x0c;
                    decoded(&bytes);
                    request(
                        &mut engine,
                        descriptor(wide),
                        u32::from(destination) | u32::from(first) << 3 | u32::from(second) << 6,
                        u32::from(control),
                        Some(wanted),
                        0,
                        false,
                    );
                }
            }
        }
    }
}

#[test]
fn shared_helpers_read_full_vector_even_when_no_memory_lane_is_selected() {
    let mut engine = engine();
    for wide in [false, true] {
        let length = if wide { 32 } else { 16 };
        let address = 0x5000 - length;
        let operand = pattern(3);
        upload(&mut engine, address, &operand[..length as usize]);
        for control in [0, 255] {
            for (address, fault) in [(address, false), (address + 1, true)] {
                let vectors = std::array::from_fn(pattern);
                let mut before = seed(&engine, &vectors);
                let metadata = &mut before[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568];
                metadata.fill(0);
                metadata[..12].copy_from_slice(&[b'R', b'3', b'A', b'M', 1, 0, 1, 0, 24, 0, 0, 0]);
                metadata[16..20].copy_from_slice(&3_u32.to_le_bytes());
                metadata[20..24].copy_from_slice(&address.to_le_bytes());
                engine.arena_mut().unwrap().copy_from_slice(&before);
                let output = (!fault).then(|| blend(vectors[0], operand, control, wide));
                request(
                    &mut engine,
                    descriptor(wide),
                    1 << 12,
                    u32::from(control),
                    output,
                    length,
                    fault,
                );
            }
        }
    }
}

#[test]
fn forbidden_prefixes_and_malformed_shared_slots_remain_neutral() {
    let mut engine = engine();
    let before_ram = ram(&engine);
    for wide in [false, true] {
        for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
            let mut bytes = vec![prefix];
            bytes.extend(encoding(wide, (1, 2, 3), 0x55, false));
            crate::support_fetch::assert_decode_contract(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding,
            );
        }
        for operands in [1 << 9, 1 << 13, 1 << 12 | 1 << 6] {
            let before = engine.arena().to_vec();
            assert_eq!(
                engine.avx_execute(descriptor(wide), operands, 255, 0, 0, FLAGS),
                Err(HostError::InvalidRequest)
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(ram(&engine), before_ram);
        }
    }
}
