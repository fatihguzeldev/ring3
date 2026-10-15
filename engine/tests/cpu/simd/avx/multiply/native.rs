use crate::fetch_fault;

use ring3_engine::{
    abi::{
        arena::{ARENA_SIZE, HELPER_OFFSET, SIMD_OFFSET, TRANSFER_OFFSET, YMM_OFFSET},
        x86::{SIMD_SIZE, YMM_SIZE, encode_simd, encode_state_v2, encode_ymm},
    },
    cpu::x86::{
        State32,
        decode::{DecodeError, decode_one},
        ir::Operation,
        simd::{SimdState, ymm::YmmUpperState},
    },
    memory::{AddressSpace, FaultReason, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, HostError, StoreCompletion},
};

const PC: u32 = 0x1000;
const FLAGS: u32 = 0x244cd7;

const FORMS: &[(u8, u8, u8)] = &[
    (1, 0xd5, 1),
    (1, 0xe4, 2),
    (1, 0xe5, 3),
    (1, 0xf4, 4),
    (1, 0xf5, 5),
    (1, 0xf6, 6),
    (2, 1, 7),
    (2, 2, 8),
    (2, 3, 9),
    (2, 4, 10),
    (2, 5, 11),
    (2, 6, 12),
    (2, 7, 13),
    (2, 0x0b, 14),
    (2, 0x28, 15),
    (2, 0x40, 16),
];

fn bytes(form: (u8, u8, u8), registers: (u8, u8, Option<u8>), wide: bool, w: bool) -> [u8; 5] {
    let (destination, first, second) = registers;
    [
        0xc4,
        0xe0 | form.0,
        ((15 - first) << 3) | (u8::from(wide) << 2) | (u8::from(w) << 7) | 1,
        form.1,
        (destination << 3) | second.map_or(0, |source| 0xc0 | source),
    ]
}

fn memory(bytes: &[u8], pc: u32) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    let page = PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}

fn decode(bytes: &[u8], pc: u32) -> Result<Operation, DecodeError> {
    let memory = memory(bytes, pc);
    let instruction = decode_one(&memory, GuestAddress(pc))?;
    assert_eq!(usize::from(instruction.length()), bytes.len());
    assert_eq!(instruction.pc(), GuestAddress(pc));
    assert_eq!(instruction.next_pc(), GuestAddress(pc + bytes.len() as u32));
    assert!(memory.is_code_current(instruction.code_snapshot()));
    Ok(*instruction.operation())
}

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn describe(engine: &mut EngineInstance, length: usize) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
        .copy_from_slice(&PC.to_le_bytes());
    engine.arena_mut().unwrap()[TRANSFER_OFFSET + 4..TRANSFER_OFFSET + 8]
        .copy_from_slice(&(length as u32).to_le_bytes());
}

fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(2, 91).unwrap();
    engine.map(PC, 1, 7).unwrap();
    engine.map(0x5000, 1, 3).unwrap();
    upload(&mut engine, PC, &[0xeb, 0xfe]);
    for offset in (0..4096).step_by(512) {
        upload(&mut engine, 0x5000 + offset as u32, &[0x5a; 512]);
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
        .read(GuestAddress(PC), &mut bytes[..4096])
        .unwrap();
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x5000), &mut bytes[4096..])
        .unwrap();
    bytes
}

fn seeded(engine: &EngineInstance, vectors: &[[u8; 32]; 8]) -> Vec<u8> {
    let mut before = engine.arena().to_vec();
    assert_eq!(before.len(), ARENA_SIZE);
    before[TRANSFER_OFFSET..TRANSFER_OFFSET + 4096].fill(0xa5);
    let state = State32 {
        registers: std::array::from_fn(|r| 0xa1b20000 | (r as u32 * 0x101)),
        eip: PC,
        eflags: FLAGS,
    };
    encode_state_v2(&state, &mut before[..56]).unwrap();
    encode_simd(
        &SimdState {
            mxcsr: 0xffff,
            xmm: std::array::from_fn(|r| vectors[r][..16].try_into().unwrap()),
        },
        &mut before[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE],
    )
    .unwrap();
    encode_ymm(
        &YmmUpperState {
            registers: std::array::from_fn(|r| vectors[r][16..].try_into().unwrap()),
        },
        &mut before[YMM_OFFSET..YMM_OFFSET + YMM_SIZE],
    )
    .unwrap();
    before
}

fn metadata(frame: &mut [u8], offset: u32) {
    let bytes = &mut frame[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568];
    bytes.fill(0);
    bytes[..12].copy_from_slice(&[b'R', b'3', b'A', b'M', 1, 0, 1, 0, 24, 0, 0, 0]);
    bytes[16..20].copy_from_slice(&3_u32.to_le_bytes());
    bytes[20..24].copy_from_slice(&offset.to_le_bytes());
}

fn expected(before: &[u8], output: Option<(usize, [u8; 32])>, fields: [u32; 6]) -> Vec<u8> {
    let mut expected = before.to_vec();
    if let Some((destination, value)) = output {
        expected[SIMD_OFFSET + 32 + destination * 16..SIMD_OFFSET + 48 + destination * 16]
            .copy_from_slice(&value[..16]);
        expected[YMM_OFFSET + 16 + destination * 16..YMM_OFFSET + 32 + destination * 16]
            .copy_from_slice(&value[16..]);
        expected[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 544].fill(0);
        expected[TRANSFER_OFFSET + 516..TRANSFER_OFFSET + 520]
            .copy_from_slice(&FLAGS.to_le_bytes());
    }
    let packet = &mut expected[HELPER_OFFSET..HELPER_OFFSET + 40];
    packet.fill(0);
    let version = if fields[0] == 9 { 15 } else { 7 };
    packet[..12].copy_from_slice(&[b'R', b'3', b'M', b'H', version, 0, 1, 0, 40, 0, 0, 0]);
    for (index, field) in fields.into_iter().enumerate() {
        packet[16 + index * 4..20 + index * 4].copy_from_slice(&field.to_le_bytes());
    }
    expected
}

fn invoke(
    engine: &mut EngineInstance,
    kind: u8,
    operands: u32,
    wide: bool,
) -> Result<StoreCompletion, HostError> {
    engine.avx_execute(
        0x01000800 | u32::from(kind) | if wide { 0x80 } else { 0 },
        operands,
        0,
        0,
        0,
        FLAGS,
    )
}

fn unsigned(bytes: &[u8], offset: usize, width: usize) -> u64 {
    let mut value = [0; 8];
    value[..width].copy_from_slice(&bytes[offset..offset + width]);
    u64::from_le_bytes(value)
}
fn signed(bytes: &[u8], offset: usize, width: usize) -> i128 {
    let value = unsigned(bytes, offset, width);
    i128::from(((value << (64 - width * 8)) as i64) >> (64 - width * 8))
}
fn put(bytes: &mut [u8], offset: usize, width: usize, value: i128) {
    bytes[offset..offset + width].copy_from_slice(&value.to_le_bytes()[..width]);
}

// independent integer equations; never invokes a Ring3 semantic kernel for expected values.
fn oracle(kind: u8, a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let mut out = [0; 32];
    for half in [0, 16] {
        match kind {
            4 | 15 => {
                for offset in (half..half + 16).step_by(8) {
                    let x = if kind == 15 {
                        signed(a, offset, 4)
                    } else {
                        i128::from(unsigned(a, offset, 4))
                    };
                    let y = if kind == 15 {
                        signed(b, offset, 4)
                    } else {
                        i128::from(unsigned(b, offset, 4))
                    };
                    put(&mut out, offset, 8, x * y);
                }
            }
            5 => {
                for offset in (half..half + 16).step_by(4) {
                    put(
                        &mut out,
                        offset,
                        4,
                        signed(a, offset, 2) * signed(b, offset, 2)
                            + signed(a, offset + 2, 2) * signed(b, offset + 2, 2),
                    );
                }
            }
            6 => {
                for offset in (half..half + 16).step_by(8) {
                    let sum = (offset..offset + 8)
                        .map(|i| i128::from(a[i].abs_diff(b[i])))
                        .sum();
                    put(&mut out, offset, 8, sum);
                }
            }
            7..=9 | 11..=13 => {
                let width = if kind == 8 || kind == 12 { 4 } else { 2 };
                let half_lanes = 8 / width;
                for lane in 0..16 / width {
                    let input = if lane < half_lanes { a } else { b };
                    let at = half + lane % half_lanes * width * 2;
                    let x = signed(input, at, width);
                    let y = signed(input, at + width, width);
                    let value = if kind >= 11 { x - y } else { x + y };
                    let value = if kind == 9 || kind == 13 {
                        value.clamp(-32768, 32767)
                    } else {
                        value
                    };
                    put(&mut out, half + lane * width, width, value);
                }
            }
            10 => {
                for offset in (half..half + 16).step_by(2) {
                    let value = i128::from(a[offset]) * i128::from(b[offset] as i8)
                        + i128::from(a[offset + 1]) * i128::from(b[offset + 1] as i8);
                    put(&mut out, offset, 2, value.clamp(-32768, 32767));
                }
            }
            1..=3 | 14 | 16 => {
                let width = if kind == 16 { 4 } else { 2 };
                for offset in (half..half + 16).step_by(width) {
                    let value = match kind {
                        1 | 16 => {
                            i128::from(unsigned(a, offset, width))
                                * i128::from(unsigned(b, offset, width))
                        }
                        2 => {
                            (i128::from(unsigned(a, offset, 2))
                                * i128::from(unsigned(b, offset, 2)))
                                >> 16
                        }
                        3 => (signed(a, offset, 2) * signed(b, offset, 2)) >> 16,
                        14 => (signed(a, offset, 2) * signed(b, offset, 2) + 0x4000) >> 15,
                        _ => unreachable!(),
                    };
                    put(&mut out, offset, width, value);
                }
            }
            _ => unreachable!("closed sixteen kinds"),
        }
    }
    out
}

fn pattern(register: usize, seed: usize) -> [u8; 32] {
    const VALUES: [u8; 8] = [0, 255, 128, 127, 1, 254, 129, 85];
    std::array::from_fn(|byte| {
        VALUES[(byte + register * 3 + seed + (byte / 16) * 5) % VALUES.len()]
    })
}

#[test]
fn every_wide_form_preserves_roles_and_accepts_both_wig_encodings() {
    assert_eq!(FORMS.len(), 16);
    for &form in FORMS {
        for index in 0..8 {
            let destination = index;
            let first = (index + 1) % 8;
            let source = (index + 3) % 8;
            for second in [Some(source), None] {
                let canonical =
                    decode(&bytes(form, (destination, first, second), true, false), PC).unwrap();
                assert!(matches!(canonical, Operation::Avx(_)));
                assert_eq!(
                    canonical,
                    decode(&bytes(form, (destination, first, second), true, true), PC).unwrap()
                );
                assert_ne!(
                    canonical,
                    decode(
                        &bytes(form, ((destination + 1) % 8, first, second), true, false),
                        PC
                    )
                    .unwrap()
                );
                assert_ne!(
                    canonical,
                    decode(
                        &bytes(form, (destination, (first + 1) % 8, second), true, false),
                        PC
                    )
                    .unwrap()
                );
                assert_ne!(
                    canonical,
                    decode(
                        &bytes(
                            form,
                            (destination, first, Some((source + 1) % 8)),
                            true,
                            false
                        ),
                        PC
                    )
                    .unwrap()
                );
            }
        }
    }
}

#[test]
fn wide_helper_products_capture_all_aliases_and_publish_both_halves() {
    let mut engine = engine();
    let before_ram = ram(&engine);
    for &(_, _, kind) in FORMS {
        for (destination, first, second) in [(0, 1, 2), (1, 1, 2), (2, 1, 2), (0, 1, 1), (0, 0, 0)]
        {
            for seed in [0, 3] {
                let vectors = std::array::from_fn(|r| pattern(r, seed));
                let value = oracle(kind, &vectors[first], &vectors[second]);
                let before = seeded(&engine, &vectors);
                engine.arena_mut().unwrap().copy_from_slice(&before);
                let expected = expected(&before, Some((destination, value)), [0; 6]);
                assert_eq!(
                    invoke(
                        &mut engine,
                        kind,
                        destination as u32 | ((first as u32) << 3) | ((second as u32) << 6),
                        true
                    ),
                    Ok(StoreCompletion::Complete)
                );
                assert_eq!(
                    engine.arena(),
                    expected,
                    "kind{kind} aliases{destination}/{first}/{second} seed{seed}"
                );
                assert_eq!(ram(&engine), before_ram);
            }
        }
    }
}

#[test]
fn wide_memory_reads_exact32_before_any_result_publication() {
    let mut engine = engine();
    let operand = pattern(3, 1);
    upload(&mut engine, 0x5fe0, &operand);
    let before_ram = ram(&engine);
    for &(_, _, kind) in FORMS {
        for (address, status, fault) in [(0x5fe0, 0, 0), (0x5fe1, 9, 0x6000), (0xffffffff, 3, 0)] {
            let vectors = std::array::from_fn(|r| pattern(r, 0));
            let value = oracle(kind, &vectors[1], &operand);
            let mut before = seeded(&engine, &vectors);
            metadata(&mut before, address);
            engine.arena_mut().unwrap().copy_from_slice(&before);
            let fields = [
                status,
                0,
                if status == 9 { 4 } else { 0 },
                fault,
                u32::from(status == 9),
                32,
            ];
            let expected = expected(&before, (status == 0).then_some((1, value)), fields);
            assert_eq!(
                invoke(&mut engine, kind, 1 | (1 << 3) | (1 << 12), true),
                Ok(StoreCompletion::Complete)
            );
            assert_eq!(engine.arena(), expected, "kind{kind} address{address:x}");
            assert_eq!(ram(&engine), before_ram);
        }
    }
}

#[test]
fn full_capture_and_page_last_fetch_do_not_access_operand_ram() {
    for &form in FORMS {
        for source in [Some(3), None] {
            let encoded = bytes(form, (1, 2, source), true, false);
            let mut engine = EngineInstance::new(1, 91).unwrap();
            engine.map(PC, 1, 7).unwrap();
            let program = [encoded.as_slice(), &[0xeb, 0xfe]].concat();
            upload(&mut engine, PC, &program);
            engine.protect(PC, 1, 4).unwrap();
            describe(&mut engine, program.len());
            let before = engine.arena().to_vec();
            assert_eq!(engine.compile(1), Ok(1));
            assert_eq!(engine.arena(), before);
            assert!(decode(&encoded, 0x1ffb).is_ok());
            fetch_fault::assert_error(
                decode(&encoded[..4], 0x1ffc).unwrap_err(),
                0x1ffc,
                0x2000,
                FaultReason::Unmapped,
                5,
            );
        }
    }
}

#[test]
fn original_xmm_shapes_wig_and_invalid_prefix_controls_stay_precise() {
    for &form in FORMS {
        for source in [Some(3), None] {
            let bytes0 = bytes(form, (1, 2, source), false, false);
            assert_eq!(
                decode(&bytes0, PC),
                decode(&bytes(form, (1, 2, source), false, true), PC)
            );
            assert!(decode(&bytes0, PC).is_ok());
            for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
                let mut invalid = vec![prefix];
                invalid.extend(bytes(form, (1, 2, source), true, false));
                assert_eq!(decode(&invalid, PC), Err(DecodeError::InvalidEncoding));
            }
        }
    }
    assert_eq!(
        decode(&[0xc4, 0xe2, 0x7d, 0x41, 0xcb], PC),
        Err(DecodeError::InvalidEncoding)
    );
    assert!(matches!(
        decode(&[0xc4, 0xe3, 0x6d, 0x42, 0xcb, 0x29], PC),
        Ok(Operation::Avx(_))
    ));
}

#[test]
fn original_xmm_helpers_retain_exact_result_upper_zero_and_full_frame() {
    let mut engine = engine();
    let before_ram = ram(&engine);
    for &(_, _, kind) in FORMS {
        let vectors = std::array::from_fn(|r| pattern(r, 0));
        let before = seeded(&engine, &vectors);
        engine.arena_mut().unwrap().copy_from_slice(&before);
        let mut value = oracle(kind, &vectors[1], &vectors[2]);
        value[16..].fill(0);
        let expected = expected(&before, Some((0, value)), [0; 6]);
        assert_eq!(
            invoke(&mut engine, kind, (1 << 3) | (2 << 6), false),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(engine.arena(), expected, "old XMMkind{kind}");
        assert_eq!(ram(&engine), before_ram);
    }
    let before = engine.arena().to_vec();
    assert_eq!(
        invoke(&mut engine, 0x91, 1 | (2 << 3) | (3 << 6), false),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(ram(&engine), before_ram);
}

#[test]
fn independent_oracle_literals_preserve_wrap_rounding_saturation_and_group_bounds() {
    let minimum = std::array::from_fn(|i| if i % 2 == 0 { 0 } else { 0x80 });
    assert_eq!(
        oracle(5, &minimum, &minimum),
        std::array::from_fn(|i| if i % 4 == 3 { 0x80 } else { 0 })
    );
    assert_eq!(oracle(14, &minimum, &minimum), minimum);
    assert_eq!(
        oracle(6, &[0; 32], &[255; 32]),
        std::array::from_fn(|i| match i % 8 {
            0 => 0xf8,
            1 => 7,
            _ => 0,
        })
    );
    let signs = std::array::from_fn(|i| if i % 4 < 2 { 0x7f } else { 0x80 });
    assert_eq!(
        oracle(10, &[255; 32], &signs),
        std::array::from_fn(|i| [255, 127, 0, 128][i % 4])
    );
    let ascending: [u8; 32] = std::array::from_fn(|i| i as u8);
    let result = oracle(7, &ascending, &[0; 32]);
    assert_eq!(&result[8..16], &[0; 8]);
    assert_eq!(&result[24..], &[0; 8]);
    assert_eq!(result[16], 34); // first upperword is 0x1110+0x1312, not low-half replay.
}
