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

#[derive(Clone, Copy)]
enum Direction {
    Left,
    Right,
    Arithmetic,
}
#[derive(Clone, Copy)]
struct Form {
    opcode: u8,
    kind: u8,
    lane: usize,
    direction: Direction,
}
const FORMS: [Form; 5] = [
    Form {
        opcode: 0x45,
        kind: 19,
        lane: 4,
        direction: Direction::Right,
    },
    Form {
        opcode: 0x45,
        kind: 20,
        lane: 8,
        direction: Direction::Right,
    },
    Form {
        opcode: 0x46,
        kind: 21,
        lane: 4,
        direction: Direction::Arithmetic,
    },
    Form {
        opcode: 0x47,
        kind: 22,
        lane: 4,
        direction: Direction::Left,
    },
    Form {
        opcode: 0x47,
        kind: 23,
        lane: 8,
        direction: Direction::Left,
    },
];
const FLAGS: u32 = 0x244cd7;

impl Form {
    fn descriptor(self, wide: bool) -> u32 {
        0x01000700 | u32::from(self.kind) | if wide { 0x80 } else { 0 }
    }
}
fn encoding(
    form: Form,
    (destination, source, count): (u8, u8, u8),
    wide: bool,
    memory: bool,
) -> Vec<u8> {
    vec![
        0xc4,
        0xe2,
        (15 - source) << 3 | if wide { 4 } else { 0 } | if form.lane == 8 { 0x80 } else { 0 } | 1,
        form.opcode,
        if memory {
            destination << 3
        } else {
            0xc0 | destination << 3 | count
        },
    ]
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

#[test]
fn encodings_select_element_width_and_three_independent_register_roles() {
    let mut forms = Vec::new();
    for form in FORMS {
        for wide in [false, true] {
            let operation = decoded(&encoding(form, (1, 2, 3), wide, false));
            assert!(!forms.contains(&operation));
            forms.push(operation);
            for register in 0..8 {
                for roles in aliases(register) {
                    let (destination, source, count) = roles;
                    let canonical = decoded(&encoding(form, roles, wide, false));
                    for altered in [
                        ((destination + 1) % 8, source, count),
                        (destination, (source + 1) % 8, count),
                        (destination, source, (count + 1) % 8),
                    ] {
                        assert_ne!(canonical, decoded(&encoding(form, altered, wide, false)));
                    }
                    assert_ne!(canonical, decoded(&encoding(form, roles, !wide, false)));
                }
            }
            let register = decoded(&encoding(form, (1, 2, 3), wide, false));
            let memory = decoded(&encoding(form, (1, 2, 0), wide, true));
            assert_ne!(register, memory);
        }
    }
    assert_eq!(forms.len(), 10);
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
fn pattern(form: Form, register: usize) -> [u8; 32] {
    let sign = 1_u128 << (form.lane * 8 - 1);
    let mask = sign * 2 - 1;
    let values = [
        mask,
        sign,
        sign - 1,
        1,
        0,
        mask - 1,
        sign + 1,
        mask / 3,
        0x12345678,
    ];
    std::array::from_fn(|byte| {
        values[(byte / form.lane + register * 3) % values.len()].to_le_bytes()[byte % form.lane]
    })
}
fn counts(form: Form, phase: usize) -> [u8; 32] {
    let bits = form.lane as u64 * 8;
    let values = [
        0,
        1,
        bits - 1,
        bits,
        bits + 1,
        1 << 31,
        u64::MAX,
        1 << 32,
        1 << 63,
    ];
    std::array::from_fn(|byte| {
        values[(byte / form.lane + phase) % values.len()].to_le_bytes()[byte % form.lane]
    })
}
fn oracle(form: Form, source: [u8; 32], counts: [u8; 32], wide: bool) -> [u8; 32] {
    let mut result = [0; 32];
    let bits = form.lane * 8;
    let modulus = 1_u128 << bits;
    for offset in (0..if wide { 32 } else { 16 }).step_by(form.lane) {
        let unsigned = |bytes: &[u8]| {
            bytes
                .iter()
                .rev()
                .fold(0_u128, |value, byte| value * 256 + u128::from(*byte))
        };
        let value = unsigned(&source[offset..offset + form.lane]);
        let count = unsigned(&counts[offset..offset + form.lane]);
        let value = match form.direction {
            Direction::Left if count < bits as u128 => value * (1_u128 << count) % modulus,
            Direction::Right if count < bits as u128 => value / (1_u128 << count),
            Direction::Left | Direction::Right => 0,
            Direction::Arithmetic => {
                let signed = if value >= modulus / 2 {
                    value as i128 - modulus as i128
                } else {
                    value as i128
                };
                (signed.div_euclid(1_i128 << count.min((bits - 1) as u128)) & (modulus as i128 - 1))
                    as u128
            }
        };
        result[offset..offset + form.lane].copy_from_slice(&value.to_le_bytes()[..form.lane]);
    }
    result
}
fn seed(engine: &EngineInstance, vectors: &[[u8; 32]; 8]) -> Vec<u8> {
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
    value: Option<[u8; 32]>,
    length: u32,
    fault: bool,
) {
    let before = engine.arena().to_vec();
    let before_ram = ram(engine);
    let wanted = expected(&before, (operands & 7) as usize, value, length, fault);
    assert_eq!(
        engine.avx_execute(descriptor, operands, 0, 0, 0, FLAGS),
        Ok(StoreCompletion::Complete)
    );
    assert_eq!(engine.arena(), wanted);
    assert_eq!(ram(engine), before_ram);
}

#[test]
fn element_counts_capture_both_sources_before_any_aliased_publication() {
    let mut engine = engine();
    for form in FORMS {
        for wide in [false, true] {
            for register in 0..8 {
                for (destination, source, count) in aliases(register) {
                    for phase in [0, 3, 6] {
                        let mut vectors = std::array::from_fn(|r| pattern(form, r));
                        vectors[usize::from(count)] = counts(form, phase);
                        let value = oracle(
                            form,
                            vectors[usize::from(source)],
                            vectors[usize::from(count)],
                            wide,
                        );
                        let before = seed(&engine, &vectors);
                        engine.arena_mut().unwrap().copy_from_slice(&before);
                        request(
                            &mut engine,
                            form.descriptor(wide),
                            u32::from(destination) | u32::from(source) << 3 | u32::from(count) << 6,
                            Some(value),
                            0,
                            false,
                        );
                    }
                }
            }
        }
    }
}
fn words(values: &[u32]) -> [u8; 32] {
    let mut result = [0; 32];
    for (lane, value) in values.iter().enumerate() {
        result[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    result
}
fn qwords(values: &[u64]) -> [u8; 32] {
    let mut result = [0; 32];
    for (lane, value) in values.iter().enumerate() {
        result[lane * 8..lane * 8 + 8].copy_from_slice(&value.to_le_bytes());
    }
    result
}
#[test]
fn literal_unsigned_boundaries_do_not_mask_or_truncate_each_count() {
    let dword_data = words(&[
        u32::MAX,
        0x80000000,
        0x7fffffff,
        1,
        0x7fffffff,
        0x80000000,
        u32::MAX,
        0x12345678,
    ]);
    let dword_counts = words(&[0, 1, 31, 32, 0x80000000, u32::MAX, 33, 4]);
    let qword_data = qwords(&[
        u64::MAX,
        0x8000000000000000,
        0x7fffffffffffffff,
        0x123456789abcdef0,
    ]);
    let qword_counts = qwords(&[1, 63, 1 << 32, u64::MAX]);
    let mut engine = engine();
    for form in FORMS {
        let (source, counts, literal) = match form.kind {
            19 => (
                dword_data,
                dword_counts,
                words(&[u32::MAX, 0x40000000, 0, 0, 0, 0, 0, 0x01234567]),
            ),
            20 => (
                qword_data,
                qword_counts,
                qwords(&[0x7fffffffffffffff, 1, 0, 0]),
            ),
            21 => (
                dword_data,
                dword_counts,
                words(&[
                    u32::MAX,
                    0xc0000000,
                    0,
                    0,
                    0,
                    u32::MAX,
                    u32::MAX,
                    0x01234567,
                ]),
            ),
            22 => (
                dword_data,
                dword_counts,
                words(&[u32::MAX, 0, 0x80000000, 0, 0, 0, 0, 0x23456780]),
            ),
            23 => (
                qword_data,
                qword_counts,
                qwords(&[0xfffffffffffffffe, 0, 0, 0]),
            ),
            _ => unreachable!(),
        };
        for wide in [false, true] {
            let mut wanted = literal;
            if !wide {
                wanted[16..].fill(0);
            }
            assert_eq!(oracle(form, source, counts, wide), wanted);
            let mut vectors = [[0xa5; 32]; 8];
            vectors[1] = source;
            vectors[2] = counts;
            let before = seed(&engine, &vectors);
            engine.arena_mut().unwrap().copy_from_slice(&before);
            request(
                &mut engine,
                form.descriptor(wide),
                1 << 3 | 2 << 6,
                Some(wanted),
                0,
                false,
            );
        }
    }
}

#[test]
fn count_memory_captures_exact_vector_width_and_faults_before_publication() {
    for form in FORMS {
        for wide in [false, true] {
            let mut bytes = encoding(form, (1, 2, 0), wide, true);
            bytes[4] = 0x0d;
            bytes.extend([0xef, 0xbe, 0xad, 0xde, 0xeb, 0xfe]);
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

            let mut engine = engine();
            let length = if wide { 32 } else { 16 };
            let input = counts(form, 0);
            let address = 0x5000 - length;
            upload(&mut engine, address, &input[..length as usize]);
            for (address, fault) in [(address, false), (address + 1, true), (0x4ff8, true)] {
                let vectors = std::array::from_fn(|r| pattern(form, r));
                let mut before = seed(&engine, &vectors);
                let metadata = &mut before[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568];
                metadata.fill(0);
                metadata[..12].copy_from_slice(&[b'R', b'3', b'A', b'M', 1, 0, 1, 0, 24, 0, 0, 0]);
                metadata[16..20].copy_from_slice(&3_u32.to_le_bytes());
                metadata[20..24].copy_from_slice(&address.to_le_bytes());
                engine.arena_mut().unwrap().copy_from_slice(&before);
                let value = (!fault).then(|| oracle(form, vectors[0], input, wide));
                request(
                    &mut engine,
                    form.descriptor(wide),
                    1 << 12,
                    value,
                    length,
                    fault,
                );
            }
        }
    }
}

#[test]
fn existing_uniform_widths_keep_one_count_and_per_group_byte_shifts() {
    let mut engine = engine();
    for (opcode, group, kind, lane, direction) in [
        (0x71, 2, 1, 2, 0),
        (0x71, 4, 2, 2, 1),
        (0x71, 6, 3, 2, 2),
        (0x72, 2, 4, 4, 0),
        (0x72, 4, 5, 4, 1),
        (0x72, 6, 6, 4, 2),
        (0x73, 2, 7, 8, 0),
        (0x73, 3, 8, 1, 3),
        (0x73, 6, 9, 8, 2),
        (0x73, 7, 10, 1, 4),
        (0xd1, 0, 11, 2, 0),
        (0xd2, 0, 12, 4, 0),
        (0xd3, 0, 13, 8, 0),
        (0xe1, 0, 14, 2, 1),
        (0xe2, 0, 15, 4, 1),
        (0xf1, 0, 16, 2, 2),
        (0xf2, 0, 17, 4, 2),
        (0xf3, 0, 18, 8, 2),
    ] {
        for wide in [false, true] {
            let bytes = if kind <= 10 {
                vec![
                    0xc5,
                    0xf9 | if wide { 4 } else { 0 },
                    opcode,
                    0xc0 | group << 3 | 1,
                    1,
                ]
            } else {
                vec![0xc5, 0xf1 | if wide { 4 } else { 0 }, opcode, 0xc2]
            };
            decoded(&bytes);
            let mut value = [0; 32];
            let mut vectors = [[255; 32]; 8];
            if lane == 1 {
                vectors[1] = std::array::from_fn(|byte| byte as u8 + 1);
                for base in (0..if wide { 32 } else { 16 }).step_by(16) {
                    for byte in 0_usize..16 {
                        let input = if direction == 3 {
                            (byte + 1 < 16).then_some(byte + 1)
                        } else {
                            byte.checked_sub(1)
                        };
                        if let Some(input) = input {
                            value[base + byte] = vectors[1][base + input];
                        }
                    }
                }
            } else {
                let mask = u64::MAX >> (64 - lane * 8);
                let result = match direction {
                    0 => mask / 2,
                    1 => mask,
                    2 => mask - 1,
                    _ => unreachable!(),
                };
                for (byte, output) in value[..if wide { 32 } else { 16 }].iter_mut().enumerate() {
                    *output = result.to_le_bytes()[byte % lane];
                }
            }
            vectors[2][..8].copy_from_slice(&1_u64.to_le_bytes());
            let before = seed(&engine, &vectors);
            engine.arena_mut().unwrap().copy_from_slice(&before);
            let operands = 1 << 3 | if kind <= 10 { 0 } else { 2 << 6 };
            let descriptor = 0x01000700 | kind | if wide { 0x80 } else { 0 };
            let before_ram = ram(&engine);
            let wanted = expected(&before, 0, Some(value), 0, false);
            assert_eq!(
                engine.avx_execute(
                    descriptor,
                    operands,
                    if kind <= 10 { 1 } else { 0 },
                    0,
                    0,
                    FLAGS
                ),
                Ok(StoreCompletion::Complete)
            );
            assert_eq!(engine.arena(), wanted);
            assert_eq!(ram(&engine), before_ram);
        }
    }
}

#[test]
fn forbidden_legacy_prefixes_remain_invalid_for_both_vector_lengths() {
    for form in FORMS {
        for wide in [false, true] {
            for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
                let mut bytes = vec![prefix];
                bytes.extend(encoding(form, (1, 2, 3), wide, false));
                crate::support_fetch::assert_decode_contract(
                    decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                    DecodeError::InvalidEncoding,
                );
            }
        }
    }
}

#[test]
fn malformed_slots_immediates_and_unknown_kinds_refuse_neutrally() {
    let mut engine = engine();
    let before_ram = ram(&engine);
    for form in FORMS {
        for wide in [false, true] {
            for (descriptor, operands, immediate) in [
                (form.descriptor(wide), 1 << 9, 0),
                (form.descriptor(wide), 1 << 13, 0),
                (form.descriptor(wide), 1 << 12 | 1 << 6, 0),
                (form.descriptor(wide), 0, 1),
                (form.descriptor(wide), 0, 255),
                (form.descriptor(wide) | 0x40, 0, 0),
                (0x02000700 | u32::from(form.kind), 0, 0),
                (0x01000718 | if wide { 0x80 } else { 0 }, 0, 0),
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
    }
}
