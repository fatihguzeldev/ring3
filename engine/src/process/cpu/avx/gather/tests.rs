use super::*;
use crate::{
    abi::{
        arena::{SEGMENTS_OFFSET, SIMD_OFFSET, YMM_OFFSET},
        x86::{
            SEGMENTS_SIZE, SIMD_SIZE, YMM_SIZE, encode_segment_context, encode_simd, encode_ymm,
        },
    },
    cpu::x86::{
        SegmentCache, SegmentContext,
        avx::{GatherGeometry, GatherScale, VectorRegister},
        simd::ymm::YmmUpperState,
    },
    memory::GuestAddress,
};

fn instruction() -> GatherInstruction {
    GatherInstruction::new(
        GatherGeometry::DwordDword128,
        VectorRegister::new(1).unwrap(),
        VectorRegister::new(2).unwrap(),
        VectorRegister::new(3).unwrap(),
        GatherScale::One,
    )
    .unwrap()
}

fn engine(second: u32, selected: bool) -> (EngineInstance, VectorState) {
    let mut engine = EngineInstance::new(1, 91).unwrap();
    engine.map(0x5000, 1, 3).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x5000),
            &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88],
        )
        .unwrap();
    let mut vectors = VectorState::default();
    vectors.simd.xmm[1] = [0xa5; 16];
    vectors.upper[1] = [0x6b; 16];
    vectors.simd.xmm[2][..4].copy_from_slice(&0x5000_u32.to_le_bytes());
    vectors.simd.xmm[2][4..8].copy_from_slice(&second.to_le_bytes());
    vectors.simd.xmm[3] = [0x13; 16];
    vectors.upper[3] = [0x91; 16];
    if selected {
        vectors.simd.xmm[3][3] = 0x80;
        vectors.simd.xmm[3][7] = 0x81;
    }
    let arena = engine.arena_mut().unwrap();
    encode_simd(
        &vectors.simd,
        &mut arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE],
    )
    .unwrap();
    encode_ymm(
        &YmmUpperState {
            registers: vectors.upper,
        },
        &mut arena[YMM_OFFSET..YMM_OFFSET + YMM_SIZE],
    )
    .unwrap();
    let start = TRANSFER_OFFSET + 544;
    arena[start..start + 24].fill(0);
    arena[start..start + 12].copy_from_slice(&[b'R', b'3', b'G', b'A', 1, 0, 1, 0, 24, 0, 0, 0]);
    arena[start + 16..start + 20].copy_from_slice(&3_u32.to_le_bytes());
    (engine, vectors)
}

fn expected(
    mut arena: Vec<u8>,
    destination: [u8; 32],
    mask: [u8; 32],
    fields: [u32; 6],
) -> Vec<u8> {
    arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 32].copy_from_slice(&destination);
    arena[TRANSFER_OFFSET + 32..TRANSFER_OFFSET + 64].copy_from_slice(&mask);
    let packet = &mut arena[HELPER_OFFSET..HELPER_OFFSET + 40];
    packet.fill(0);
    packet[..12].copy_from_slice(&[b'R', b'3', b'G', b'H', 1, 0, 1, 0, 40, 0, 0, 0]);
    for (index, value) in fields.into_iter().enumerate() {
        packet[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    arena
}

fn ram(engine: &EngineInstance) -> [u8; 4096] {
    let mut bytes = [0; 4096];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x5000), &mut bytes)
        .unwrap();
    bytes
}

#[test]
fn success_and_later_page_fault_stage_two_images_without_architectural_publication() {
    for (second, fault) in [(0x5004, false), (0x6000, true)] {
        let (mut engine, vectors) = engine(second, true);
        let before = engine.arena().to_vec();
        let before_ram = ram(&engine);
        let mut destination = [0xa5; 32];
        destination[16..].fill(if fault { 0x6b } else { 0 });
        destination[..4].copy_from_slice(&[0x11, 0x22, 0x33, 0x44]);
        let mut mask = [0; 32];
        let fields = if fault {
            mask[4..8].fill(0xff);
            [1, 1, 0x6000, 1, 1, 1]
        } else {
            destination[4..8].copy_from_slice(&[0x55, 0x66, 0x77, 0x88]);
            [0, 0, 0, 0, 3, u32::MAX]
        };
        let expected = expected(before, destination, mask, fields);
        assert_eq!(
            engine.avx_gather_apply(instruction(), &vectors),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(engine.arena(), expected);
        assert_eq!(ram(&engine), before_ram);
        assert_eq!(engine.arena(), expected);
    }
}

#[test]
fn zero_mask_suppresses_cached_access_and_nonzero_mask_returns_precise_segment_status() {
    for (segment, status) in [(SegmentRegister::Ds, 3), (SegmentRegister::Ss, 7)] {
        for selected in [false, true] {
            let (mut engine, vectors) = engine(0x6000, selected);
            let mut context = SegmentContext::default();
            if segment == SegmentRegister::Ss {
                context.caches[segment.index()] = SegmentCache::from_bits(0xf2300000).unwrap();
            } else {
                context.state.selectors[segment.index()] = 0;
                context.caches[segment.index()] = SegmentCache::UNUSABLE;
            }
            let arena = engine.arena_mut().unwrap();
            encode_segment_context(
                &context,
                &mut arena[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE],
            )
            .unwrap();
            arena[TRANSFER_OFFSET + 544 + 16..TRANSFER_OFFSET + 544 + 20]
                .copy_from_slice(&(segment.index() as u32).to_le_bytes());
            let before = arena.to_vec();
            let before_ram = ram(&engine);
            let mut destination = [0xa5; 32];
            destination[16..].fill(if selected { 0x6b } else { 0 });
            let mut mask = [0; 32];
            let fields = if selected {
                mask[..8].fill(0xff);
                [status, 0, 0, 0, 0, 0]
            } else {
                [0, 0, 0, 0, 0, u32::MAX]
            };
            let expected = expected(before, destination, mask, fields);
            assert_eq!(
                engine.avx_gather_apply(instruction(), &vectors),
                Ok(StoreCompletion::Complete)
            );
            assert_eq!(engine.arena(), expected);
            assert_eq!(ram(&engine), before_ram);
        }
    }
}

#[test]
fn malformed_request_or_nonzero_invalid_context_does_not_publish_even_scratch() {
    for bad_context in [false, true] {
        let (mut engine, vectors) = engine(0x5004, true);
        let arena = engine.arena_mut().unwrap();
        if bad_context {
            arena[SEGMENTS_OFFSET] = 0;
        } else {
            arena[TRANSFER_OFFSET + 544 + 12] = 1;
        }
        let before = arena.to_vec();
        let before_ram = ram(&engine);
        let error = if bad_context {
            HostError::Infrastructure
        } else {
            HostError::InvalidRequest
        };
        assert_eq!(engine.avx_gather_apply(instruction(), &vectors), Err(error));
        assert_eq!(engine.arena(), before);
        assert_eq!(ram(&engine), before_ram);
        assert_eq!(engine.arena(), before);
    }
}
