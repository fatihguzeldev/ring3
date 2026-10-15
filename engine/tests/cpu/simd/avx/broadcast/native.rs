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

#[derive(Clone, Copy)]
struct Form {
    opcode: u8,
    bytes: usize,
    wide: bool,
    memory_kind: Option<u8>,
    register_kind: u8,
}

const FORMS: &[Form] = &[
    Form {
        opcode: 0x78,
        bytes: 1,
        wide: false,
        memory_kind: Some(8),
        register_kind: 8,
    },
    Form {
        opcode: 0x78,
        bytes: 1,
        wide: true,
        memory_kind: Some(9),
        register_kind: 9,
    },
    Form {
        opcode: 0x79,
        bytes: 2,
        wide: false,
        memory_kind: Some(10),
        register_kind: 10,
    },
    Form {
        opcode: 0x79,
        bytes: 2,
        wide: true,
        memory_kind: Some(11),
        register_kind: 11,
    },
    Form {
        opcode: 0x58,
        bytes: 4,
        wide: false,
        memory_kind: Some(1),
        register_kind: 13,
    },
    Form {
        opcode: 0x58,
        bytes: 4,
        wide: true,
        memory_kind: Some(2),
        register_kind: 14,
    },
    Form {
        opcode: 0x59,
        bytes: 8,
        wide: false,
        memory_kind: Some(12),
        register_kind: 12,
    },
    Form {
        opcode: 0x59,
        bytes: 8,
        wide: true,
        memory_kind: Some(3),
        register_kind: 15,
    },
    Form {
        opcode: 0x18,
        bytes: 4,
        wide: false,
        memory_kind: None,
        register_kind: 13,
    },
    Form {
        opcode: 0x18,
        bytes: 4,
        wide: true,
        memory_kind: None,
        register_kind: 14,
    },
    Form {
        opcode: 0x19,
        bytes: 8,
        wide: true,
        memory_kind: None,
        register_kind: 15,
    },
];

fn bytes(form: Form, destination: u8, source: Option<u8>) -> Vec<u8> {
    vec![
        0xc4,
        0xe2,
        if form.wide { 0x7d } else { 0x79 },
        form.opcode,
        destination << 3 | source.map_or(0, |source| 0xc0 | source),
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

fn literal(element: usize) -> [u8; 32] {
    // independent raw boundary literals; no numerical evaluation or production oracle.
    match element {
        1 => [0x80; 32],
        2 => [
            1, 0x80, 1, 0x80, 1, 0x80, 1, 0x80, 1, 0x80, 1, 0x80, 1, 0x80, 1, 0x80, 1, 0x80, 1,
            0x80, 1, 0x80, 1, 0x80, 1, 0x80, 1, 0x80, 1, 0x80, 1, 0x80,
        ],
        4 => [
            1, 0, 0x80, 0x7f, 1, 0, 0x80, 0x7f, 1, 0, 0x80, 0x7f, 1, 0, 0x80, 0x7f, 1, 0, 0x80,
            0x7f, 1, 0, 0x80, 0x7f, 1, 0, 0x80, 0x7f, 1, 0, 0x80, 0x7f,
        ],
        8 => [
            1, 0, 0, 0, 0, 0, 0xf0, 0x7f, 1, 0, 0, 0, 0, 0, 0xf0, 0x7f, 1, 0, 0, 0, 0, 0, 0xf0,
            0x7f, 1, 0, 0, 0, 0, 0, 0xf0, 0x7f,
        ],
        _ => unreachable!(),
    }
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
) -> Result<StoreCompletion, HostError> {
    engine.avx_execute(0x01000e00 | u32::from(kind), operands, 0, 0, 0, FLAGS)
}

#[test]
fn new_forms_keep_canonical_roles_aliases_and_width() {
    assert_eq!(FORMS.len(), 11);
    assert_eq!(
        FORMS
            .iter()
            .map(|f| 1 + usize::from(f.memory_kind.is_some()))
            .sum::<usize>(),
        19
    );
    for &form in FORMS {
        for destination in 0..8 {
            for source in 0..8 {
                let canonical = decode(&bytes(form, destination, Some(source)), PC).unwrap();
                assert!(matches!(canonical, Operation::Avx(_)));
                assert_ne!(
                    canonical,
                    decode(&bytes(form, (destination + 1) % 8, Some(source)), PC).unwrap()
                );
                assert_ne!(
                    canonical,
                    decode(&bytes(form, destination, Some((source + 1) % 8)), PC).unwrap()
                );
            }
            if form.memory_kind.is_some() {
                let canonical = decode(&bytes(form, destination, None), PC).unwrap();
                assert_ne!(
                    canonical,
                    decode(&bytes(form, destination, Some(0)), PC).unwrap()
                );
            }
        }
    }
    for (integer, float) in [(0x58, 0x18), (0x59, 0x19)] {
        for wide in [false, true] {
            if integer == 0x59 && !wide {
                continue;
            }
            let control = if wide { 0x7d } else { 0x79 };
            assert_eq!(
                decode(&[0xc4, 0xe2, control, integer, 0xcb], PC),
                decode(&[0xc4, 0xe2, control, float, 0xcb], PC)
            );
            assert_eq!(
                decode(&[0xc4, 0xe2, control, integer, 0x08], PC),
                decode(&[0xc4, 0xe2, control, float, 0x08], PC)
            );
        }
    }
}

#[test]
fn register_helpers_copy_only_the_low_element_before_alias_publication() {
    let mut engine = engine();
    let before_ram = ram(&engine);
    for &form in FORMS {
        for destination in 0..8 {
            for source in 0..8 {
                for poison in [0x31, 0xe2] {
                    let mut vectors = std::array::from_fn(|r| [0x11 + r as u8 * 7; 32]);
                    vectors[source].fill(poison);
                    vectors[source][..form.bytes]
                        .copy_from_slice(&literal(form.bytes)[..form.bytes]);
                    let before = seeded(&engine, &vectors);
                    engine.arena_mut().unwrap().copy_from_slice(&before);
                    let mut value = literal(form.bytes);
                    if !form.wide {
                        value[16..].fill(0);
                    }
                    let expected = expected(&before, Some((destination, value)), [0; 6]);
                    assert_eq!(
                        invoke(
                            &mut engine,
                            form.register_kind,
                            destination as u32 | (source as u32) << 6
                        ),
                        Ok(StoreCompletion::Complete)
                    );
                    assert_eq!(
                        engine.arena(),
                        expected,
                        "kind{} d{destination} s{source} poison{poison}",
                        form.register_kind
                    );
                    assert_eq!(ram(&engine), before_ram);
                }
            }
        }
    }
}

#[test]
fn memory_helpers_read_exact_elements_and_publish_no_partial_fault_result() {
    let mut engine = engine();
    for &form in FORMS.iter().filter(|f| f.memory_kind.is_some()) {
        let source = literal(form.bytes);
        let end = 0x6000 - form.bytes as u32;
        upload(&mut engine, end, &source[..form.bytes]);
        let before_ram = ram(&engine);
        for (address, status, fault) in [
            (end, 0, 0),
            (
                0xffffffff,
                if form.bytes == 1 { 9 } else { 3 },
                if form.bytes == 1 { 0xffffffff } else { 0 },
            ),
        ]
        .into_iter()
        .chain((form.bytes > 1).then_some((end + 1, 9, 0x6000)))
        {
            let vectors = std::array::from_fn(|r| [0x91 + r as u8; 32]);
            let mut before = seeded(&engine, &vectors);
            metadata(&mut before, address);
            engine.arena_mut().unwrap().copy_from_slice(&before);
            let mut value = source;
            if !form.wide {
                value[16..].fill(0);
            }
            let output = (status == 0).then_some((7, value));
            let fields = [
                status,
                0,
                if status == 9 { 4 } else { 0 },
                fault,
                u32::from(status == 9),
                form.bytes as u32,
            ];
            let expected = expected(&before, output, fields);
            assert_eq!(
                invoke(&mut engine, form.memory_kind.unwrap(), 7 | 1 << 12),
                Ok(StoreCompletion::Complete)
            );
            assert_eq!(
                engine.arena(),
                expected,
                "element{} address{address:08x}",
                form.bytes
            );
            assert_eq!(ram(&engine), before_ram);
        }
    }
}

#[test]
fn complete_capture_and_fetch_faults_use_only_instruction_authority() {
    for &form in FORMS {
        for source in [Some(3), None]
            .into_iter()
            .filter(|s| s.is_some() || form.memory_kind.is_some())
        {
            let encoded = bytes(form, 1, source);
            let mut engine = EngineInstance::new(1, 91).unwrap();
            engine.map(PC, 1, 7).unwrap();
            let program = [encoded.as_slice(), &[0xeb, 0xfe]].concat();
            upload(&mut engine, PC, &program);
            engine.protect(PC, 1, 4).unwrap();
            describe(&mut engine, program.len());
            let before = engine.arena().to_vec();
            assert_eq!(engine.compile(1), Ok(1));
            assert_eq!(engine.arena(), before);
            assert!(decode(&encoded, 0x2000 - encoded.len() as u32).is_ok());
            for length in 1..encoded.len() {
                let pc = 0x2000 - length as u32;
                fetch_fault::assert_error(
                    decode(&encoded[..length], pc).unwrap_err(),
                    pc,
                    0x2000,
                    FaultReason::Unmapped,
                    length as u32 + 1,
                );
            }
        }
    }
}

#[test]
fn strict_encodings_keep_reserved_fields_and_mode_neighbors_invalid() {
    for &form in FORMS {
        for source in [Some(3), None]
            .into_iter()
            .filter(|s| s.is_some() || form.memory_kind.is_some())
        {
            let canonical = bytes(form, 1, source);
            for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
                let mut invalid = vec![prefix];
                invalid.extend(&canonical);
                assert_eq!(decode(&invalid, PC), Err(DecodeError::InvalidEncoding));
            }
            for change in [0x80, 8] {
                let mut invalid = canonical.clone();
                invalid[2] ^= change;
                assert_eq!(decode(&invalid, PC), Err(DecodeError::InvalidEncoding));
            }
        }
    }
    for opcode in [0x19, 0x5a] {
        for modrm in [0x08, 0xcb] {
            assert_eq!(
                decode(&[0xc4, 0xe2, 0x79, opcode, modrm], PC),
                Err(DecodeError::InvalidEncoding)
            );
        }
    }
}

#[test]
fn old_memory_and_integer_block_aliases_keep_their_canonical_meaning() {
    for (integer, float) in [
        (
            &[0xc4, 0xe2, 0x7d, 0x5a, 0x08][..],
            &[0xc4, 0xe2, 0x7d, 0x1a, 0x08][..],
        ),
        (
            &[0xc4, 0xe3, 0x6d, 0x38, 0xcb, 1][..],
            &[0xc4, 0xe3, 0x6d, 0x18, 0xcb, 1][..],
        ),
        (
            &[0xc4, 0xe3, 0x7d, 0x39, 0xcb, 1][..],
            &[0xc4, 0xe3, 0x7d, 0x19, 0xcb, 1][..],
        ),
        (
            &[0xc4, 0xe3, 0x6d, 0x46, 0xcb, 0x21][..],
            &[0xc4, 0xe3, 0x6d, 0x06, 0xcb, 0x21][..],
        ),
    ] {
        let a = decode(integer, PC).unwrap();
        let b = decode(float, PC).unwrap();
        assert_eq!(a, b);
    }
    for code in [
        &[0xc4, 0xe2, 0x79, 0x18, 0x08][..],
        &[0xc4, 0xe2, 0x7d, 0x18, 0x08][..],
        &[0xc4, 0xe2, 0x7d, 0x19, 0x08][..],
    ] {
        assert!(matches!(decode(code, PC), Ok(Operation::Avx(_))));
        let mut invalid = code.to_vec();
        invalid[2] |= 0x80;
        assert_eq!(decode(&invalid, PC), Err(DecodeError::InvalidEncoding));
    }
}

#[test]
fn old_helper_memory_policy_and_raw_literals_remain_neutral() {
    let mut engine = engine();
    for &(kind, element, wide) in &[(1, 4, false), (2, 4, true), (3, 8, true)] {
        let source = literal(element);
        upload(&mut engine, 0x5000, &source[..element]);
        let vectors = std::array::from_fn(|r| [0x41 + r as u8; 32]);
        let mut before = seeded(&engine, &vectors);
        metadata(&mut before, 0x5000);
        engine.arena_mut().unwrap().copy_from_slice(&before);
        let before_ram = ram(&engine);
        let mut value = source;
        if !wide {
            value[16..].fill(0);
        }
        let expected = expected(&before, Some((7, value)), [0, 0, 0, 0, 0, element as u32]);
        assert_eq!(
            invoke(&mut engine, kind, 7 | 1 << 12),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(engine.arena(), expected);
        assert_eq!(ram(&engine), before_ram);
        for operands in [
            7,
            7 | 1 << 6,
            7 | 1 << 12 | 1 << 6,
            7 | 1 << 13,
            7 | 1 << 12 | 1 << 3,
        ] {
            let before = engine.arena().to_vec();
            assert_eq!(
                invoke(&mut engine, kind, operands),
                Err(HostError::InvalidRequest)
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(ram(&engine), before_ram);
        }
    }
}
