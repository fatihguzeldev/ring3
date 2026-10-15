use ring3_engine::{
    abi::{
        arena::{
            ARENA_SIZE, HELPER_OFFSET, SEGMENTS_OFFSET, SIMD_OFFSET, TRANSFER_OFFSET, YMM_OFFSET,
        },
        x86::{
            SEGMENTS_SIZE, SIMD_SIZE, YMM_SIZE, encode_segment_context, encode_simd,
            encode_state_v2, encode_ymm,
        },
    },
    cpu::x86::{
        SegmentCache, SegmentContext, SegmentRegister, State32,
        simd::{SimdState, ymm::YmmUpperState},
    },
    memory::GuestAddress,
    process::{EngineInstance, HostError, StoreCompletion},
};

const FLAGS: u32 = 0x244cd7;
const DESTINATION: usize = 1;
const INDEX: usize = 2;
const MASK: usize = 3;
const OPERANDS: u32 = 1 | (2 << 3) | (3 << 9) | (1 << 12);

#[derive(Clone, Copy, Debug)]
struct Geometry {
    id: u32,
    data: usize,
    index: usize,
    lanes: usize,
}

// Literal active-lane table, including both unequal-size forms.
const GEOMETRY: &[Geometry] = &[
    Geometry {
        id: 1,
        data: 4,
        index: 4,
        lanes: 4,
    },
    Geometry {
        id: 2,
        data: 4,
        index: 4,
        lanes: 8,
    },
    Geometry {
        id: 3,
        data: 8,
        index: 4,
        lanes: 2,
    },
    Geometry {
        id: 4,
        data: 8,
        index: 4,
        lanes: 4,
    },
    Geometry {
        id: 5,
        data: 4,
        index: 8,
        lanes: 2,
    },
    Geometry {
        id: 6,
        data: 4,
        index: 8,
        lanes: 4,
    },
    Geometry {
        id: 7,
        data: 8,
        index: 8,
        lanes: 2,
    },
    Geometry {
        id: 8,
        data: 8,
        index: 8,
        lanes: 4,
    },
];

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn engine() -> EngineInstance {
    // Code, regular gather data and a low32-wrap target each own one page.
    let mut engine = EngineInstance::new(3, 91).unwrap();
    for (address, permissions) in [(0, 3), (0x1000, 7), (0x5000, 3)] {
        engine.map(address, 1, permissions).unwrap();
    }
    upload(&mut engine, 0x1000, &[0xeb, 0xfe]);
    upload(
        &mut engine,
        0x10,
        &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88],
    );
    for offset in (0..4096).step_by(512) {
        let value: [u8; 512] = std::array::from_fn(|byte| (byte % 251) as u8);
        upload(&mut engine, 0x5000 + offset as u32, &value);
    }
    let arena = engine.arena_mut().unwrap();
    arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 8].copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    engine
}

fn ram(engine: &EngineInstance) -> Vec<u8> {
    let mut result = vec![0; 3 * 4096];
    for (index, address) in [0, 0x1000, 0x5000].into_iter().enumerate() {
        engine
            .memory()
            .unwrap()
            .read(
                GuestAddress(address),
                &mut result[index * 4096..(index + 1) * 4096],
            )
            .unwrap();
    }
    result
}

fn vectors(geometry: Geometry, selected: u8) -> [[u8; 32]; 8] {
    let mut values = std::array::from_fn(|register| {
        std::array::from_fn(|byte| (register * 19 + byte * 5 + 7) as u8)
    });
    values[INDEX].fill(0xee);
    values[MASK].fill(0xff);
    for lane in 0..geometry.lanes {
        put(
            &mut values[INDEX],
            lane * geometry.index,
            geometry.index,
            (lane * 8) as u64,
        );
        put(
            &mut values[MASK],
            lane * geometry.data,
            geometry.data,
            0x11223344,
        );
        values[MASK][(lane + 1) * geometry.data - 1] = if selected & (1 << lane) != 0 {
            0x80
        } else {
            0x7f
        };
    }
    values
}

fn put(bytes: &mut [u8], offset: usize, width: usize, value: u64) {
    bytes[offset..offset + width].copy_from_slice(&value.to_le_bytes()[..width]);
}

fn frame(
    engine: &EngineInstance,
    values: &[[u8; 32]; 8],
    segment: SegmentRegister,
    base: u32,
    context: &SegmentContext,
) -> Vec<u8> {
    let mut frame = engine.arena().to_vec();
    assert_eq!(frame.len(), ARENA_SIZE);
    frame[TRANSFER_OFFSET..TRANSFER_OFFSET + 4096].fill(0xa5);
    encode_state_v2(
        &State32 {
            registers: [0xabcdef01; 8],
            eip: 0x1000,
            eflags: FLAGS,
        },
        &mut frame[..56],
    )
    .unwrap();
    encode_simd(
        &SimdState {
            mxcsr: 0xffff,
            xmm: std::array::from_fn(|register| values[register][..16].try_into().unwrap()),
        },
        &mut frame[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE],
    )
    .unwrap();
    encode_ymm(
        &YmmUpperState {
            registers: std::array::from_fn(|register| values[register][16..].try_into().unwrap()),
        },
        &mut frame[YMM_OFFSET..YMM_OFFSET + YMM_SIZE],
    )
    .unwrap();
    encode_segment_context(
        context,
        &mut frame[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE],
    )
    .unwrap();
    let metadata = &mut frame[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568];
    metadata.fill(0);
    metadata[..12].copy_from_slice(&[b'R', b'3', b'G', b'A', 1, 0, 1, 0, 24, 0, 0, 0]);
    metadata[16..20].copy_from_slice(&(segment.index() as u32).to_le_bytes());
    metadata[20..24].copy_from_slice(&base.to_le_bytes());
    frame
}

fn packet(fields: [u32; 6]) -> [u8; 40] {
    let mut packet = [0; 40];
    let version = if fields[0] == 9 { 2 } else { 1 };
    packet[..12].copy_from_slice(&[b'R', b'3', b'G', b'H', version, 0, 1, 0, 40, 0, 0, 0]);
    for (index, field) in fields.into_iter().enumerate() {
        packet[16 + index * 4..20 + index * 4].copy_from_slice(&field.to_le_bytes());
    }
    packet
}

fn expected(before: &[u8], destination: &[u8; 32], mask: &[u8; 32], fields: [u32; 6]) -> Vec<u8> {
    let mut expected = before.to_vec();
    expected[TRANSFER_OFFSET..TRANSFER_OFFSET + 32].copy_from_slice(destination);
    expected[TRANSFER_OFFSET + 32..TRANSFER_OFFSET + 64].copy_from_slice(mask);
    expected[HELPER_OFFSET..HELPER_OFFSET + 40].copy_from_slice(&packet(fields));
    expected
}

fn invoke(
    engine: &mut EngineInstance,
    geometry: Geometry,
    scale: u32,
) -> Result<StoreCompletion, HostError> {
    engine.avx_execute(0x01001400 | geometry.id, OPERANDS, scale, 0, 0, FLAGS)
}

fn destination(value: [u8; 32], geometry: Geometry, fault: bool) -> [u8; 32] {
    let mut result = value;
    if !fault || geometry.data != geometry.index {
        result[geometry.data * geometry.lanes..].fill(0);
    }
    result
}

fn normalized_mask(geometry: Geometry, selected: u8, completed: u8) -> [u8; 32] {
    let mut result = [0; 32];
    for lane in 0..geometry.lanes {
        if selected & !completed & (1 << lane) != 0 {
            result[lane * geometry.data..(lane + 1) * geometry.data].fill(0xff);
        }
    }
    result
}

fn install(engine: &mut EngineInstance, frame: &[u8]) {
    engine.arena_mut().unwrap().copy_from_slice(frame);
}

#[test]
fn all_geometries_stage_sparse_raw_reads_without_publishing_any_architectural_state() {
    let mut engine = engine();
    let before_ram = ram(&engine);
    for &geometry in GEOMETRY {
        let selected = 0x55 & (((1_u16 << geometry.lanes) - 1) as u8);
        let values = vectors(geometry, selected);
        let before = frame(
            &engine,
            &values,
            SegmentRegister::Ds,
            0x5000,
            &SegmentContext::default(),
        );
        let mut output = destination(values[DESTINATION], geometry, false);
        for lane in 0..geometry.lanes {
            if selected & (1 << lane) != 0 {
                output[lane * geometry.data..(lane + 1) * geometry.data]
                    .copy_from_slice(&before_ram[8192 + lane * 8..8192 + lane * 8 + geometry.data]);
            }
        }
        install(&mut engine, &before);
        assert_eq!(
            invoke(&mut engine, geometry, 0),
            Ok(StoreCompletion::Complete),
            "geometry{geometry:?}"
        );
        assert_eq!(
            engine.arena(),
            expected(
                &before,
                &output,
                &[0; 32],
                [0, 0, 0, 0, selected.into(), u32::MAX]
            )
        );
        assert_eq!(ram(&engine), before_ram);
    }
}

#[test]
fn exact_element_endpoints_and_zero_masks_do_not_probe_a_nominal_vector_span() {
    let mut engine = engine();
    let before_ram = ram(&engine);
    for &geometry in GEOMETRY {
        let values = vectors(geometry, 1);
        let offset = 4096 - geometry.data;
        let before = frame(
            &engine,
            &values,
            SegmentRegister::Ds,
            0x5000 + offset as u32,
            &SegmentContext::default(),
        );
        let mut output = destination(values[DESTINATION], geometry, false);
        output[..geometry.data].copy_from_slice(&before_ram[8192 + offset..8192 + 4096]);
        install(&mut engine, &before);
        assert_eq!(
            invoke(&mut engine, geometry, 0),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(
            engine.arena(),
            expected(&before, &output, &[0; 32], [0, 0, 0, 0, 1, u32::MAX])
        );
        for segment in [SegmentRegister::Ds, SegmentRegister::Ss] {
            let mut context = SegmentContext::default();
            if segment == SegmentRegister::Ds {
                context.state.selectors[segment.index()] = 0;
                context.caches[segment.index()] = SegmentCache::UNUSABLE;
            } else {
                context.caches[segment.index()] = SegmentCache::from_bits(0xf2300000).unwrap();
            }
            let values = vectors(geometry, 0);
            let before = frame(&engine, &values, segment, u32::MAX, &context);
            let output = destination(values[DESTINATION], geometry, false);
            install(&mut engine, &before);
            assert_eq!(
                invoke(&mut engine, geometry, 3),
                Ok(StoreCompletion::Complete)
            );
            assert_eq!(
                engine.arena(),
                expected(&before, &output, &[0; 32], [0, 0, 0, 0, 0, u32::MAX])
            );
            assert_eq!(ram(&engine), before_ram);
        }
    }
}

#[test]
fn signed_index_and_low32_wrapping_apply_before_cached_span_authority() {
    let mut engine = engine();
    let before_ram = ram(&engine);
    for (geometry, base, index, scale, limit, expected_bytes) in [
        (
            GEOMETRY[0],
            0x2000,
            0xfffff000,
            0,
            0x1fff,
            [0xeb, 0xfe, 0, 0],
        ),
        (
            GEOMETRY[4],
            0x2000,
            0xfffffffffffff000,
            0,
            0x1fff,
            [0xeb, 0xfe, 0, 0],
        ),
        (GEOMETRY[4], 0, 0x100000400, 2, 0x1fff, [0xeb, 0xfe, 0, 0]),
        // R3GA receives the unchecked low32 base+disp result of FFFFFFF0+20.
        // Generated arithmetic itself is separately owned by the later actual proof.
        (GEOMETRY[0], 0x10, 0, 0, 0x1fff, [0x11, 0x22, 0x33, 0x44]),
    ] {
        let mut context = SegmentContext::default();
        context.caches[SegmentRegister::Ds.index()] =
            SegmentCache::from_bits(0xf2300000 | limit).unwrap();
        let mut values = vectors(geometry, 1);
        put(&mut values[INDEX], 0, geometry.index, index);
        let before = frame(&engine, &values, SegmentRegister::Ds, base, &context);
        let mut output = destination(values[DESTINATION], geometry, false);
        output[..4].copy_from_slice(&expected_bytes);
        install(&mut engine, &before);
        assert_eq!(
            invoke(&mut engine, geometry, scale),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(
            engine.arena(),
            expected(&before, &output, &[0; 32], [0, 0, 0, 0, 1, u32::MAX])
        );
        assert_eq!(ram(&engine), before_ram);
    }
}

#[test]
fn partial_faults_stage_a_completed_prefix_and_normalize_every_pending_mask() {
    let mut engine = engine();
    let before_ram = ram(&engine);
    for &geometry in GEOMETRY {
        let selected = ((1_u16 << geometry.lanes) - 1) as u8;
        let mut values = vectors(geometry, selected);
        put(&mut values[INDEX], geometry.index, geometry.index, 0x1010);
        let before = frame(
            &engine,
            &values,
            SegmentRegister::Ds,
            0x5ff0,
            &SegmentContext::default(),
        );
        let mut output = destination(values[DESTINATION], geometry, true);
        output[..geometry.data]
            .copy_from_slice(&before_ram[8192 + 0xff0..8192 + 0xff0 + geometry.data]);
        let mask = normalized_mask(geometry, selected, 1);
        install(&mut engine, &before);
        assert_eq!(
            invoke(&mut engine, geometry, 0),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(
            engine.arena(),
            expected(&before, &output, &mask, [9, 4, 0x7000, 1, 1, 1])
        );
        assert_eq!(ram(&engine), before_ram);
    }
}

#[test]
fn lower_page_fault_wins_over_a_higher_segment_fault_and_each_lane_has_exact_width() {
    let mut engine = engine();
    let before_ram = ram(&engine);
    for geometry in [GEOMETRY[0], GEOMETRY[2]] {
        for (segment, limit, base, later_index, fields) in [
            (
                SegmentRegister::Ds,
                0x7fff,
                0x6000,
                0x2000,
                [9, 4, 0x6000, 1, 0, 0],
            ),
            (SegmentRegister::Ds, 0x5fff, 0x6000, 0, [3, 0, 0, 0, 0, 0]),
            (SegmentRegister::Ss, 0x5fff, 0x6000, 0, [7, 0, 0, 0, 0, 0]),
            (
                SegmentRegister::Ds,
                0xfffff,
                u32::MAX,
                0,
                [3, 0, 0, 0, 0, 0],
            ),
        ] {
            let mut context = SegmentContext::default();
            context.caches[segment.index()] = if base == u32::MAX {
                SegmentContext::default().caches[segment.index()]
            } else {
                SegmentCache::from_bits(0xf2300000 | limit).unwrap()
            };
            let mut values = vectors(geometry, 3);
            put(
                &mut values[INDEX],
                geometry.index,
                geometry.index,
                later_index,
            );
            let before = frame(&engine, &values, segment, base, &context);
            let output = destination(values[DESTINATION], geometry, true);
            let mask = normalized_mask(geometry, 3, 0);
            install(&mut engine, &before);
            assert_eq!(
                invoke(&mut engine, geometry, 0),
                Ok(StoreCompletion::Complete)
            );
            assert_eq!(engine.arena(), expected(&before, &output, &mask, fields));
            assert_eq!(ram(&engine), before_ram);
        }
        let values = vectors(geometry, 1);
        let before = frame(
            &engine,
            &values,
            SegmentRegister::Ds,
            0x6000 - ((geometry.data as u32) / 2),
            &SegmentContext::default(),
        );
        let output = destination(values[DESTINATION], geometry, true);
        let mask = normalized_mask(geometry, 1, 0);
        install(&mut engine, &before);
        assert_eq!(
            invoke(&mut engine, geometry, 0),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(
            engine.arena(),
            expected(&before, &output, &mask, [9, 4, 0x6000, 1, 0, 0])
        );
        assert_eq!(ram(&engine), before_ram);
    }
}

#[test]
fn malformed_and_unknown_requests_keep_the_complete_arena_and_ram_unchanged() {
    let mut engine = engine();
    let before_ram = ram(&engine);
    let values = vectors(GEOMETRY[0], 1);
    let before = frame(
        &engine,
        &values,
        SegmentRegister::Ds,
        0x5000,
        &SegmentContext::default(),
    );
    for (descriptor, operands, scale) in [
        (0x01001501, OPERANDS, 0),
        (0x01001400, OPERANDS, 0),
        (0x01001409, OPERANDS, 0),
        (0x01011401, OPERANDS, 0),
        (0x02001401, OPERANDS, 0),
        (0x01001401, OPERANDS | (1 << 6), 0),
        (0x01001401, OPERANDS | (1 << 13), 0),
        (0x01001401, OPERANDS & !(1 << 12), 0),
        (0x01001401, (OPERANDS & !7) | 2, 0),
        (0x01001401, OPERANDS, 4),
    ] {
        install(&mut engine, &before);
        assert_eq!(
            engine.avx_execute(descriptor, operands, scale, 0, 0, FLAGS),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(ram(&engine), before_ram);
    }
    for (offset, value) in [(544, 0), (548, 2), (550, 2), (552, 23), (556, 1), (560, 6)] {
        let mut malformed = before.clone();
        malformed[TRANSFER_OFFSET + offset] = value;
        install(&mut engine, &malformed);
        assert_eq!(
            invoke(&mut engine, GEOMETRY[0], 0),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), malformed);
        assert_eq!(ram(&engine), before_ram);
    }
}
