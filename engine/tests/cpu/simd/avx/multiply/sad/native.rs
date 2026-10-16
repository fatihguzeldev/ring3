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
    w: bool,
) -> Vec<u8> {
    vec![
        0xc4,
        0xe3,
        (15 - first) << 3 | if wide { 4 } else { 0 } | if w { 0x80 } else { 0 } | 1,
        0x42,
        destination << 3 | if memory { 0 } else { 0xc0 | second },
        immediate,
    ]
}
fn pattern(register: usize) -> [u8; 32] {
    std::array::from_fn(|byte| ((byte * 13 + register * 17) ^ (byte / 16 * 0xa5)) as u8)
}
fn oracle(first: [u8; 32], second: [u8; 32], immediate: u8, wide: bool) -> [u8; 32] {
    let mut output = [0; 32];
    for half in 0..if wide { 2 } else { 1 } {
        let control = immediate >> (half * 3) & 7;
        let a = half * 16 + usize::from(control >> 2) * 4;
        let b = half * 16 + usize::from(control & 3) * 4;
        for lane in 0..8 {
            let sum: u16 = (0..4)
                .map(|byte| u16::from(first[a + lane + byte].abs_diff(second[b + byte])))
                .sum();
            output[half * 16 + lane * 2..half * 16 + lane * 2 + 2]
                .copy_from_slice(&sum.to_le_bytes());
        }
    }
    output
}
fn words(values: [u16; 16]) -> [u8; 32] {
    let mut output = [0; 32];
    for (lane, value) in values.into_iter().enumerate() {
        output[lane * 2..lane * 2 + 2].copy_from_slice(&value.to_le_bytes());
    }
    output
}
#[test]
fn ymm_encoding_preserves_wig_roles_and_captures_without_operand_reads() {
    for register in 0..8 {
        for roles in aliases(register) {
            for immediate in [0, 7, 8, 32, 63, 200, 255] {
                let canonical = decoded(&encoding(true, roles, immediate, false, false));
                assert_eq!(
                    canonical,
                    decoded(&encoding(true, roles, immediate, false, true))
                );
                assert_ne!(
                    canonical,
                    decoded(&encoding(false, roles, immediate, false, false))
                );
            }
        }
    }
    for w in [false, true] {
        let mut bytes = encoding(true, (1, 2, 0), 8, true, w);
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
#[test]
fn ymm_half_controls_are_independent_with_ignored_high_bits_and_full_read() {
    let first = std::array::from_fn(|byte| byte as u8);
    let second = std::array::from_fn(|byte| byte as u8 + 32);
    let literal = words([
        128, 124, 120, 116, 112, 108, 104, 100, 144, 140, 136, 132, 128, 124, 120, 116,
    ]);
    assert_eq!(oracle(first, second, 8, true), literal);
    assert_eq!(oracle(first, second, 200, true), literal);
    assert_ne!(&literal[..16], &literal[16..]);
    let mut engine = engine();
    for immediate in 0_u8..=255 {
        for (destination, a, b) in aliases(immediate % 8) {
            let vectors = std::array::from_fn(pattern);
            let output = oracle(
                vectors[usize::from(a)],
                vectors[usize::from(b)],
                immediate,
                true,
            );
            let before = seed(&engine, &vectors);
            engine.arena_mut().unwrap().copy_from_slice(&before);
            request(
                &mut engine,
                0x01000892,
                u32::from(destination) | u32::from(a) << 3 | u32::from(b) << 6,
                u32::from(immediate),
                Some(output),
                0,
                false,
            );
        }
    }
    for immediate in [8, 200] {
        let mut vectors = [[0x91; 32]; 8];
        vectors[1] = first;
        vectors[2] = second;
        let before = seed(&engine, &vectors);
        engine.arena_mut().unwrap().copy_from_slice(&before);
        request(
            &mut engine,
            0x01000892,
            1 << 3 | 2 << 6,
            immediate,
            Some(literal),
            0,
            false,
        );
    }
    let operand = pattern(3);
    upload(&mut engine, 0x4fe0, &operand);
    for (address, fault) in [(0x4fe0_u32, false), (0x4fe1, true), (0x4ff0, true)] {
        let vectors = std::array::from_fn(pattern);
        let mut before = seed(&engine, &vectors);
        let metadata = &mut before[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568];
        metadata.fill(0);
        metadata[..12].copy_from_slice(&[b'R', b'3', b'A', b'M', 1, 0, 1, 0, 24, 0, 0, 0]);
        metadata[16..20].copy_from_slice(&3_u32.to_le_bytes());
        metadata[20..24].copy_from_slice(&address.to_le_bytes());
        engine.arena_mut().unwrap().copy_from_slice(&before);
        let output = (!fault).then(|| oracle(vectors[0], operand, 8, true));
        request(&mut engine, 0x01000892, 1 << 12, 8, output, 32, fault);
    }
}
#[test]
fn old_xmm_sad_keeps_low_control_and_upper_zeroing_for_every_immediate() {
    let mut engine = engine();
    for immediate in 0_u8..=255 {
        let vectors = std::array::from_fn(pattern);
        let before = seed(&engine, &vectors);
        engine.arena_mut().unwrap().copy_from_slice(&before);
        decoded(&encoding(false, (0, 1, 2), immediate, false, false));
        let output = oracle(vectors[1], vectors[2], immediate, false);
        request(
            &mut engine,
            0x01000812,
            1 << 3 | 2 << 6,
            u32::from(immediate),
            Some(output),
            0,
            false,
        );
    }
}
#[test]
fn existing_sixteen_ymm_codes_keep_admission_and_zero_results() {
    let mut engine = engine();
    for (kind, map, opcode) in [
        (1, 1, 0xd5),
        (2, 1, 0xe4),
        (3, 1, 0xe5),
        (4, 1, 0xf4),
        (5, 1, 0xf5),
        (6, 1, 0xf6),
        (7, 2, 1),
        (8, 2, 2),
        (9, 2, 3),
        (10, 2, 4),
        (11, 2, 5),
        (12, 2, 6),
        (13, 2, 7),
        (14, 2, 0x0b),
        (15, 2, 0x28),
        (16, 2, 0x40),
    ] {
        for w in [0, 0x80] {
            decoded(&[0xc4, 0xe0 | map, 0x6d | w, opcode, 0xcb]);
        }
        let mut vectors = [[0; 32]; 8];
        vectors[0] = [0x91; 32];
        let before = seed(&engine, &vectors);
        engine.arena_mut().unwrap().copy_from_slice(&before);
        request(
            &mut engine,
            0x01000880 | kind,
            1 << 3 | 2 << 6,
            0,
            Some([0; 32]),
            0,
            false,
        );
    }
}
#[test]
fn prefix_and_unassigned_unary_width_refusals_are_precise_and_neutral() {
    let mut engine = engine();
    let before_ram = ram(&engine);
    for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
        let mut bytes = vec![prefix];
        bytes.extend(encoding(true, (1, 2, 3), 8, false, false));
        crate::support_fetch::assert_decode_contract(
            decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
            DecodeError::InvalidEncoding,
        );
    }
    for (descriptor, operands, immediate) in [
        (0x01000891, 0, 0),
        (0x01000893, 0, 0),
        (0x01000892, 1 << 9, 8),
        (0x01000892, 1 << 13, 8),
        (0x01000892, 1 << 12 | 1 << 6, 8),
        (0x01000892, 0, 256),
        (0x01000812, 1 << 9, 8),
    ] {
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.avx_execute(descriptor, operands, immediate, 0, 0, FLAGS),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(ram(&engine), before_ram);
    }
}
