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
    kind: u8,
    from: usize,
    to: usize,
    signed: bool,
}

const FORMS: [Form; 12] = [
    Form {
        opcode: 0x20,
        kind: 16,
        from: 1,
        to: 2,
        signed: true,
    },
    Form {
        opcode: 0x21,
        kind: 17,
        from: 1,
        to: 4,
        signed: true,
    },
    Form {
        opcode: 0x22,
        kind: 18,
        from: 1,
        to: 8,
        signed: true,
    },
    Form {
        opcode: 0x23,
        kind: 19,
        from: 2,
        to: 4,
        signed: true,
    },
    Form {
        opcode: 0x24,
        kind: 20,
        from: 2,
        to: 8,
        signed: true,
    },
    Form {
        opcode: 0x25,
        kind: 21,
        from: 4,
        to: 8,
        signed: true,
    },
    Form {
        opcode: 0x30,
        kind: 23,
        from: 1,
        to: 2,
        signed: false,
    },
    Form {
        opcode: 0x31,
        kind: 24,
        from: 1,
        to: 4,
        signed: false,
    },
    Form {
        opcode: 0x32,
        kind: 25,
        from: 1,
        to: 8,
        signed: false,
    },
    Form {
        opcode: 0x33,
        kind: 26,
        from: 2,
        to: 4,
        signed: false,
    },
    Form {
        opcode: 0x34,
        kind: 27,
        from: 2,
        to: 8,
        signed: false,
    },
    Form {
        opcode: 0x35,
        kind: 28,
        from: 4,
        to: 8,
        signed: false,
    },
];

fn encoding(form: Form, wide: bool, w: bool, operands: &[u8]) -> Vec<u8> {
    let mut bytes = vec![
        0xc4,
        0xe2,
        0x79 | if wide { 4 } else { 0 } | if w { 0x80 } else { 0 },
    ];
    bytes.push(form.opcode);
    bytes.extend_from_slice(operands);
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

#[test]
fn ymm_forms_capture_unary_xmm_sources_all_aliases_and_ignored_w() {
    let mut operations = Vec::new();
    for form in FORMS {
        let canonical = decoded(&encoding(form, true, false, &[0xcb]));
        assert!(!operations.contains(&canonical));
        operations.push(canonical);
        for destination in 0..8 {
            for source in 0..8 {
                let modrm = 0xc0 | destination << 3 | source;
                let operation = decoded(&encoding(form, true, false, &[modrm]));
                assert_eq!(operation, decoded(&encoding(form, true, true, &[modrm])));
                assert_ne!(operation, decoded(&encoding(form, false, false, &[modrm])));
                assert_ne!(
                    operation,
                    decoded(&encoding(
                        form,
                        true,
                        false,
                        &[0xc0 | ((destination + 1) % 8) << 3 | source]
                    ))
                );
                assert_ne!(
                    operation,
                    decoded(&encoding(
                        form,
                        true,
                        false,
                        &[0xc0 | destination << 3 | ((source + 1) % 8)]
                    ))
                );
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
fn ymm_memory_forms_capture_segment_address_size_and_exact_unread_operands() {
    for form in FORMS {
        let mut operations = Vec::new();
        for (prefix, address) in [
            (None, &[0x08][..]),
            (None, &[0x4c, 0x24, 0x7f][..]),
            (None, &[0x0d, 0xef, 0xbe, 0xad, 0xde][..]),
            (Some(0x36), &[0x08][..]),
            (Some(0x64), &[0x08][..]),
            (Some(0x67), &[0x0a][..]),
        ] {
            let mut bytes = prefix.into_iter().collect::<Vec<_>>();
            bytes.extend(encoding(form, true, false, address));
            let operation = decoded(&bytes);
            let mut w_alias = bytes.clone();
            w_alias[usize::from(prefix.is_some()) + 2] |= 0x80;
            assert_eq!(operation, decoded(&w_alias));
            assert!(!operations.contains(&operation));
            operations.push(operation);
        }
        let mut bytes = encoding(form, true, false, &[0x0d, 0xef, 0xbe, 0xad, 0xde]);
        bytes.extend([0xeb, 0xfe]);
        let mut engine = EngineInstance::new(1, KEY).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        upload(&mut engine, 0x1000, &bytes);
        describe(&mut engine, bytes.len());
        let before = engine.arena().to_vec();
        assert_eq!(engine.compile(1), Ok(1));
        assert_eq!(engine.arena(), before);
        assert_eq!(&engine.artifact_bytes().unwrap()[..8], b"\0asm\x01\0\0\0");
        assert_eq!(engine.guard(KEY, 1), Ok(()));
        upload(&mut engine, 0x1000, &[0x90]);
        assert!(engine.guard(KEY, 1).is_err());
    }
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

fn invoke(
    engine: &mut EngineInstance,
    resident: Option<u64>,
    form: Form,
    wide: bool,
    operands: u32,
    immediate: u32,
) -> Result<StoreCompletion, HostError> {
    let descriptor = 0x01000600 | u32::from(form.kind | if wide { 0x80 } else { 0 });
    match resident {
        Some(id) => {
            engine.avx_execute_resident(KEY, id, descriptor, operands, immediate, 0, 0, FLAGS)
        }
        None => engine.avx_execute(descriptor, operands, immediate, 0, 0, FLAGS),
    }
}

fn pattern(from: usize, register: usize) -> [u8; 16] {
    let modulus = 1_u64 << (from * 8);
    let sign = modulus / 2;
    let values = [
        sign,
        sign - 1,
        modulus - 1,
        1,
        0,
        modulus - 2,
        sign + 1,
        sign - 2,
        2,
        modulus - 3,
        sign + 2,
        sign - 3,
        0x12,
        modulus - 0x13,
        0x23,
        modulus - 0x24,
    ];
    std::array::from_fn(|byte| {
        values[(byte / from + register * 3) % values.len()].to_le_bytes()[byte % from]
    })
}

fn oracle(form: Form, output_bytes: usize, source: &[u8]) -> [u8; 32] {
    let mut result = [0; 32];
    for lane in 0..output_bytes / form.to {
        let offset = lane * form.from;
        let negative = form.signed && source[offset + form.from - 1] & 0x80 != 0;
        let destination = &mut result[lane * form.to..(lane + 1) * form.to];
        destination.fill(if negative { 0xff } else { 0 });
        destination[..form.from].copy_from_slice(&source[offset..offset + form.from]);
    }
    result
}

fn seed(engine: &mut EngineInstance, from: usize, variant: u8) {
    let simd = SimdState {
        mxcsr: 0xffff,
        xmm: std::array::from_fn(|r| pattern(from, r)),
    };
    let upper = YmmUpperState {
        registers: std::array::from_fn(|r| {
            std::array::from_fn(|b| (r * 23 + b * 17) as u8 ^ variant)
        }),
    };
    let arena = engine.arena_mut().unwrap();
    encode_simd(&simd, &mut arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
    encode_ymm(&upper, &mut arena[YMM_OFFSET..YMM_OFFSET + YMM_SIZE]).unwrap();
    arena[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 544].fill(0xa5);
    // register-only operations must ignore a stale address record.
    arena[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568].fill(0xff);
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

fn literal_windows() {
    for signed in [true, false] {
        let form = FORMS[if signed { 2 } else { 8 }];
        let result = oracle(form, 32, &[0x80, 0x7f, 0xff, 1]);
        let literal = if signed {
            [-128_i64, 127, -1, 1]
        } else {
            [128, 127, 255, 1]
        };
        for (lane, value) in literal.into_iter().enumerate() {
            assert_eq!(&result[lane * 8..lane * 8 + 8], &value.to_le_bytes());
        }
    }
    let result = oracle(FORMS[1], 32, &[0x80, 0x7f, 0xff, 0, 1, 0xfe, 0x81, 0x7e]);
    for (lane, value) in [-128_i32, 127, -1, 0, 1, -2, -127, 126]
        .into_iter()
        .enumerate()
    {
        assert_eq!(&result[lane * 4..lane * 4 + 4], &value.to_le_bytes());
    }
}

#[test]
fn ymm_helpers_extend_contiguous_low_xmm_windows_before_alias_publication() {
    literal_windows();
    let (mut engine, id) = helper_engine();
    let before_ram = ram(&engine);
    for form in FORMS {
        for destination in 0..8 {
            for source in 0..8 {
                for (variant, resident) in [(0x35, None), (0xca, Some(id))] {
                    seed(&mut engine, form.from, variant);
                    let before = engine.arena().to_vec();
                    let input = pattern(form.from, source);
                    let expected = success(&before, destination, oracle(form, 32, &input), 0);
                    assert_eq!(
                        invoke(
                            &mut engine,
                            resident,
                            form,
                            true,
                            destination as u32 | (source as u32) << 6,
                            0
                        ),
                        Ok(StoreCompletion::Complete)
                    );
                    assert_eq!(
                        engine.arena(),
                        expected,
                        "opcode{:02x} D{destination} B{source} upper{variant:02x}",
                        form.opcode
                    );
                    assert_eq!(ram(&engine), before_ram);
                }
            }
        }
    }
}

fn address(engine: &mut EngineInstance, segment: u32, offset: u32) {
    let arena = engine.arena_mut().unwrap();
    let metadata = &mut arena[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568];
    metadata.fill(0);
    metadata[..12].copy_from_slice(&[b'R', b'3', b'A', b'M', 1, 0, 1, 0, 24, 0, 0, 0]);
    metadata[16..20].copy_from_slice(&segment.to_le_bytes());
    metadata[20..24].copy_from_slice(&offset.to_le_bytes());
}

fn memory_cases(wide: bool) {
    let (mut engine, id) = helper_engine();
    for form in FORMS {
        let output = if wide { 32 } else { 16 };
        let length = output / form.to * form.from;
        let input = pattern(form.from, 0);
        let endpoint = 0x6000 - length as u32;
        upload(&mut engine, endpoint, &input[..length]);
        let before_ram = ram(&engine);
        for resident in [None, Some(id)] {
            for (offset, fields) in [
                (endpoint, [0, 0, 0, 0, 0, length as u32]),
                (endpoint + 1, [9, 0, 4, 0x6000, 1, length as u32]),
            ] {
                seed(&mut engine, form.from, 0x53);
                address(&mut engine, 3, offset);
                let before = engine.arena().to_vec();
                let expected = if offset == endpoint {
                    success(
                        &before,
                        0,
                        oracle(form, output, &input[..length]),
                        length as u32,
                    )
                } else {
                    let mut expected = before;
                    packet(&mut expected, fields);
                    expected
                };
                assert_eq!(
                    invoke(&mut engine, resident, form, wide, 1 << 12, 0),
                    Ok(StoreCompletion::Complete)
                );
                assert_eq!(
                    engine.arena(),
                    expected,
                    "opcode{:02x} length{length} address{offset:08x}",
                    form.opcode
                );
                assert_eq!(ram(&engine), before_ram);
            }
            for segment in [3, 2] {
                seed(&mut engine, form.from, 0x53);
                address(&mut engine, segment, u32::MAX - length as u32 + 2);
                let before = engine.arena().to_vec();
                let mut expected = before;
                packet(
                    &mut expected,
                    [if segment == 2 { 7 } else { 3 }, 0, 0, 0, 0, length as u32],
                );
                assert_eq!(
                    invoke(&mut engine, resident, form, wide, 1 << 12, 0),
                    Ok(StoreCompletion::Complete)
                );
                assert_eq!(
                    engine.arena(),
                    expected,
                    "full logical source span before paging"
                );
                assert_eq!(ram(&engine), before_ram);
            }
        }
    }
}

#[test]
fn ymm_memory_helpers_read_exact_four_eight_or_sixteen_bytes_atomically() {
    memory_cases(true);
}

#[test]
fn existing_xmm_widening_keeps_half_source_width_and_zeroes_destination_upper() {
    let (mut engine, id) = helper_engine();
    for form in FORMS {
        for destination in 0..8 {
            for source in [destination, (destination + 1) % 8] {
                let modrm = 0xc0 | (destination as u8) << 3 | source as u8;
                let operation = decoded(&encoding(form, false, false, &[modrm]));
                assert_eq!(operation, decoded(&encoding(form, false, true, &[modrm])));
                for resident in [None, Some(id)] {
                    seed(&mut engine, form.from, 0x97);
                    let before = engine.arena().to_vec();
                    let value = oracle(form, 16, &pattern(form.from, source));
                    let expected = success(&before, destination, value, 0);
                    assert_eq!(
                        invoke(
                            &mut engine,
                            resident,
                            form,
                            false,
                            destination as u32 | (source as u32) << 6,
                            0
                        ),
                        Ok(StoreCompletion::Complete)
                    );
                    assert_eq!(engine.arena(), expected);
                }
            }
        }
    }
    memory_cases(false);
}

#[test]
fn reserved_vvvv_and_forbidden_legacy_prefixes_stay_encoding_errors() {
    for form in FORMS {
        for wide in [false, true] {
            for w in [false, true] {
                let valid = encoding(form, wide, w, &[0xcb]);
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
                    bytes[2] = (bytes[2] & !0x78) | reserved << 3;
                    crate::support_fetch::assert_decode_contract(
                        decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                        DecodeError::InvalidEncoding,
                    );
                }
            }
        }
    }
}

#[test]
fn malformed_wide_descriptors_refuse_without_consuming_metadata_or_state() {
    let (mut engine, id) = helper_engine();
    let before_ram = ram(&engine);
    for form in FORMS {
        for (operands, immediate) in [
            (1 << 3, 0),
            (1 << 9, 0),
            (1 << 13, 0),
            (1 << 12 | 1 << 6, 0),
            (1 << 14, 0),
            (0, 1),
            (0, 255),
            (0, 256),
        ] {
            for resident in [None, Some(id)] {
                seed(&mut engine, form.from, 0x61);
                let before = engine.arena().to_vec();
                crate::support_fetch::assert_host_contract(
                    (invoke(&mut engine, resident, form, true, operands, immediate))
                        .expect_err("compilation must refuse this fixture"),
                    HostError::InvalidRequest,
                );
                assert_eq!(engine.arena(), before);
                assert_eq!(ram(&engine), before_ram);
            }
        }
    }
    for kind in [0x80, 0xc0, 0xd0, 0xff] {
        seed(&mut engine, 1, 0x61);
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.avx_execute(0x01000600 | kind, 0, 0, 0, 0, FLAGS),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), before);
    }
}
