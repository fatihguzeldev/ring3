use ring3_engine::{
    abi::{
        arena::{HELPER_OFFSET, SEGMENTS_OFFSET, SIMD_OFFSET, TRANSFER_OFFSET, YMM_OFFSET},
        x86::{
            SEGMENTS_SIZE, SIMD_SIZE, YMM_SIZE, decode_simd, decode_ymm, encode_segment_context,
            encode_simd, encode_ymm,
        },
    },
    cpu::x86::{
        SegmentCache, SegmentContext,
        decode::{DecodeError, decode_one},
        ir::Operation,
        simd::{SimdState, ymm::YmmUpperState},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, HostError, StoreCompletion},
};

const KEY: u64 = 91;
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
    output[..12].copy_from_slice(&[b'R', b'3', b'M', b'H', 7, 0, 1, 0, 40, 0, 0, 0]);
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

fn encoding(destination: u8, wide: bool, w: bool, operand: &[u8], prefix: &[u8]) -> Vec<u8> {
    let mut bytes = prefix.to_vec();
    bytes.extend([
        0xc4,
        0xe2,
        0x79 | if wide { 4 } else { 0 } | if w { 0x80 } else { 0 },
        0x2a,
    ]);
    bytes.push(destination << 3 | operand[0]);
    bytes.extend_from_slice(&operand[1..]);
    bytes
}

#[test]
fn ymm_stream_load_decodes_as_the_existing_aligned_avx1_load_with_ignored_w() {
    for destination in 0..8 {
        for (prefix, operand) in [
            (&[][..], &[0][..]),
            (&[][..], &[4, 0x24][..]),
            (&[0x36][..], &[0][..]),
            (&[0x64][..], &[0][..]),
            (&[0x67][..], &[6, 0x20, 0x50][..]),
        ] {
            let bytes = encoding(destination, true, false, operand, prefix);
            let canonical = decoded(&bytes);
            assert_eq!(
                canonical,
                decoded(&encoding(destination, true, true, operand, prefix))
            );
            let mut ordinary = bytes.clone();
            ordinary[prefix.len() + 1] = 0xe1;
            ordinary[prefix.len() + 3] = 0x6f;
            assert_eq!(canonical, decoded(&ordinary));
        }
    }
}

#[test]
fn ymm_stream_capture_does_not_probe_unmapped_data_and_preserves_code_currentness() {
    let mut bytes = encoding(3, true, false, &[5, 0xe0, 0xbe, 0xad, 0xde], &[]);
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

fn seed(engine: &mut EngineInstance) {
    let simd = SimdState {
        mxcsr: 0xffff,
        xmm: std::array::from_fn(|r| [r as u8 * 29; 16]),
    };
    let upper = YmmUpperState {
        registers: std::array::from_fn(|r| [r as u8 * 31 + 3; 16]),
    };
    let arena = engine.arena_mut().unwrap();
    encode_simd(&simd, &mut arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
    encode_ymm(&upper, &mut arena[YMM_OFFSET..YMM_OFFSET + YMM_SIZE]).unwrap();
    arena[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 544].fill(0xa5);
}

fn call(
    engine: &mut EngineInstance,
    resident: Option<u64>,
    descriptor: u32,
    operands: u32,
) -> Result<StoreCompletion, HostError> {
    match resident {
        None => engine.avx_execute(descriptor, operands, 0, 0, 0, FLAGS),
        Some(id) => engine.avx_execute_resident(KEY, id, descriptor, operands, 0, 0, 0, FLAGS),
    }
}

#[test]
fn shared_aligned_ymm_helper_remains_available_with_exact_read_alignment_and_fault_neutrality() {
    let (mut engine, id) = helper_engine();
    let data: [u8; 32] = std::array::from_fn(|byte| (byte * 13 + 7) as u8);
    upload(&mut engine, 0x5fe0, &data);
    let before_ram = ram(&engine);
    for destination in 0..8 {
        for resident in [None, Some(id)] {
            for (offset, fields) in [
                (0x5fe0, [0, 0, 0, 0, 0, 32]),
                (0x5010, [3, 0, 0, 0, 0, 32]),
                (0x5ff0, [3, 0, 0, 0, 0, 32]),
                (0x6000, [1, 0, 1, 0x6000, 1, 32]),
            ] {
                seed(&mut engine);
                address(&mut engine, 3, offset);
                let before = engine.arena().to_vec();
                let expected = if fields[0] == 0 {
                    success(&before, destination, data, 32)
                } else {
                    let mut expected = before;
                    packet(&mut expected, fields);
                    expected
                };
                assert_eq!(
                    call(
                        &mut engine,
                        resident,
                        0x01000318,
                        destination as u32 | (1 << 12)
                    ),
                    Ok(StoreCompletion::Complete)
                );
                assert_eq!(
                    engine.arena(),
                    expected,
                    "D{destination} offset{offset:08x}"
                );
                assert_eq!(ram(&engine), before_ram);
            }
        }
    }
    for segment in [3, 2] {
        for (limit, permitted) in [(0x5fff, true), (0x5ffe, false)] {
            seed(&mut engine);
            let mut context = SegmentContext::default();
            context.caches[segment as usize] = SegmentCache::from_bits(0xf2300000 | limit).unwrap();
            encode_segment_context(
                &context,
                &mut engine.arena_mut().unwrap()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE],
            )
            .unwrap();
            address(&mut engine, segment, 0x5fe0);
            let before = engine.arena().to_vec();
            let expected = if permitted {
                success(&before, 0, data, 32)
            } else {
                let mut expected = before;
                packet(
                    &mut expected,
                    [if segment == 2 { 7 } else { 3 }, 0, 0, 0, 0, 32],
                );
                expected
            };
            assert_eq!(
                call(&mut engine, None, 0x01000318, 1 << 12),
                Ok(StoreCompletion::Complete)
            );
            assert_eq!(
                engine.arena(),
                expected,
                "exactlastsegmentbyte segment{segment}"
            );
            assert_eq!(ram(&engine), before_ram);
        }
    }
    for (logical, aligned) in [(0x5fd0, true), (0x5fe0, false)] {
        seed(&mut engine);
        let mut context = SegmentContext::default();
        context.state.selectors[4] = context.state.selectors[3];
        context.state.bases[4] = 0x10;
        context.caches[4] = context.caches[3];
        encode_segment_context(
            &context,
            &mut engine.arena_mut().unwrap()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE],
        )
        .unwrap();
        address(&mut engine, 4, logical);
        let before = engine.arena().to_vec();
        let expected = if aligned {
            success(&before, 0, data, 32)
        } else {
            let mut expected = before;
            packet(&mut expected, [3, 0, 0, 0, 0, 32]);
            expected
        };
        assert_eq!(
            call(&mut engine, None, 0x01000318, 1 << 12),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(engine.arena(), expected, "linear alignment after FS base");
        assert_eq!(ram(&engine), before_ram);
    }
    for (operands, immediate) in [
        (1 << 3, 0),
        (1 << 9, 0),
        (1 << 13, 0),
        (1 << 12 | 1 << 6, 0),
        (0, 1),
    ] {
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.avx_execute(0x01000318, operands, immediate, 0, 0, FLAGS),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), before);
    }
}

#[test]
fn existing_xmm_stream_keeps_sixteen_byte_alignment_read_and_upper_zero() {
    for w in [false, true] {
        decoded(&encoding(7, false, w, &[0], &[]));
    }
    let (mut engine, _) = helper_engine();
    let data: [u8; 16] = std::array::from_fn(|byte| (byte * 17 + 11) as u8);
    upload(&mut engine, 0x5ff0, &data);
    seed(&mut engine);
    address(&mut engine, 3, 0x5ff0);
    let before = engine.arena().to_vec();
    let mut result = [0; 32];
    result[..16].copy_from_slice(&data);
    let expected = success(&before, 0, result, 16);
    let before_ram = ram(&engine);
    assert_eq!(
        call(&mut engine, None, 0x0100032b, 1 << 12),
        Ok(StoreCompletion::Complete)
    );
    assert_eq!(engine.arena(), expected);
    assert_eq!(ram(&engine), before_ram);
}

#[test]
fn memory_only_reserved_vvvv_and_legacy_prefix_rules_remain_strict() {
    for wide in [false, true] {
        for w in [false, true] {
            let valid = encoding(0, wide, w, &[0], &[]);
            for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
                let mut bytes = vec![prefix];
                bytes.extend(&valid);
                assert_eq!(
                    decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                    DecodeError::InvalidEncoding
                );
            }
            for reserved in 0..15 {
                let mut bytes = valid.clone();
                bytes[2] = bytes[2] & !0x78 | (reserved << 3);
                assert_eq!(
                    decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                    DecodeError::InvalidEncoding
                );
            }
            let register = encoding(0, wide, w, &[0xc1], &[]);
            assert_eq!(
                decode_one(&code(&register), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
    }
}
