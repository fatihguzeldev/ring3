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
struct Form {
    map: u8,
    opcode: u8,
    immediate: bool,
}
const FORMS: [Form; 4] = [
    Form {
        map: 2,
        opcode: 0x16,
        immediate: false,
    },
    Form {
        map: 2,
        opcode: 0x36,
        immediate: false,
    },
    Form {
        map: 3,
        opcode: 0,
        immediate: true,
    },
    Form {
        map: 3,
        opcode: 1,
        immediate: true,
    },
];
const FLAGS: u32 = 0x244cd7;
impl Form {
    fn descriptor(self) -> u32 {
        if self.immediate {
            0x01000420
        } else {
            0x0100041f
        }
    }
}
fn encoding(
    form: Form,
    (destination, indices, source): (u8, u8, u8),
    immediate: u8,
    memory: bool,
) -> Vec<u8> {
    let vvvv = if form.immediate { 15 } else { 15 - indices };
    let mut bytes = vec![
        0xc4,
        0xe0 | form.map,
        vvvv << 3 | 5 | if form.immediate { 0x80 } else { 0 },
        form.opcode,
        destination << 3 | if memory { 0 } else { 0xc0 | source },
    ];
    if form.immediate {
        bytes.push(immediate);
    }
    bytes
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
    packet[..12].copy_from_slice(&[b'R', b'3', b'M', b'H', 7, 0, 1, 0, 40, 0, 0, 0]);
    let fields = if fault {
        [1, 0, 1, 0x5000, 1, length]
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

fn qwords(values: &[u64]) -> [u8; 32] {
    let mut result = [0; 32];
    for (lane, value) in values.iter().enumerate() {
        result[lane * 8..lane * 8 + 8].copy_from_slice(&value.to_le_bytes());
    }
    result
}
fn raw(register: usize) -> [u8; 32] {
    let values = [
        0x7f800001, 0x80000000, 1, 0xdeadbeef, 0x7fc01234, 0xff800001, 0x3f800000, 0x12345678,
    ];
    words(&std::array::from_fn::<_, 8, _>(|lane| {
        values[(lane + register) % 8]
    }))
}
fn indices(rotation: usize, poison: u32) -> [u8; 32] {
    words(&std::array::from_fn::<_, 8, _>(|lane| {
        ((7 + rotation - lane) % 8) as u32 | poison & !7
    }))
}
fn select(source: [u8; 32], selections: &[usize], width: usize) -> [u8; 32] {
    let mut output = [0; 32];
    for (destination, source_lane) in selections.iter().enumerate() {
        output[destination * width..(destination + 1) * width]
            .copy_from_slice(&source[source_lane * width..(source_lane + 1) * width]);
    }
    output
}
fn variable(source: [u8; 32], control: [u8; 32]) -> [u8; 32] {
    select(
        source,
        &std::array::from_fn::<_, 8, _>(|lane| usize::from(control[lane * 4] & 7)),
        4,
    )
}
fn immediate(source: [u8; 32], control: u8) -> [u8; 32] {
    select(
        source,
        &std::array::from_fn::<_, 4, _>(|lane| usize::from(control >> (lane * 2) & 3)),
        8,
    )
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

#[test]
fn whole_vector_encodings_keep_indices_data_and_immediate_roles_distinct() {
    for form in FORMS {
        for register in 0..8 {
            for roles in aliases(register) {
                let (destination, indices, source) = roles;
                let canonical = decoded(&encoding(form, roles, 0x1b, false));
                assert_ne!(
                    canonical,
                    decoded(&encoding(
                        form,
                        ((destination + 1) % 8, indices, source),
                        0x1b,
                        false
                    ))
                );
                assert_ne!(
                    canonical,
                    decoded(&encoding(
                        form,
                        (destination, indices, (source + 1) % 8),
                        0x1b,
                        false
                    ))
                );
                if form.immediate {
                    assert_ne!(canonical, decoded(&encoding(form, roles, 0xe4, false)));
                    assert_eq!(
                        canonical,
                        decoded(&encoding(
                            form,
                            (destination, (indices + 1) % 8, source),
                            0x1b,
                            false
                        ))
                    );
                } else {
                    assert_ne!(
                        canonical,
                        decoded(&encoding(
                            form,
                            (destination, (indices + 1) % 8, source),
                            0x1b,
                            false
                        ))
                    );
                }
            }
        }
        let bytes = encoding(form, (1, 2, 3), 0x1b, false);
        for changed in [bytes[2] ^ 0x80, bytes[2] & !4] {
            let mut invalid = bytes.clone();
            invalid[2] = changed;
            assert_eq!(
                decode_one(&code(&invalid), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
        if form.immediate {
            let mut invalid = bytes.clone();
            invalid[2] &= !8;
            assert_eq!(
                decode_one(&code(&invalid), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
        let other = Form {
            opcode: if form.immediate {
                form.opcode ^ 1
            } else {
                form.opcode ^ 0x20
            },
            ..form
        };
        assert_eq!(
            decoded(&bytes),
            decoded(&encoding(other, (1, 2, 3), 0x1b, false))
        );
    }
}

#[test]
fn variable_low_three_bits_select_either_half_before_any_alias_write() {
    let source = words(&[10, 11, 12, 13, 20, 21, 22, 23]);
    let control = words(&[7, 4, 1, 6, 0, 5, 2, 3]);
    assert_eq!(
        variable(source, control),
        words(&[23, 20, 11, 22, 10, 21, 12, 13])
    );
    let mut engine = engine();
    for register in 0..8 {
        for (destination, index, source) in aliases(register) {
            for rotation in 0..8 {
                for poison in [0, 0xfffffff8, 0x80000000] {
                    let mut vectors = std::array::from_fn(raw);
                    vectors[usize::from(index)] = indices(rotation, poison);
                    let output =
                        variable(vectors[usize::from(source)], vectors[usize::from(index)]);
                    let before = seed(&engine, &vectors);
                    engine.arena_mut().unwrap().copy_from_slice(&before);
                    request(
                        &mut engine,
                        0x0100041f,
                        u32::from(destination) | u32::from(index) << 3 | u32::from(source) << 6,
                        0,
                        Some(output),
                        0,
                        false,
                    );
                }
            }
        }
    }
}

#[test]
fn each_immediate_pair_selects_any_original_qword_under_aliases() {
    let source = qwords(&[10, 11, 20, 21]);
    assert_eq!(immediate(source, 0x1b), qwords(&[21, 20, 11, 10]));
    assert_eq!(immediate(source, 0xaa), qwords(&[20, 20, 20, 20]));
    let mut engine = engine();
    for register in 0_u8..8 {
        for destination in [register, (register + 1) % 8] {
            for control in 0..=255 {
                let vectors = std::array::from_fn(raw);
                let output = immediate(vectors[usize::from(register)], control);
                let before = seed(&engine, &vectors);
                engine.arena_mut().unwrap().copy_from_slice(&before);
                request(
                    &mut engine,
                    0x01000420,
                    u32::from(destination) | u32::from(register) << 6,
                    u32::from(control),
                    Some(output),
                    0,
                    false,
                );
            }
        }
    }
}

#[test]
fn memory_data_requires_all_thirty_two_bytes_even_when_only_low_lanes_selected() {
    for form in FORMS {
        let mut bytes = encoding(form, (1, 2, 0), 0, true);
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

        let mut engine = engine();
        let input = raw(0);
        upload(&mut engine, 0x4fe0, &input);
        for (address, fault) in [(0x4fe0_u32, false), (0x4fe1, true), (0x4ff0, true)] {
            let vectors = [[0; 32]; 8];
            let mut before = seed(&engine, &vectors);
            let metadata = &mut before[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568];
            metadata.fill(0);
            metadata[..12].copy_from_slice(&[b'R', b'3', b'A', b'M', 1, 0, 1, 0, 24, 0, 0, 0]);
            metadata[16..20].copy_from_slice(&3_u32.to_le_bytes());
            metadata[20..24].copy_from_slice(&address.to_le_bytes());
            engine.arena_mut().unwrap().copy_from_slice(&before);
            let output = if fault {
                None
            } else {
                Some(if form.immediate {
                    immediate(input, 0)
                } else {
                    variable(input, [0; 32])
                })
            };
            request(
                &mut engine,
                form.descriptor(),
                1 << 12,
                0,
                output,
                32,
                fault,
            );
        }
    }
}

#[test]
fn old_avx1_variable_and_immediate_ymm_forms_remain_lane_local() {
    let dwords = words(&[10, 11, 12, 13, 20, 21, 22, 23]);
    let qwords_data = qwords(&[10, 11, 20, 21]);
    let mut engine = engine();
    for (kind, bytes, source, control, expected_value, immediate_value) in [
        (
            14,
            vec![0xc4, 0xe2, 0x75, 0x0c, 0xc2],
            dwords,
            words(&[7, 4, 1, 6, 0, 5, 2, 3]),
            words(&[13, 10, 11, 12, 20, 21, 22, 23]),
            0,
        ),
        (
            16,
            vec![0xc4, 0xe2, 0x75, 0x0d, 0xc2],
            qwords_data,
            qwords(&[2, 0, 2, 0]),
            qwords(&[11, 10, 21, 20]),
            0,
        ),
        (
            18,
            vec![0xc4, 0xe3, 0x7d, 0x04, 0xc1, 0x1b],
            dwords,
            [0; 32],
            words(&[13, 12, 11, 10, 23, 22, 21, 20]),
            0x1b,
        ),
        (
            20,
            vec![0xc4, 0xe3, 0x7d, 0x05, 0xc1, 0x05],
            qwords_data,
            [0; 32],
            qwords(&[11, 10, 21, 20]),
            0x05,
        ),
    ] {
        decoded(&bytes);
        let mut vectors = [[0x91; 32]; 8];
        vectors[1] = source;
        vectors[2] = control;
        let before = seed(&engine, &vectors);
        engine.arena_mut().unwrap().copy_from_slice(&before);
        let operands = if kind <= 16 { 1 << 3 | 2 << 6 } else { 1 << 6 };
        request(
            &mut engine,
            0x01000400 | kind,
            operands,
            immediate_value,
            Some(expected_value),
            0,
            false,
        );
    }
}

#[test]
fn forbidden_legacy_prefixes_remain_invalid() {
    for form in FORMS {
        for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
            let mut bytes = vec![prefix];
            bytes.extend(encoding(form, (1, 2, 3), 0x1b, false));
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
    }
}

#[test]
fn reserved_slots_and_unknown_templates_refuse_without_cpu_or_ram_changes() {
    let mut engine = engine();
    let before_ram = ram(&engine);
    for (descriptor, operands, immediate) in [
        (0x0100041f, 1 << 9, 0),
        (0x0100041f, 1 << 13, 0),
        (0x0100041f, 1 << 12 | 1 << 6, 0),
        (0x0100041f, 0, 1),
        (0x01000420, 1 << 3, 0),
        (0x01000420, 1 << 9, 0),
        (0x01000420, 1 << 13, 0),
        (0x01000420, 1 << 12 | 1 << 6, 0),
        (0x01000421, 0, 0),
        (0x0200041f, 0, 0),
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
