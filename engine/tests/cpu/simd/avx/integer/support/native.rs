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

#[derive(Clone, Copy)]
pub struct Form {
    pub map: u8,
    pub opcode: u8,
    pub kind: u8,
    pub lane: usize,
    pub unary: bool,
}

pub type Oracle = fn(Form, u64, u64) -> u64;
pub type Literals = ([u64; 8], [u64; 8], [u64; 8]);
const FLAGS: u32 = 0x244cd7;

pub fn encoding(
    form: Form,
    destination: u8,
    first: u8,
    second: u8,
    wide: bool,
    w: bool,
    two_byte: bool,
) -> Vec<u8> {
    let vvvv = if form.unary { 15 } else { 15 - first };
    let control = vvvv << 3 | if wide { 4 } else { 0 } | 1;
    let mut bytes = if two_byte {
        assert_eq!(form.map, 1);
        assert!(!w);
        vec![0xc5, control | 0x80]
    } else {
        vec![0xc4, 0xe0 | form.map, control | if w { 0x80 } else { 0 }]
    };
    bytes.extend([form.opcode, 0xc0 | destination << 3 | second]);
    bytes
}

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
    assert!(
        matches!(instruction.operation(), Operation::Avx(_)),
        "{bytes:02x?}"
    );
    assert_eq!(usize::from(instruction.length()), bytes.len());
    assert_eq!(
        instruction.next_pc(),
        GuestAddress(0x1000 + bytes.len() as u32)
    );
    assert!(memory.is_code_current(instruction.code_snapshot()));
    *instruction.operation()
}

fn aliases(register: u8, unary: bool) -> Vec<(u8, u8, u8)> {
    let next = (register + 1) % 8;
    if unary {
        vec![(register, 0, next), (register, 0, register)]
    } else {
        let third = (register + 2) % 8;
        vec![
            (register, next, third),
            (register, register, next),
            (register, next, register),
            (register, next, next),
            (register, register, register),
        ]
    }
}

pub fn register_encodings(forms: &[Form], wide: bool) {
    let mut distinct = Vec::new();
    for &form in forms {
        let sample = decoded(&encoding(form, 1, 2, 3, wide, false, false));
        assert!(!distinct.contains(&sample));
        distinct.push(sample);
        for register in 0..8 {
            for (destination, first, second) in aliases(register, form.unary) {
                let canonical = decoded(&encoding(
                    form,
                    destination,
                    first,
                    second,
                    wide,
                    false,
                    false,
                ));
                assert_eq!(
                    canonical,
                    decoded(&encoding(
                        form,
                        destination,
                        first,
                        second,
                        wide,
                        true,
                        false
                    ))
                );
                if form.map == 1 {
                    assert_eq!(
                        canonical,
                        decoded(&encoding(
                            form,
                            destination,
                            first,
                            second,
                            wide,
                            false,
                            true
                        ))
                    );
                }
                if wide {
                    assert_ne!(
                        canonical,
                        decoded(&encoding(
                            form,
                            destination,
                            first,
                            second,
                            false,
                            false,
                            false
                        ))
                    );
                }
                assert_ne!(
                    canonical,
                    decoded(&encoding(
                        form,
                        (destination + 1) % 8,
                        first,
                        second,
                        wide,
                        false,
                        false
                    ))
                );
                assert_ne!(
                    canonical,
                    decoded(&encoding(
                        form,
                        destination,
                        first,
                        (second + 1) % 8,
                        wide,
                        false,
                        false
                    ))
                );
                if !form.unary {
                    assert_ne!(
                        canonical,
                        decoded(&encoding(
                            form,
                            destination,
                            (first + 1) % 8,
                            second,
                            wide,
                            false,
                            false
                        ))
                    );
                }
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

fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(2, 91).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x4000, 1, 3).unwrap();
    upload(&mut engine, 0x1000, &[0xeb, 0xfe]);
    for offset in (0..4096).step_by(512) {
        let data: Vec<u8> = (0..512)
            .map(|byte| (byte * 37 + offset + 19) as u8)
            .collect();
        upload(&mut engine, 0x4000 + offset as u32, &data);
    }
    describe(&mut engine, 2);
    engine.compile(1).unwrap();
    engine
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
        .read(GuestAddress(0x4000), &mut bytes[4096..])
        .unwrap();
    bytes
}

fn pattern(lane: usize, register: usize) -> [u8; 32] {
    let mask = u64::MAX >> (64 - lane * 8);
    let sign = 1_u64 << (lane * 8 - 1);
    let numbers = [
        0,
        mask,
        sign,
        sign - 1,
        1,
        mask - 1,
        sign + 1,
        mask / 3,
        2,
        mask / 2 + 3,
        mask - 3,
    ];
    std::array::from_fn(|byte| {
        numbers[(byte / lane + register * 3) % numbers.len()].to_le_bytes()[byte % lane]
    })
}

fn pack(lane: usize, numbers: &[u64; 8], start: usize) -> [u8; 32] {
    std::array::from_fn(|byte| {
        numbers[(byte / lane + start) % numbers.len()].to_le_bytes()[byte % lane]
    })
}

fn expected_value(
    form: Form,
    first: &[u8; 32],
    second: &[u8; 32],
    wide: bool,
    oracle: Oracle,
) -> [u8; 32] {
    let mut result = [0; 32];
    let bytes = if wide { 32 } else { 16 };
    for offset in (0..bytes).step_by(form.lane) {
        let read = |source: &[u8; 32]| {
            source[offset..offset + form.lane]
                .iter()
                .rev()
                .fold(0_u64, |value, byte| value * 256 + u64::from(*byte))
        };
        result[offset..offset + form.lane]
            .copy_from_slice(&oracle(form, read(first), read(second)).to_le_bytes()[..form.lane]);
    }
    result
}

fn seeded(engine: &mut EngineInstance, vectors: &[[u8; 32]; 8]) -> Vec<u8> {
    let simd = SimdState {
        mxcsr: 0xffff,
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

fn expected_frame(
    before: &[u8],
    destination: usize,
    value: Option<&[u8; 32]>,
    length: u32,
    fault: bool,
) -> Vec<u8> {
    let mut expected = before.to_vec();
    if let Some(value) = value {
        let mut simd =
            ring3_engine::abi::x86::decode_simd(&expected[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE])
                .unwrap();
        let mut upper =
            ring3_engine::abi::x86::decode_ymm(&expected[YMM_OFFSET..YMM_OFFSET + YMM_SIZE])
                .unwrap();
        simd.xmm[destination].copy_from_slice(&value[..16]);
        upper.registers[destination].copy_from_slice(&value[16..]);
        encode_simd(&simd, &mut expected[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
        encode_ymm(&upper, &mut expected[YMM_OFFSET..YMM_OFFSET + YMM_SIZE]).unwrap();
        expected[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 544].fill(0);
        expected[TRANSFER_OFFSET + 516..TRANSFER_OFFSET + 520]
            .copy_from_slice(&FLAGS.to_le_bytes());
    }
    let packet = &mut expected[HELPER_OFFSET..HELPER_OFFSET + 40];
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
    expected
}

fn descriptor(form: Form, wide: bool) -> u32 {
    0x01000500 | u32::from(form.kind) | if wide { 0x80 } else { 0 }
}

fn operands(form: Form, destination: u8, first: u8, second: u8) -> u32 {
    u32::from(destination)
        | if form.unary { 0 } else { u32::from(first) << 3 }
        | u32::from(second) << 6
}

pub fn helper_registers(forms: &[Form], wide: bool, oracle: Oracle) {
    let mut engine = engine();
    let before_ram = ram(&engine);
    for &form in forms {
        for register in 0..8 {
            for (destination, first, second) in aliases(register, form.unary) {
                let vectors = std::array::from_fn(|r| pattern(form.lane, r));
                let before = seeded(&mut engine, &vectors);
                engine.arena_mut().unwrap().copy_from_slice(&before);
                let value = expected_value(
                    form,
                    &vectors[usize::from(first)],
                    &vectors[usize::from(second)],
                    wide,
                    oracle,
                );
                let expected =
                    expected_frame(&before, usize::from(destination), Some(&value), 0, false);
                assert_eq!(
                    engine.avx_execute(
                        descriptor(form, wide),
                        operands(form, destination, first, second),
                        0,
                        0,
                        0,
                        FLAGS
                    ),
                    Ok(StoreCompletion::Complete),
                    "kind{} roles{destination}/{first}/{second}",
                    form.kind
                );
                assert_eq!(engine.arena(), expected, "full arena kind{}", form.kind);
                assert_eq!(ram(&engine), before_ram, "all mapped RAM kind{}", form.kind);
            }
        }
    }
}

pub fn literal_values(forms: &[Form], oracle: Oracle, literals: fn(Form) -> Literals) {
    let mut engine = engine();
    let before_ram = ram(&engine);
    for &form in forms {
        let (first, second, result) = literals(form);
        for start in [0, 4] {
            let a = pack(form.lane, &first, start);
            let b = pack(form.lane, &second, start);
            let value = pack(form.lane, &result, start);
            assert_eq!(
                expected_value(form, &a, &b, true, oracle),
                value,
                "literal oracle kind{}",
                form.kind
            );
            let mut vectors = std::array::from_fn(|r| pattern(form.lane, r));
            vectors[1] = a;
            vectors[2] = b;
            let before = seeded(&mut engine, &vectors);
            engine.arena_mut().unwrap().copy_from_slice(&before);
            let expected = expected_frame(&before, 0, Some(&value), 0, false);
            assert_eq!(
                engine.avx_execute(
                    descriptor(form, true),
                    operands(form, 0, 1, 2),
                    0,
                    0,
                    0,
                    FLAGS
                ),
                Ok(StoreCompletion::Complete)
            );
            assert_eq!(
                engine.arena(),
                expected,
                "literal kind{} start{start}",
                form.kind
            );
            assert_eq!(ram(&engine), before_ram);
        }
    }
}

pub fn memory_capture_and_faults(forms: &[Form], oracle: Oracle) {
    for &form in forms {
        let mut distinct = Vec::new();
        for (prefix, tail) in [
            (None, &[0x08][..]),
            (None, &[0x4c, 0x24, 0x7f][..]),
            (None, &[0x0d, 0xef, 0xbe, 0xad, 0xde][..]),
            (Some(0x64), &[0x08][..]),
            (Some(0x67), &[0x0a][..]),
        ] {
            let mut bytes: Vec<u8> = prefix.into_iter().collect();
            let vvvv = if form.unary { 15 } else { 13 };
            bytes.extend([0xc4, 0xe0 | form.map, vvvv << 3 | 5, form.opcode]);
            bytes.extend(tail);
            let operation = decoded(&bytes);
            let mut w_alias = bytes.clone();
            w_alias[usize::from(prefix.is_some()) + 2] |= 0x80;
            assert_eq!(operation, decoded(&w_alias));
            assert!(!distinct.contains(&operation));
            distinct.push(operation);
        }
        let mut bytes = encoding(form, 1, 2, 0, true, false, false);
        bytes[4] = 0x0d;
        bytes.extend([0xef, 0xbe, 0xad, 0xde, 0xeb, 0xfe]);
        let mut capture = EngineInstance::new(1, 91).unwrap();
        capture.map(0x1000, 1, 7).unwrap();
        upload(&mut capture, 0x1000, &bytes);
        describe(&mut capture, bytes.len());
        let before = capture.arena().to_vec();
        assert_eq!(capture.compile(1), Ok(1));
        assert_eq!(capture.arena(), before);
        assert_eq!(&capture.artifact_bytes().unwrap()[..8], b"\0asm\x01\0\0\0");
        assert_eq!(capture.guard(91, 1), Ok(()));
        upload(&mut capture, 0x1000, &[0x90]);
        assert!(capture.guard(91, 1).is_err());
    }
    let mut engine = engine();
    let data: [u8; 32] = std::array::from_fn(|byte| (byte * 37 + 19) as u8);
    upload(&mut engine, 0x4fe0, &data);
    let before_ram = ram(&engine);
    for &form in forms {
        for (address, fault) in [(0x4fe0_u32, false), (0x4fe1, true)] {
            let vectors = std::array::from_fn(|r| pattern(form.lane, r));
            let mut before = seeded(&mut engine, &vectors);
            let metadata = &mut before[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568];
            metadata.fill(0);
            metadata[..12].copy_from_slice(&[b'R', b'3', b'A', b'M', 1, 0, 1, 0, 24, 0, 0, 0]);
            metadata[16..20].copy_from_slice(&3_u32.to_le_bytes());
            metadata[20..24].copy_from_slice(&address.to_le_bytes());
            engine.arena_mut().unwrap().copy_from_slice(&before);
            let value = expected_value(form, &vectors[0], &data, true, oracle);
            let expected = expected_frame(
                &before,
                0,
                if fault { None } else { Some(&value) },
                32,
                fault,
            );
            assert_eq!(
                engine.avx_execute(descriptor(form, true), 1 << 12, 0, 0, 0, FLAGS),
                Ok(StoreCompletion::Complete)
            );
            assert_eq!(
                engine.arena(),
                expected,
                "kind{} address{address:08x}",
                form.kind
            );
            assert_eq!(ram(&engine), before_ram);
        }
    }
}

pub fn malformed_controls(forms: &[Form]) {
    let mut engine = engine();
    let before_ram = ram(&engine);
    for &form in forms {
        for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
            let mut bytes = vec![prefix];
            bytes.extend(encoding(form, 1, 2, 3, true, false, false));
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding,
                "{bytes:02x?}"
            );
        }
        if form.unary {
            let mut bytes = encoding(form, 1, 0, 3, true, false, false);
            bytes[2] &= !8;
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding,
                "reserved vvvv {bytes:02x?}"
            );
        }
        let mut invalid = vec![
            (1 << 9, 0),
            (1 << 13, 0),
            (1 << 12 | 1 << 6, 0),
            (0, 1),
            (0, 255),
        ];
        if form.unary {
            invalid.push((1 << 3, 0));
        }
        for (slots, immediate) in invalid {
            let before = engine.arena().to_vec();
            assert_eq!(
                engine.avx_execute(descriptor(form, true), slots, immediate, 0, 0, FLAGS),
                Err(HostError::InvalidRequest)
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(ram(&engine), before_ram);
        }
    }
}
