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

const KEY: u64 = 91;
const FLAGS: u32 = 0x244cd7;

#[derive(Clone, Copy)]
struct Form {
    opcode: u8,
    map: u8,
    pp: u8,
    kind: u8,
}
const FORMS: [Form; 19] = [
    Form {
        opcode: 0x60,
        map: 1,
        pp: 1,
        kind: 1,
    },
    Form {
        opcode: 0x61,
        map: 1,
        pp: 1,
        kind: 2,
    },
    Form {
        opcode: 0x62,
        map: 1,
        pp: 1,
        kind: 3,
    },
    Form {
        opcode: 0x63,
        map: 1,
        pp: 1,
        kind: 4,
    },
    Form {
        opcode: 0x67,
        map: 1,
        pp: 1,
        kind: 5,
    },
    Form {
        opcode: 0x68,
        map: 1,
        pp: 1,
        kind: 6,
    },
    Form {
        opcode: 0x69,
        map: 1,
        pp: 1,
        kind: 7,
    },
    Form {
        opcode: 0x6a,
        map: 1,
        pp: 1,
        kind: 8,
    },
    Form {
        opcode: 0x6b,
        map: 1,
        pp: 1,
        kind: 9,
    },
    Form {
        opcode: 0x6c,
        map: 1,
        pp: 1,
        kind: 10,
    },
    Form {
        opcode: 0x6d,
        map: 1,
        pp: 1,
        kind: 11,
    },
    Form {
        opcode: 0x70,
        map: 1,
        pp: 1,
        kind: 12,
    },
    Form {
        opcode: 0x70,
        map: 1,
        pp: 2,
        kind: 13,
    },
    Form {
        opcode: 0x70,
        map: 1,
        pp: 3,
        kind: 14,
    },
    Form {
        opcode: 0x00,
        map: 2,
        pp: 1,
        kind: 15,
    },
    Form {
        opcode: 0x2b,
        map: 2,
        pp: 1,
        kind: 22,
    },
    Form {
        opcode: 0x0e,
        map: 3,
        pp: 1,
        kind: 29,
    },
    Form {
        opcode: 0x0f,
        map: 3,
        pp: 1,
        kind: 30,
    },
    Form {
        opcode: 0x4c,
        map: 3,
        pp: 1,
        kind: 31,
    },
];

impl Form {
    fn unary(self) -> bool {
        matches!(self.kind, 12..=14)
    }
    fn immediate(self) -> bool {
        matches!(self.kind, 12..=14 | 29 | 30)
    }
    fn control(self) -> u8 {
        if self.kind == 29 {
            0x96
        } else if self.kind == 30 {
            19
        } else if self.immediate() {
            0x1b
        } else {
            0
        }
    }
}

fn encoding(form: Form, wide: bool, w: bool, roles: [u8; 4], control: u8, memory: bool) -> Vec<u8> {
    let [destination, first, second, mask] = roles;
    let vvvv = if form.unary() { 15 } else { 15 - first };
    let mut bytes = vec![
        0xc4,
        0xe0 | form.map,
        vvvv << 3 | if wide { 4 } else { 0 } | form.pp | if w { 0x80 } else { 0 },
        form.opcode,
        (if memory { 0 } else { 0xc0 }) | destination << 3 | if memory { 0 } else { second },
    ];
    if form.kind == 31 {
        bytes.push(mask << 4 | control);
    } else if form.immediate() {
        bytes.push(control);
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
    let version = if fields[0] == 9 { 15 } else { 7 };
    output[..12].copy_from_slice(&[b'R', b'3', b'M', b'H', version, 0, 1, 0, 40, 0, 0, 0]);
    for (index, field) in fields.into_iter().enumerate() {
        output[16 + index * 4..20 + index * 4].copy_from_slice(&field.to_le_bytes());
    }
}

fn success(before: &[u8], destination: usize, value: [u8; 32], length: u32) -> Vec<u8> {
    let mut expected = before.to_vec();
    let mut simd = decode_simd(&before[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
    let mut upper = decode_ymm(&before[YMM_OFFSET..YMM_OFFSET + YMM_SIZE]).unwrap();
    simd.xmm[destination].copy_from_slice(&value[..16]);
    upper.registers[destination].copy_from_slice(&value[16..]);
    encode_simd(&simd, &mut expected[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
    encode_ymm(&upper, &mut expected[YMM_OFFSET..YMM_OFFSET + YMM_SIZE]).unwrap();
    expected[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 544].fill(0);
    expected[TRANSFER_OFFSET + 516..TRANSFER_OFFSET + 520].copy_from_slice(&FLAGS.to_le_bytes());
    packet(&mut expected, [0, 0, 0, 0, 0, length]);
    expected
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

fn address(engine: &mut EngineInstance, segment: u32, offset: u32) {
    let arena = engine.arena_mut().unwrap();
    let metadata = &mut arena[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568];
    metadata.fill(0);
    metadata[..12].copy_from_slice(&[b'R', b'3', b'A', b'M', 1, 0, 1, 0, 24, 0, 0, 0]);
    metadata[16..20].copy_from_slice(&segment.to_le_bytes());
    metadata[20..24].copy_from_slice(&offset.to_le_bytes());
}

fn roles(register: u8) -> [[u8; 4]; 8] {
    let a = (register + 1) % 8;
    let b = (register + 2) % 8;
    let m = (register + 3) % 8;
    [
        [register, a, b, m],
        [a, a, b, m],
        [b, a, b, m],
        [m, a, b, m],
        [register, a, a, m],
        [register, a, b, a],
        [register, a, b, b],
        [a, a, a, a],
    ]
}

#[test]
fn ymm_encodings_preserve_exact_roles_w_policy_and_explicit_mask_aliases() {
    let mut unique = Vec::new();
    for form in FORMS {
        let operation = decoded(&encoding(
            form,
            true,
            false,
            [0, 1, 2, 3],
            form.control(),
            false,
        ));
        assert!(!unique.contains(&operation));
        unique.push(operation);
        for register in 0..8 {
            for operands in roles(register) {
                let bytes = encoding(form, true, false, operands, form.control(), false);
                let operation = decoded(&bytes);
                if form.kind != 31 {
                    assert_eq!(
                        operation,
                        decoded(&encoding(form, true, true, operands, form.control(), false))
                    );
                } else {
                    for ignored in [0, 1, 15, 128, 143] {
                        assert_eq!(
                            operation,
                            decoded(&encoding(form, true, false, operands, ignored, false))
                        );
                    }
                }
                assert_ne!(
                    operation,
                    decoded(&encoding(
                        form,
                        false,
                        false,
                        operands,
                        form.control(),
                        false
                    ))
                );
            }
        }
    }
}

#[test]
fn ymm_memory_forms_bind_exact_addresses_without_reading_unmapped_sources() {
    for form in FORMS {
        let bytes = encoding(form, true, false, [3, 2, 0, 1], form.control(), true);
        let direct = decoded(&bytes);
        for prefix in [0x36, 0x64, 0x67] {
            let mut overridden = vec![prefix];
            overridden.extend(&bytes);
            assert_ne!(direct, decoded(&overridden));
        }
        let mut code = bytes[..5].to_vec();
        code[4] = 0x1d;
        code.extend([0xef, 0xbe, 0xad, 0xde]);
        code.extend_from_slice(&bytes[5..]);
        code.extend([0xeb, 0xfe]);
        let mut engine = EngineInstance::new(1, KEY).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        upload(&mut engine, 0x1000, &code);
        describe(&mut engine, code.len());
        let before = engine.arena().to_vec();
        assert_eq!(engine.compile(1), Ok(1));
        assert_eq!(engine.arena(), before);
        assert_eq!(&engine.artifact_bytes().unwrap()[..8], b"\0asm\x01\0\0\0");
        assert_eq!(engine.guard(KEY, 1), Ok(()));
        upload(&mut engine, 0x1000, &[0x90]);
        assert!(engine.guard(KEY, 1).is_err());
    }
}

fn pattern(register: usize) -> [u8; 32] {
    let raw = [
        0x00, 0x80, 0xff, 0x7f, 0x01, 0x00, 0xff, 0xff, 0x80, 0xff, 0x7f, 0x00, 0, 1, 0xff, 0,
        0x80, 0x7f, 0xff, 1, 0, 0xfe, 0x81, 0x7e, 2, 0xfd, 0x82, 0x7d, 0x12, 0xed, 0x23, 0xdc,
    ];
    std::array::from_fn(|byte| raw[byte] ^ (register * 37) as u8)
}

fn seed(engine: &mut EngineInstance) -> [[u8; 32]; 8] {
    let vectors = std::array::from_fn(pattern);
    let simd = SimdState {
        mxcsr: 0xffff,
        xmm: std::array::from_fn(|r| vectors[r][..16].try_into().unwrap()),
    };
    let upper = YmmUpperState {
        registers: std::array::from_fn(|r| vectors[r][16..].try_into().unwrap()),
    };
    let arena = engine.arena_mut().unwrap();
    encode_simd(&simd, &mut arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
    encode_ymm(&upper, &mut arena[YMM_OFFSET..YMM_OFFSET + YMM_SIZE]).unwrap();
    arena[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 544].fill(0xa5);
    arena[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568].fill(0xff);
    vectors
}

fn oracle(
    form: Form,
    first: [u8; 32],
    second: [u8; 32],
    mask: [u8; 32],
    control: u8,
    output_bytes: usize,
) -> [u8; 32] {
    let mut result = [0; 32];
    for half in (0..output_bytes).step_by(16) {
        let a = &first[half..half + 16];
        let b = &second[half..half + 16];
        let m = &mask[half..half + 16];
        let out = &mut result[half..half + 16];
        match form.kind {
            1..=3 | 6..=8 | 10 | 11 => {
                let width = match form.kind {
                    1 | 6 => 1,
                    2 | 7 => 2,
                    3 | 8 => 4,
                    _ => 8,
                };
                let start = if matches!(form.kind, 6..=8 | 11) {
                    8
                } else {
                    0
                };
                for lane in 0..8 / width {
                    out[lane * width * 2..lane * width * 2 + width]
                        .copy_from_slice(&a[start + lane * width..start + (lane + 1) * width]);
                    out[lane * width * 2 + width..(lane + 1) * width * 2]
                        .copy_from_slice(&b[start + lane * width..start + (lane + 1) * width]);
                }
            }
            4 | 5 | 9 | 22 => {
                let width = if matches!(form.kind, 4 | 5) { 1 } else { 2 };
                let source_width = width * 2;
                for (source, bytes) in [a, b].into_iter().enumerate() {
                    for lane in 0..16 / source_width {
                        let at = lane * source_width;
                        let signed = if source_width == 2 {
                            i16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) as i64
                        } else {
                            i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as i64
                        };
                        let (min, max) = if matches!(form.kind, 5 | 22) {
                            (0, (1_i64 << (width * 8)) - 1)
                        } else {
                            (-(1_i64 << (width * 8 - 1)), (1_i64 << (width * 8 - 1)) - 1)
                        };
                        let value = signed.clamp(min, max).to_le_bytes();
                        let output = source * 8 + lane * width;
                        out[output..output + width].copy_from_slice(&value[..width]);
                    }
                }
            }
            12 => {
                for lane in 0..4 {
                    let source = usize::from(control >> (lane * 2) & 3) * 4;
                    out[lane * 4..lane * 4 + 4].copy_from_slice(&b[source..source + 4]);
                }
            }
            13 | 14 => {
                out.copy_from_slice(b);
                let start = if form.kind == 13 { 8 } else { 0 };
                for lane in 0..4 {
                    let source = start + usize::from(control >> (lane * 2) & 3) * 2;
                    out[start + lane * 2..start + lane * 2 + 2]
                        .copy_from_slice(&b[source..source + 2]);
                }
            }
            15 => {
                for lane in 0..16 {
                    out[lane] = if b[lane] & 0x80 != 0 {
                        0
                    } else {
                        a[usize::from(b[lane] & 15)]
                    };
                }
            }
            29 => {
                for lane in 0..8 {
                    let source = if control & (1 << lane) != 0 { b } else { a };
                    out[lane * 2..lane * 2 + 2].copy_from_slice(&source[lane * 2..lane * 2 + 2]);
                }
            }
            30 => {
                for (lane, byte) in out.iter_mut().enumerate() {
                    let source = lane + usize::from(control);
                    *byte = if source < 16 {
                        b[source]
                    } else if source < 32 {
                        a[source - 16]
                    } else {
                        0
                    };
                }
            }
            31 => {
                for lane in 0..16 {
                    out[lane] = if m[lane] & 0x80 != 0 {
                        b[lane]
                    } else {
                        a[lane]
                    };
                }
            }
            _ => unreachable!("test-owned nineteen-form census"),
        }
    }
    result
}

fn invoke(
    engine: &mut EngineInstance,
    resident: Option<u64>,
    form: Form,
    wide: bool,
    registers: [u8; 4],
    control: u8,
    memory: bool,
) -> Result<StoreCompletion, HostError> {
    let [destination, first, second, mask] = registers;
    let operands = u32::from(destination)
        | if form.unary() {
            0
        } else {
            u32::from(first) << 3
        }
        | if memory {
            1 << 12
        } else {
            u32::from(second) << 6
        }
        | if form.kind == 31 {
            u32::from(mask) << 9
        } else {
            0
        };
    let descriptor = 0x01000600 | u32::from(form.kind | if wide { 0x80 } else { 0 });
    let immediate = if form.immediate() {
        u32::from(control)
    } else {
        0
    };
    match resident {
        None => engine.avx_execute(descriptor, operands, immediate, 0, 0, FLAGS),
        Some(id) => {
            engine.avx_execute_resident(KEY, id, descriptor, operands, immediate, 0, 0, FLAGS)
        }
    }
}

fn register_helpers(wide: bool) {
    let (mut engine, id) = helper_engine();
    let before_ram = ram(&engine);
    for form in FORMS {
        for register in 0..8 {
            for registers in roles(register) {
                for resident in [None, Some(id)] {
                    let vectors = seed(&mut engine);
                    let [d, a, b, m] = registers.map(usize::from);
                    let value = oracle(
                        form,
                        vectors[a],
                        vectors[b],
                        vectors[m],
                        form.control(),
                        if wide { 32 } else { 16 },
                    );
                    let expected = success(engine.arena(), d, value, 0);
                    assert_eq!(
                        invoke(
                            &mut engine,
                            resident,
                            form,
                            wide,
                            registers,
                            form.control(),
                            false
                        ),
                        Ok(StoreCompletion::Complete)
                    );
                    assert_eq!(
                        engine.arena(),
                        expected,
                        "kind{} roles{registers:?}",
                        form.kind
                    );
                    assert_eq!(ram(&engine), before_ram);
                }
            }
        }
    }
}

#[test]
fn ymm_helpers_keep_packs_shuffles_align_and_masks_local_to_each_captured_half() {
    register_helpers(true);
}

#[test]
fn ymm_memory_helpers_check_full_thirty_two_bytes_before_any_vector_publication() {
    let (mut engine, id) = helper_engine();
    let data = pattern(2);
    upload(&mut engine, 0x5fe0, &data);
    let before_ram = ram(&engine);
    for form in FORMS {
        let control = if form.kind == 29 {
            0
        } else if form.kind == 30 {
            255
        } else {
            form.control()
        };
        for resident in [None, Some(id)] {
            for (offset, fault) in [(0x5fe0, false), (0x5fe1, true), (0x5ff0, true)] {
                let vectors = seed(&mut engine);
                address(&mut engine, 3, offset);
                let before = engine.arena().to_vec();
                let expected = if fault {
                    let mut expected = before;
                    packet(&mut expected, [9, 0, 4, 0x6000, 1, 32]);
                    expected
                } else {
                    success(
                        &before,
                        0,
                        oracle(form, vectors[0], data, vectors[1], control, 32),
                        32,
                    )
                };
                assert_eq!(
                    invoke(
                        &mut engine,
                        resident,
                        form,
                        true,
                        [0, 0, 0, 1],
                        control,
                        true
                    ),
                    Ok(StoreCompletion::Complete)
                );
                assert_eq!(
                    engine.arena(),
                    expected,
                    "kind{} offset{offset:08x}",
                    form.kind
                );
                assert_eq!(ram(&engine), before_ram);
            }
            for segment in [3, 2] {
                seed(&mut engine);
                address(&mut engine, segment, 0xfffffff0);
                let mut expected = engine.arena().to_vec();
                packet(
                    &mut expected,
                    [if segment == 2 { 7 } else { 3 }, 0, 0, 0, 0, 32],
                );
                assert_eq!(
                    invoke(
                        &mut engine,
                        resident,
                        form,
                        true,
                        [0, 0, 0, 1],
                        control,
                        true
                    ),
                    Ok(StoreCompletion::Complete)
                );
                assert_eq!(engine.arena(), expected);
                assert_eq!(ram(&engine), before_ram);
            }
        }
    }
}

#[test]
fn existing_xmm_lane_operations_keep_all_roles_and_destination_upper_zero() {
    for form in FORMS {
        decoded(&encoding(
            form,
            false,
            false,
            [0, 1, 2, 3],
            form.control(),
            false,
        ));
    }
    register_helpers(false);
}

#[test]
fn reserved_vvvv_legacy_prefixes_and_blend_w1_stay_invalid_before_admission() {
    for form in FORMS {
        for wide in [false, true] {
            let valid = encoding(form, wide, false, [0, 1, 2, 3], form.control(), false);
            for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
                let mut bytes = vec![prefix];
                bytes.extend(&valid);
                assert_eq!(
                    decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                    DecodeError::InvalidEncoding
                );
            }
            if form.unary() {
                for reserved in 0..15 {
                    let mut bytes = valid.clone();
                    bytes[2] = bytes[2] & !0x78 | reserved << 3;
                    assert_eq!(
                        decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                        DecodeError::InvalidEncoding
                    );
                }
            }
            if form.kind == 31 {
                let mut bytes = valid.clone();
                bytes[2] |= 0x80;
                assert_eq!(
                    decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                    DecodeError::InvalidEncoding
                );
            }
        }
    }
}

#[test]
fn malformed_lane_descriptors_refuse_without_state_metadata_or_ram_consumption() {
    let (mut engine, _) = helper_engine();
    let before_ram = ram(&engine);
    for form in FORMS {
        let mut invalid = vec![(1 << 13, 0), (1 << 12 | 1 << 6, 0), (1 << 14, 0), (0, 256)];
        if form.unary() {
            invalid.push((1 << 3, 0));
        }
        if form.kind != 31 {
            invalid.push((1 << 9, 0));
        }
        if !form.immediate() {
            invalid.push((0, 1));
        }
        for (operands, immediate) in invalid {
            seed(&mut engine);
            let before = engine.arena().to_vec();
            assert_eq!(
                engine.avx_execute(
                    0x01000680 | u32::from(form.kind),
                    operands,
                    immediate,
                    0,
                    0,
                    FLAGS
                ),
                Err(HostError::InvalidRequest)
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(ram(&engine), before_ram);
        }
    }
}
