use ring3_engine::{
    abi::{
        arena::{HELPER_OFFSET, SEGMENTS_OFFSET, SIMD_OFFSET, TRANSFER_OFFSET, YMM_OFFSET},
        x86::{
            SEGMENTS_SIZE, SIMD_SIZE, YMM_SIZE, encode_segment_context, encode_simd, encode_ymm,
        },
    },
    cpu::x86::{
        SegmentCache, SegmentContext, SegmentRegister,
        decode::{DecodeError, decode_one},
        ir::Operation,
        simd::{SimdState, ymm::YmmUpperState},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, StoreCompletion},
};

#[derive(Clone, Copy)]
struct Form {
    width: usize,
    lane: usize,
    store: bool,
    kind: u32,
}

const FORMS: [Form; 8] = [
    Form {
        width: 16,
        lane: 4,
        store: false,
        kind: 2,
    },
    Form {
        width: 32,
        lane: 4,
        store: false,
        kind: 3,
    },
    Form {
        width: 16,
        lane: 8,
        store: false,
        kind: 4,
    },
    Form {
        width: 32,
        lane: 8,
        store: false,
        kind: 5,
    },
    Form {
        width: 16,
        lane: 4,
        store: true,
        kind: 6,
    },
    Form {
        width: 32,
        lane: 4,
        store: true,
        kind: 7,
    },
    Form {
        width: 16,
        lane: 8,
        store: true,
        kind: 8,
    },
    Form {
        width: 32,
        lane: 8,
        store: true,
        kind: 9,
    },
];

fn encoding(form: Form, mask: u8, data: u8, memory: &[u8], floating: bool) -> Vec<u8> {
    let w = if !floating && form.lane == 8 { 0x80 } else { 0 };
    let l = if form.width == 32 { 4 } else { 0 };
    let opcode = if floating {
        0x2c + u8::from(form.lane == 8) + 2 * u8::from(form.store)
    } else if form.store {
        0x8e
    } else {
        0x8c
    };
    let mut bytes = vec![0xc4, 0xe2, w | (15 - mask) << 3 | l | 1, opcode];
    bytes.push(memory[0] | data << 3);
    bytes.extend_from_slice(&memory[1..]);
    bytes
}

fn code(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    let page = PageRange::new(GuestAddress(0x1000), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}

fn operation(bytes: &[u8]) -> Operation {
    let memory = code(bytes);
    let value = decode_one(&memory, GuestAddress(0x1000)).unwrap();
    assert!(matches!(value.operation(), Operation::Avx(_)));
    assert_eq!(usize::from(value.length()), bytes.len());
    assert_eq!(value.next_pc(), GuestAddress(0x1000 + bytes.len() as u32));
    assert!(memory.is_code_current(value.code_snapshot()));
    *value.operation()
}

#[test]
fn integer_forms_preserve_mask_data_width_and_shared_float_semantics() {
    let mut distinct = Vec::new();
    for form in FORMS {
        let canonical = operation(&encoding(form, 2, 1, &[0], false));
        assert!(!distinct.contains(&canonical));
        distinct.push(canonical);
        for mask in 0..8 {
            for data in 0..8 {
                let integer = encoding(form, mask, data, &[0], false);
                let floating = encoding(form, mask, data, &[0], true);
                assert_eq!(operation(&integer), operation(&floating), "{integer:02x?}");
                let mut other_w = integer.clone();
                other_w[2] ^= 0x80;
                assert_ne!(operation(&integer), operation(&other_w));
                assert_ne!(
                    operation(&integer),
                    operation(&encoding(form, (mask + 1) % 8, data, &[0], false))
                );
                assert_ne!(
                    operation(&integer),
                    operation(&encoding(form, mask, (data + 1) % 8, &[0], false))
                );
            }
        }
    }
}

#[test]
fn integer_addresses_are_bound_without_reading_selected_guest_memory() {
    for form in FORMS {
        for (prefix, address) in [
            (None, &[0][..]),
            (None, &[0x44, 0x24, 0x7f][..]),
            (None, &[0x05, 0xef, 0xbe, 0xad, 0xde][..]),
            (Some(0x64), &[0][..]),
            (Some(0x36), &[0][..]),
            (Some(0x67), &[0x02][..]),
        ] {
            let mut integer = encoding(form, 7, 7, address, false);
            let mut floating = encoding(form, 7, 7, address, true);
            if let Some(prefix) = prefix {
                integer.insert(0, prefix);
                floating.insert(0, prefix);
            }
            assert_eq!(operation(&integer), operation(&floating));
        }
    }
}

#[test]
fn invalid_prefixes_and_register_memory_substitutions_remain_encoding_errors() {
    for form in FORMS {
        let bytes = encoding(form, 2, 1, &[0], false);
        for prefix in [0xf0, 0x66, 0xf2, 0xf3] {
            let bytes = [&[prefix][..], &bytes].concat();
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
        let bytes = encoding(form, 2, 1, &[0xc0], false);
        assert_eq!(
            decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
    }
}

#[test]
fn existing_float_forms_remain_admitted_with_strict_w0() {
    for form in FORMS {
        operation(&encoding(form, 7, 7, &[0], true));
        let mut w1 = encoding(form, 2, 1, &[0], true);
        w1[2] |= 0x80;
        assert_eq!(
            decode_one(&code(&w1), GuestAddress(0x1000)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
    }
}

fn upload(engine: &mut EngineInstance, address: u32, value: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + value.len()]
        .copy_from_slice(value);
    engine.upload(address, value.len() as u32).unwrap();
}

fn ram(engine: &mut EngineInstance) -> Vec<u8> {
    let saved = engine.arena().to_vec();
    let mut result = Vec::with_capacity(8192);
    for page in [0x1000, 0x5000] {
        for offset in (0..4096).step_by(512) {
            engine.cpu_read(page + offset, 512, 1).unwrap();
            assert_eq!(
                &engine.arena()[HELPER_OFFSET + 16..HELPER_OFFSET + 20],
                &[0; 4]
            );
            result.extend_from_slice(&engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + 512]);
        }
    }
    engine.arena_mut().unwrap().copy_from_slice(&saved);
    result
}

#[test]
fn shared_helpers_preserve_raw_selected_values_and_atomic_faults_while_avx2_is_off() {
    let mut engine = EngineInstance::new(2, 91).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x5000, 1, 3).unwrap();
    upload(&mut engine, 0x1000, &[0xeb, 0xfe]);
    let raw = [
        1, 0, 0, 0, 0, 0, 0xf0, 0x7f, 1, 0, 0x80, 0x7f, 0, 0, 0, 0x80, 1, 0, 0, 0, 0, 0, 0, 0,
        0x45, 0x23, 0xc1, 0x7f, 0, 0, 0x80, 0x3f,
    ];
    let mut data: [u8; 512] = std::array::from_fn(|byte| (byte * 37 + 19) as u8);
    data[448..480].copy_from_slice(&raw);
    upload(&mut engine, 0x5e00, &data);
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    let initial = engine.arena().to_vec();
    let original_ram = ram(&mut engine);
    for form in FORMS {
        let lanes = form.width / form.lane;
        for (scenario, bits) in [
            (0, 0),
            (1, 1),
            (2, (1 << lanes) - 1),
            (3, 3),
            (4, 3),
            (5, 3),
        ] {
            for data_register in [1, 3] {
                let address = if scenario == 0 {
                    u32::MAX
                } else if scenario == 2 {
                    0x5fc0
                } else {
                    0x6000 - form.lane as u32
                };
                let mut vectors: [[u8; 32]; 8] =
                    std::array::from_fn(|r| std::array::from_fn(|b| (r * 43 + b * 71 + 1) as u8));
                vectors[1] = raw;
                vectors[3] = [0x7f; 32];
                for lane in 0..lanes {
                    vectors[3][(lane + 1) * form.lane - 1] =
                        if bits & (1 << lane) != 0 { 0x80 } else { 0x7f };
                }
                let simd = SimdState {
                    mxcsr: 0xffff,
                    xmm: vectors.map(|v| v[..16].try_into().unwrap()),
                };
                let upper = YmmUpperState {
                    registers: vectors.map(|v| v[16..].try_into().unwrap()),
                };
                let mut context = SegmentContext::default();
                let segment = if scenario == 5 {
                    SegmentRegister::Ss
                } else {
                    SegmentRegister::Ds
                };
                if scenario == 0 {
                    context.state.selectors[SegmentRegister::Ds.index()] = 0;
                    context.caches[SegmentRegister::Ds.index()] = SegmentCache::UNUSABLE;
                } else if scenario >= 4 {
                    context.caches[segment.index()] = SegmentCache::from_bits(0xf230_5fff).unwrap();
                }
                let mut before = initial.clone();
                encode_simd(&simd, &mut before[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
                encode_ymm(&upper, &mut before[YMM_OFFSET..YMM_OFFSET + YMM_SIZE]).unwrap();
                encode_segment_context(
                    &context,
                    &mut before[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE],
                )
                .unwrap();
                before[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 544].fill(0xa5);
                let metadata = &mut before[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568];
                metadata.fill(0);
                metadata[..12].copy_from_slice(&[b'R', b'3', b'A', b'M', 1, 0, 1, 0, 24, 0, 0, 0]);
                metadata[16..20].copy_from_slice(&(segment.index() as u32).to_le_bytes());
                metadata[20..24].copy_from_slice(&address.to_le_bytes());
                engine.arena_mut().unwrap().copy_from_slice(&before);
                let before_ram = ram(&mut engine);
                let mut expected_ram = before_ram.clone();
                let mut expected = before;
                let fault = scenario >= 3;
                let status: u32 = if scenario == 3 {
                    9
                } else if scenario == 4 {
                    3
                } else if scenario == 5 {
                    7
                } else {
                    0
                };
                expected[HELPER_OFFSET..HELPER_OFFSET + 40].fill(0);
                let version = if scenario == 3 { 15 } else { 7 };
                expected[HELPER_OFFSET..HELPER_OFFSET + 12]
                    .copy_from_slice(&[b'R', b'3', b'M', b'H', version, 0, 1, 0, 40, 0, 0, 0]);
                expected[HELPER_OFFSET + 16..HELPER_OFFSET + 20]
                    .copy_from_slice(&status.to_le_bytes());
                expected[HELPER_OFFSET + 36..HELPER_OFFSET + 40]
                    .copy_from_slice(&(form.width as u32).to_le_bytes());
                if scenario == 3 {
                    expected[HELPER_OFFSET + 24..HELPER_OFFSET + 28]
                        .copy_from_slice(&(if form.store { 6_u32 } else { 4 }).to_le_bytes());
                    expected[HELPER_OFFSET + 28..HELPER_OFFSET + 32]
                        .copy_from_slice(&0x6000_u32.to_le_bytes());
                    expected[HELPER_OFFSET + 32..HELPER_OFFSET + 36]
                        .copy_from_slice(&(if form.store { 2_u32 } else { 1 }).to_le_bytes());
                }
                if !fault {
                    expected[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 544].fill(0);
                    expected[TRANSFER_OFFSET + 516..TRANSFER_OFFSET + 520]
                        .copy_from_slice(&0x244cd7_u32.to_le_bytes());
                    let mut value = [0; 32];
                    for lane in 0..lanes {
                        if bits & (1 << lane) == 0 {
                            continue;
                        }
                        let at = lane * form.lane;
                        let memory = 4096 + address as usize - 0x5000 + at;
                        if form.store {
                            expected_ram[memory..memory + form.lane]
                                .copy_from_slice(&vectors[data_register][at..at + form.lane]);
                        } else {
                            value[at..at + form.lane]
                                .copy_from_slice(&before_ram[memory..memory + form.lane]);
                        }
                    }
                    if !form.store {
                        let mut output = simd;
                        let mut output_upper = upper;
                        output.xmm[data_register].copy_from_slice(&value[..16]);
                        output_upper.registers[data_register].copy_from_slice(&value[16..]);
                        encode_simd(&output, &mut expected[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE])
                            .unwrap();
                        encode_ymm(
                            &output_upper,
                            &mut expected[YMM_OFFSET..YMM_OFFSET + YMM_SIZE],
                        )
                        .unwrap();
                    }
                }
                let operands = 3 << 9
                    | if form.store {
                        (data_register as u32) << 6 | 1 << 13
                    } else {
                        data_register as u32 | 1 << 12
                    };
                assert_eq!(
                    engine.avx_execute(0x01000d00 | form.kind, operands, 0, 0, 0, 0x244cd7),
                    Ok(StoreCompletion::Complete)
                );
                assert_eq!(
                    engine.arena(),
                    expected,
                    "kind{} scenario{scenario} data{data_register}",
                    form.kind
                );
                assert_eq!(ram(&mut engine), expected_ram);
                assert_eq!(engine.arena(), expected);
            }
        }
    }
    assert_eq!(&ram(&mut engine)[..4096], &original_ram[..4096]);
}
