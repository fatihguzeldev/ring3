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
enum Direction {
    Left,
    Right,
    Arithmetic,
    BytesLeft,
    BytesRight,
}
#[derive(Clone, Copy)]
struct Form {
    opcode: u8,
    group: u8,
    kind: u8,
    lane: usize,
    direction: Direction,
}
const FORMS: [Form; 18] = [
    Form {
        opcode: 0x71,
        group: 2,
        kind: 1,
        lane: 2,
        direction: Direction::Right,
    },
    Form {
        opcode: 0x71,
        group: 4,
        kind: 2,
        lane: 2,
        direction: Direction::Arithmetic,
    },
    Form {
        opcode: 0x71,
        group: 6,
        kind: 3,
        lane: 2,
        direction: Direction::Left,
    },
    Form {
        opcode: 0x72,
        group: 2,
        kind: 4,
        lane: 4,
        direction: Direction::Right,
    },
    Form {
        opcode: 0x72,
        group: 4,
        kind: 5,
        lane: 4,
        direction: Direction::Arithmetic,
    },
    Form {
        opcode: 0x72,
        group: 6,
        kind: 6,
        lane: 4,
        direction: Direction::Left,
    },
    Form {
        opcode: 0x73,
        group: 2,
        kind: 7,
        lane: 8,
        direction: Direction::Right,
    },
    Form {
        opcode: 0x73,
        group: 3,
        kind: 8,
        lane: 1,
        direction: Direction::BytesRight,
    },
    Form {
        opcode: 0x73,
        group: 6,
        kind: 9,
        lane: 8,
        direction: Direction::Left,
    },
    Form {
        opcode: 0x73,
        group: 7,
        kind: 10,
        lane: 1,
        direction: Direction::BytesLeft,
    },
    Form {
        opcode: 0xd1,
        group: 0,
        kind: 11,
        lane: 2,
        direction: Direction::Right,
    },
    Form {
        opcode: 0xd2,
        group: 0,
        kind: 12,
        lane: 4,
        direction: Direction::Right,
    },
    Form {
        opcode: 0xd3,
        group: 0,
        kind: 13,
        lane: 8,
        direction: Direction::Right,
    },
    Form {
        opcode: 0xe1,
        group: 0,
        kind: 14,
        lane: 2,
        direction: Direction::Arithmetic,
    },
    Form {
        opcode: 0xe2,
        group: 0,
        kind: 15,
        lane: 4,
        direction: Direction::Arithmetic,
    },
    Form {
        opcode: 0xf1,
        group: 0,
        kind: 16,
        lane: 2,
        direction: Direction::Left,
    },
    Form {
        opcode: 0xf2,
        group: 0,
        kind: 17,
        lane: 4,
        direction: Direction::Left,
    },
    Form {
        opcode: 0xf3,
        group: 0,
        kind: 18,
        lane: 8,
        direction: Direction::Left,
    },
];
const FLAGS: u32 = 0x244cd7;

impl Form {
    fn immediate(self) -> bool {
        self.kind <= 10
    }
    fn boundary(self) -> u64 {
        if self.lane == 1 {
            16
        } else {
            self.lane as u64 * 8
        }
    }
    fn descriptor(self, wide: bool) -> u32 {
        0x01000700 | u32::from(self.kind) | if wide { 0x80 } else { 0 }
    }
}

fn encoding(
    form: Form,
    roles: (u8, u8, u8),
    immediate: u8,
    wide: bool,
    w: bool,
    two: bool,
) -> Vec<u8> {
    let (destination, source, count) = roles;
    let vvvv = 15
        - if form.immediate() {
            destination
        } else {
            source
        };
    let control = vvvv << 3 | if wide { 4 } else { 0 } | 1;
    let mut bytes = if two {
        assert!(!w);
        vec![0xc5, control | 0x80]
    } else {
        vec![0xc4, 0xe1, control | if w { 0x80 } else { 0 }]
    };
    let modrm = if form.immediate() {
        0xc0 | form.group << 3 | source
    } else {
        0xc0 | destination << 3 | count
    };
    bytes.extend([form.opcode, modrm]);
    if form.immediate() {
        bytes.push(immediate);
    }
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

fn aliases(form: Form, register: u8) -> Vec<(u8, u8, u8)> {
    let next = (register + 1) % 8;
    if form.immediate() {
        vec![(register, next, 0), (register, register, 0)]
    } else {
        vec![
            (register, next, (register + 2) % 8),
            (register, register, next),
            (register, next, register),
            (register, next, next),
            (register, register, register),
        ]
    }
}

#[test]
fn ymm_encodings_distinguish_immediate_and_register_count_roles() {
    let mut distinct = Vec::new();
    for form in FORMS {
        let sample = decoded(&encoding(form, (1, 2, 3), 1, true, false, true));
        assert!(!distinct.contains(&sample));
        distinct.push(sample);
        for register in 0..8 {
            for (destination, source, count) in aliases(form, register) {
                let roles = (destination, source, count);
                let canonical = decoded(&encoding(form, roles, 1, true, false, true));
                for w in [false, true] {
                    assert_eq!(
                        canonical,
                        decoded(&encoding(form, roles, 1, true, w, false))
                    );
                }
                assert_ne!(
                    canonical,
                    decoded(&encoding(form, roles, 1, false, false, true))
                );
                assert_ne!(
                    canonical,
                    decoded(&encoding(
                        form,
                        ((destination + 1) % 8, source, count),
                        1,
                        true,
                        false,
                        true
                    ))
                );
                assert_ne!(
                    canonical,
                    decoded(&encoding(
                        form,
                        (destination, (source + 1) % 8, count),
                        1,
                        true,
                        false,
                        true
                    ))
                );
                if form.immediate() {
                    assert_ne!(
                        canonical,
                        decoded(&encoding(form, roles, 2, true, false, true))
                    );
                } else {
                    assert_ne!(
                        canonical,
                        decoded(&encoding(
                            form,
                            (destination, source, (count + 1) % 8),
                            1,
                            true,
                            false,
                            true
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
fn pattern(form: Form, register: usize) -> [u8; 32] {
    if form.lane == 1 {
        return std::array::from_fn(|byte| (byte + 1 + register * 37) as u8);
    }
    let sign = 1_u128 << (form.lane * 8 - 1);
    let mask = sign * 2 - 1;
    let numbers = [
        mask,
        sign,
        sign - 1,
        1,
        0,
        mask - 1,
        sign + 1,
        mask / 3,
        2,
        mask / 2 + 3,
        mask - 3,
    ];
    std::array::from_fn(|byte| {
        numbers[(byte / form.lane + register * 3) % numbers.len()].to_le_bytes()[byte % form.lane]
    })
}

fn oracle(form: Form, source: [u8; 32], count: u64, wide: bool) -> [u8; 32] {
    let mut result = [0; 32];
    let bytes = if wide { 32 } else { 16 };
    if form.lane == 1 {
        for base in (0..bytes).step_by(16) {
            for index in 0..16 {
                let input = match form.direction {
                    Direction::BytesLeft => (index as u64).checked_sub(count),
                    Direction::BytesRight => (index as u64).checked_add(count).filter(|i| *i < 16),
                    _ => unreachable!(),
                };
                if let Some(input) = input {
                    result[base + index] = source[base + input as usize];
                }
            }
        }
        return result;
    }
    let bits = form.lane * 8;
    let modulus = 1_u128 << bits;
    for offset in (0..bytes).step_by(form.lane) {
        let value = source[offset..offset + form.lane]
            .iter()
            .rev()
            .fold(0_u128, |a, b| a * 256 + u128::from(*b));
        let shifted = match form.direction {
            Direction::Left if count < bits as u64 => value * (1_u128 << count) % modulus,
            Direction::Right if count < bits as u64 => value / (1_u128 << count),
            Direction::Left | Direction::Right => 0,
            Direction::Arithmetic => {
                let signed = if value >= modulus / 2 {
                    value as i128 - modulus as i128
                } else {
                    value as i128
                };
                (signed.div_euclid(1_i128 << count.min((bits - 1) as u64)) & (modulus as i128 - 1))
                    as u128
            }
            _ => unreachable!(),
        };
        result[offset..offset + form.lane].copy_from_slice(&shifted.to_le_bytes()[..form.lane]);
    }
    result
}

fn seed(engine: &mut EngineInstance, vectors: &[[u8; 32]; 8]) -> Vec<u8> {
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
        let mut simd =
            ring3_engine::abi::x86::decode_simd(&result[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE])
                .unwrap();
        let mut upper =
            ring3_engine::abi::x86::decode_ymm(&result[YMM_OFFSET..YMM_OFFSET + YMM_SIZE]).unwrap();
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
fn slots(form: Form, (destination, source, count): (u8, u8, u8)) -> u32 {
    u32::from(destination)
        | u32::from(source) << 3
        | if form.immediate() {
            0
        } else {
            u32::from(count) << 6
        }
}

fn helpers(wide: bool) {
    let mut engine = engine();
    let before_ram = ram(&engine);
    for form in FORMS {
        let counts = if form.immediate() {
            vec![0, 1, form.boundary() - 1, form.boundary(), 255]
        } else {
            vec![
                0,
                1,
                form.boundary() - 1,
                form.boundary(),
                form.boundary() + 1,
                1_u64 << 32,
                u64::MAX,
            ]
        };
        for count in counts {
            for register in 0..8 {
                for roles in aliases(form, register) {
                    let (destination, source, count_register) = roles;
                    for &poison in if form.immediate() {
                        &[0][..]
                    } else {
                        &[0, 255][..]
                    } {
                        let mut vectors = std::array::from_fn(|r| pattern(form, r));
                        if !form.immediate() {
                            vectors[usize::from(count_register)][..8]
                                .copy_from_slice(&count.to_le_bytes());
                            vectors[usize::from(count_register)][8..].fill(poison);
                        }
                        let value = oracle(form, vectors[usize::from(source)], count, wide);
                        let before = seed(&mut engine, &vectors);
                        engine.arena_mut().unwrap().copy_from_slice(&before);
                        let wanted =
                            expected(&before, usize::from(destination), Some(value), 0, false);
                        assert_eq!(
                            engine.avx_execute(
                                form.descriptor(wide),
                                slots(form, roles),
                                if form.immediate() { count as u32 } else { 0 },
                                0,
                                0,
                                FLAGS
                            ),
                            Ok(StoreCompletion::Complete),
                            "kind{} count{count} roles{roles:?}",
                            form.kind
                        );
                        assert_eq!(engine.arena(), wanted);
                        assert_eq!(ram(&engine), before_ram);
                    }
                }
            }
        }
    }
}

#[test]
fn ymm_helpers_use_one_complete_low64_count_for_both_halves() {
    helpers(true);
}

#[test]
fn ymm_literal_boundaries_keep_byte_groups_and_signed_fill() {
    let mut engine = engine();
    let before_ram = ram(&engine);
    for form in FORMS {
        for count in [1, form.boundary()] {
            let mut source = [255; 32];
            let value = if form.lane == 1 {
                source = std::array::from_fn(|i| i as u8 + 1);
                if count == 16 {
                    [0; 32]
                } else {
                    match form.direction {
                        Direction::BytesLeft => [
                            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 0, 17, 18, 19,
                            20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
                        ],
                        Direction::BytesRight => [
                            2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 0, 18, 19, 20, 21,
                            22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 0,
                        ],
                        _ => unreachable!(),
                    }
                }
            } else {
                let mask = u64::MAX >> (64 - form.lane * 8);
                let number = match form.direction {
                    Direction::Left => {
                        if count == 1 {
                            mask - 1
                        } else {
                            0
                        }
                    }
                    Direction::Right => {
                        if count == 1 {
                            mask / 2
                        } else {
                            0
                        }
                    }
                    Direction::Arithmetic => mask,
                    _ => unreachable!(),
                };
                std::array::from_fn(|byte| number.to_le_bytes()[byte % form.lane])
            };
            assert_eq!(oracle(form, source, count, true), value);
            let mut vectors = [[0xa5; 32]; 8];
            vectors[1] = source;
            vectors[2][..8].copy_from_slice(&count.to_le_bytes());
            let before = seed(&mut engine, &vectors);
            engine.arena_mut().unwrap().copy_from_slice(&before);
            let wanted = expected(&before, 0, Some(value), 0, false);
            assert_eq!(
                engine.avx_execute(
                    form.descriptor(true),
                    slots(form, (0, 1, 2)),
                    if form.immediate() { count as u32 } else { 0 },
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
fn m128_counts_capture_without_reads_and_fault_on_sixteenth_byte() {
    for form in FORMS.into_iter().filter(|form| !form.immediate()) {
        let mut bytes = encoding(form, (1, 2, 0), 0, true, false, false);
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
    }
    let mut engine = engine();
    for form in FORMS.into_iter().filter(|form| !form.immediate()) {
        for count in [0, 1, u64::MAX] {
            let mut data = [255; 16];
            data[..8].copy_from_slice(&count.to_le_bytes());
            upload(&mut engine, 0x4ff0, &data);
            let before_ram = ram(&engine);
            for (address, fault) in [(0x4ff0_u32, false), (0x4ff1, true), (0x4ff8, true)] {
                let vectors = std::array::from_fn(|r| pattern(form, r));
                let mut before = seed(&mut engine, &vectors);
                let metadata = &mut before[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568];
                metadata.fill(0);
                metadata[..12].copy_from_slice(&[b'R', b'3', b'A', b'M', 1, 0, 1, 0, 24, 0, 0, 0]);
                metadata[16..20].copy_from_slice(&3_u32.to_le_bytes());
                metadata[20..24].copy_from_slice(&address.to_le_bytes());
                engine.arena_mut().unwrap().copy_from_slice(&before);
                let value = oracle(form, vectors[0], count, true);
                let wanted = expected(
                    &before,
                    0,
                    if fault { None } else { Some(value) },
                    16,
                    fault,
                );
                assert_eq!(
                    engine.avx_execute(form.descriptor(true), 1 << 12, 0, 0, 0, FLAGS),
                    Ok(StoreCompletion::Complete)
                );
                assert_eq!(
                    engine.arena(),
                    wanted,
                    "kind{} address{address:08x} count{count}",
                    form.kind
                );
                assert_eq!(ram(&engine), before_ram);
            }
        }
    }
}

#[test]
fn all_xmm_forms_preserve_legacy_results_and_upper_zeroing() {
    for form in FORMS {
        let canonical = decoded(&encoding(form, (7, 6, 5), 1, false, false, true));
        for w in [false, true] {
            assert_eq!(
                canonical,
                decoded(&encoding(form, (7, 6, 5), 1, false, w, false))
            );
        }
    }
    helpers(false);
}

#[test]
fn reserved_prefixes_and_immediate_memory_forms_remain_invalid() {
    for form in FORMS {
        for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
            let mut bytes = vec![prefix];
            bytes.extend(encoding(form, (1, 2, 3), 1, true, false, false));
            crate::support_fetch::assert_decode_contract(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding,
            );
        }
        if form.immediate() {
            let mut bytes = encoding(form, (1, 2, 0), 1, false, false, false);
            bytes[4] = form.group << 3;
            crate::support_fetch::assert_decode_contract(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding,
            );
        }
    }
}

#[test]
fn legacy_slots_and_malformed_wide_requests_refuse_without_publication() {
    let mut engine = engine();
    let before_ram = ram(&engine);
    for form in FORMS {
        let mut invalid = vec![(1 << 9, 0), (1 << 13, 0), (1 << 12 | 1 << 6, 0)];
        if form.immediate() {
            invalid.extend([(1 << 6, 0), (1 << 12, 0)]);
        } else {
            invalid.extend([(0, 1), (0, 255)]);
        }
        for wide in [false, true] {
            for &(operands, immediate) in &invalid {
                let before = engine.arena().to_vec();
                assert_eq!(
                    engine.avx_execute(form.descriptor(wide), operands, immediate, 0, 0, FLAGS),
                    Err(HostError::InvalidRequest)
                );
                assert_eq!(engine.arena(), before);
                assert_eq!(ram(&engine), before_ram);
            }
        }
    }
}
