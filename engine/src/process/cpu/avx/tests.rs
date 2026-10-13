use super::*;
use crate::{
    abi::{
        arena::{SEGMENTS_OFFSET, STATE_OFFSET},
        x86::{AVX_ADDRESS_MAGIC, AVX_ADDRESS_VERSION, SEGMENTS_SIZE, encode_segment_context},
    },
    cpu::x86::{Register32, simd::SimdState},
    process::{
        CallError,
        calls::{
            call::{PendingCall, PendingOwner},
            context::ExtendedCpuState,
        },
    },
    windows::{CallFrame32, CallingConvention32},
};

fn engine() -> (EngineInstance, u64) {
    let mut engine = EngineInstance::new(4, 91).unwrap();
    for address in [0x1000, 0x5000, 0x6000, 0x8000] {
        engine.map(address, 1, 7).unwrap();
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0xeb, 0xfe])
        .unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    let id = engine.compile_resident(1).unwrap().get();
    (engine, id)
}
fn descriptor(family: u32, kind: u32) -> u32 {
    0x0100_0000 | family << 8 | kind
}
fn address(engine: &mut EngineInstance, segment: SegmentRegister, offset: u32) {
    let arena = engine.arena_mut().unwrap();
    let start = TRANSFER_OFFSET + AVX_ADDRESS_TRANSFER;
    let output = &mut arena[start..start + AVX_ADDRESS_SIZE];
    output.fill(0);
    output[..4].copy_from_slice(&AVX_ADDRESS_MAGIC);
    output[4..6].copy_from_slice(&AVX_ADDRESS_VERSION.to_le_bytes());
    output[6..8].copy_from_slice(&1_u16.to_le_bytes());
    output[8..12].copy_from_slice(&(AVX_ADDRESS_SIZE as u32).to_le_bytes());
    output[16..20].copy_from_slice(&(segment.index() as u32).to_le_bytes());
    output[20..24].copy_from_slice(&offset.to_le_bytes());
}
fn seed(engine: &mut EngineInstance) -> VectorState {
    let vectors = VectorState {
        simd: SimdState {
            mxcsr: 0x1f80,
            xmm: std::array::from_fn(|r| std::array::from_fn(|b| (r * 29 + b * 13) as u8)),
        },
        upper: std::array::from_fn(|r| std::array::from_fn(|b| (r * 17 + b * 31 + 3) as u8)),
    };
    encode_simd(
        &vectors.simd,
        &mut engine.arena_mut().unwrap()[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE],
    )
    .unwrap();
    encode_ymm(
        &YmmUpperState {
            registers: vectors.upper,
        },
        &mut engine.arena_mut().unwrap()[YMM_OFFSET..YMM_OFFSET + YMM_SIZE],
    )
    .unwrap();
    vectors
}
fn packet(engine: &EngineInstance, offset: usize) -> u32 {
    u32::from_le_bytes(
        engine.arena()[HELPER_OFFSET + offset..HELPER_OFFSET + offset + 4]
            .try_into()
            .unwrap(),
    )
}
fn fault_neutral(engine: &EngineInstance, before: &[u8], length: u32, status: u32, detail: u32) {
    let mut expected = before.to_vec();
    encode_cpu_result(
        length,
        0,
        match status {
            3 => Err(CpuMemoryError::GeneralProtection(detail)),
            7 => Err(CpuMemoryError::StackSegment(detail)),
            _ => panic!("typed fault"),
        },
        &mut expected[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
    )
    .unwrap();
    assert_eq!(engine.arena(), expected);
}

#[test]
fn register_copy_ignores_stale_metadata_and_live_general_input_is_explicit() {
    let (mut engine, id) = engine();
    let vectors = seed(&mut engine);
    engine.arena_mut().unwrap()[TRANSFER_OFFSET + AVX_ADDRESS_TRANSFER
        ..TRANSFER_OFFSET + AVX_ADDRESS_TRANSFER + AVX_ADDRESS_SIZE]
        .fill(0xff);
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.avx_execute(descriptor(3, 2), 1 | 2 << 6, 0, 0, 0, 0x244fd7),
        Ok(StoreCompletion::Complete)
    );
    let result = decode_simd(&engine.arena()[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
    let upper = decode_ymm(&engine.arena()[YMM_OFFSET..YMM_OFFSET + YMM_SIZE]).unwrap();
    assert_eq!(result.xmm[1], vectors.simd.xmm[2]);
    assert_eq!(upper.registers[1], vectors.upper[2]);
    assert_eq!(
        &engine.arena()[TRANSFER_OFFSET + AVX_ADDRESS_TRANSFER
            ..TRANSFER_OFFSET + AVX_ADDRESS_TRANSFER + AVX_ADDRESS_SIZE],
        &before[TRANSFER_OFFSET + AVX_ADDRESS_TRANSFER
            ..TRANSFER_OFFSET + AVX_ADDRESS_TRANSFER + AVX_ADDRESS_SIZE]
    );
    assert_eq!(
        &engine.arena()[STATE_OFFSET..STATE_OFFSET + 56],
        &before[STATE_OFFSET..STATE_OFFSET + 56]
    );
    assert_eq!(
        engine.avx_execute_resident(91, id, descriptor(3, 33), 1 | 3 << 6, 0, 0x89ab_cdef, 0, 2),
        Ok(StoreCompletion::Complete)
    );
    let result = decode_simd(&engine.arena()[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
    assert_eq!(&result.xmm[1][..4], &0x89ab_cdef_u32.to_le_bytes());
    assert_eq!(&result.xmm[1][4..], &[0; 12]);
    assert_eq!(
        decode_ymm(&engine.arena()[YMM_OFFSET..YMM_OFFSET + YMM_SIZE])
            .unwrap()
            .registers[1],
        [0; 16]
    );
}

#[test]
fn strict_descriptor_metadata_flags_and_state_refusals_publish_nothing() {
    let (mut engine, id) = engine();
    seed(&mut engine);
    for args in [
        (descriptor(3, 2) | 0x10000, 0, 0, 0, 0, 2),
        (descriptor(3, 2), 1 << 15, 0, 0, 0, 2),
        (descriptor(3, 2), 0, 256, 0, 0, 2),
        (descriptor(3, 2), 0, 0, 1, 0, 2),
        (descriptor(3, 2), 0, 0, 0, 0, 0),
    ] {
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.avx_execute(args.0, args.1, args.2, args.3, args.4, args.5),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), before);
    }
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.avx_execute_resident(92, id, descriptor(3, 2), 0, 0, 0, 0, 2),
        Err(HostError::InvalidArtifact)
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(
        engine.avx_execute(descriptor(3, 2), 1 << 12, 0, 0, 0, 2),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.arena(), before);
    address(&mut engine, SegmentRegister::Ds, 0x5000);
    engine.arena_mut().unwrap()[YMM_OFFSET + 12] = 1;
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.avx_execute(descriptor(3, 2), 1 << 12, 0, 0, 0, 2),
        Err(HostError::Infrastructure)
    );
    assert_eq!(engine.arena(), before);
}

#[test]
fn full_span_segment_alignment_and_late_page_faults_precede_publication() {
    let (mut engine, _) = engine();
    seed(&mut engine);
    for segment in [SegmentRegister::Ds, SegmentRegister::Ss] {
        address(&mut engine, segment, u32::MAX - 15);
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.avx_execute(descriptor(3, 2), 1 << 12, 0, 0, 0, 2),
            Ok(StoreCompletion::Complete)
        );
        fault_neutral(
            &engine,
            &before,
            32,
            if segment == SegmentRegister::Ss { 7 } else { 3 },
            0,
        );
    }
    address(&mut engine, SegmentRegister::Ds, 0x5001);
    let before = engine.arena().to_vec();
    engine
        .avx_execute(descriptor(3, 24), 1 << 12, 0, 0, 0, 2)
        .unwrap();
    fault_neutral(&engine, &before, 32, 3, 0);
    engine.unmap(0x6000, 1).unwrap();
    address(&mut engine, SegmentRegister::Ds, 0x5ff0);
    let before = engine.arena().to_vec();
    engine
        .avx_execute(descriptor(3, 2), 1 << 12, 0, 0, 0, 2)
        .unwrap();
    assert_eq!(packet(&engine, 16), 1);
    assert_eq!(packet(&engine, 28), 0x6000);
    assert_eq!(packet(&engine, 32), 1);
    assert_eq!(packet(&engine, 36), 32);
    assert_eq!(
        &engine.arena()[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE],
        &before[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]
    );
    assert_eq!(&engine.arena()[YMM_OFFSET..], &before[YMM_OFFSET..]);
    let mut ram = [0; 16];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x5ff0), &mut ram)
        .unwrap();
    address(&mut engine, SegmentRegister::Ds, 0x5ff0);
    let before = engine.arena().to_vec();
    engine
        .avx_execute(descriptor(3, 8), 1 << 13 | 2 << 6, 0, 0, 0, 2)
        .unwrap();
    let mut after = [0; 16];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x5ff0), &mut after)
        .unwrap();
    assert_eq!(after, ram);
    assert_eq!(packet(&engine, 32), 2);
    assert_eq!(&engine.arena()[SIMD_OFFSET..], &before[SIMD_OFFSET..]);
}

#[test]
fn masked_selected_accesses_skip_disabled_segments_and_commit_once() {
    let (mut engine, _) = engine();
    seed(&mut engine);
    let mut simd = decode_simd(&engine.arena()[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
    simd.xmm[3] = [0; 16];
    encode_simd(
        &simd,
        &mut engine.arena_mut().unwrap()[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE],
    )
    .unwrap();
    let mut upper = decode_ymm(&engine.arena()[YMM_OFFSET..YMM_OFFSET + YMM_SIZE]).unwrap();
    upper.registers[3] = [0; 16];
    encode_ymm(
        &upper,
        &mut engine.arena_mut().unwrap()[YMM_OFFSET..YMM_OFFSET + YMM_SIZE],
    )
    .unwrap();
    address(&mut engine, SegmentRegister::Ds, u32::MAX);
    engine.arena_mut().unwrap()[SEGMENTS_OFFSET + 12] = 1;
    let version = engine.memory.as_ref().unwrap().code_validation_version();
    engine
        .avx_execute(descriptor(13, 3), 1 | 3 << 9 | 1 << 12, 0, 0, 0, 2)
        .unwrap();
    assert_eq!(
        decode_simd(&engine.arena()[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE])
            .unwrap()
            .xmm[1],
        [0; 16]
    );
    assert_eq!(
        decode_ymm(&engine.arena()[YMM_OFFSET..YMM_OFFSET + YMM_SIZE])
            .unwrap()
            .registers[1],
        [0; 16]
    );
    assert_eq!(
        engine.memory.as_ref().unwrap().code_validation_version(),
        version
    );
    encode_segment_context(
        &SegmentContext::default(),
        &mut engine.arena_mut().unwrap()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE],
    )
    .unwrap();
    let mut simd = decode_simd(&engine.arena()[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
    simd.xmm[3][3] = 0x80;
    encode_simd(
        &simd,
        &mut engine.arena_mut().unwrap()[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE],
    )
    .unwrap();
    address(&mut engine, SegmentRegister::Ds, 0x5ffc);
    engine.unmap(0x6000, 1).unwrap();
    let version = engine.memory.as_ref().unwrap().code_validation_version();
    engine
        .avx_execute(descriptor(13, 7), 2 << 6 | 3 << 9 | 1 << 13, 0, 0, 0, 2)
        .unwrap();
    assert_eq!(packet(&engine, 16), 0);
    assert_eq!(
        engine.memory.as_ref().unwrap().code_validation_version(),
        version + 1
    );
    simd.xmm[3][7] = 0x80;
    encode_simd(
        &simd,
        &mut engine.arena_mut().unwrap()[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE],
    )
    .unwrap();
    let mut before_ram = [0; 4];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x5ffc), &mut before_ram)
        .unwrap();
    let before = engine.arena().to_vec();
    let version = engine.memory.as_ref().unwrap().code_validation_version();
    engine
        .avx_execute(descriptor(13, 7), 2 << 6 | 3 << 9 | 1 << 13, 0, 0, 0, 2)
        .unwrap();
    assert_eq!(packet(&engine, 16), 1);
    assert_eq!(packet(&engine, 28), 0x6000);
    let mut after_ram = [0; 4];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x5ffc), &mut after_ram)
        .unwrap();
    assert_eq!(after_ram, before_ram);
    assert_eq!(
        engine.memory.as_ref().unwrap().code_validation_version(),
        version
    );
    assert_eq!(&engine.arena()[SIMD_OFFSET..], &before[SIMD_OFFSET..]);
}

#[test]
fn unmasked_numeric_fault_publishes_only_sticky_mxcsr_and_exact_packet() {
    let (mut engine, _) = engine();
    seed(&mut engine);
    let mut simd = decode_simd(&engine.arena()[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]).unwrap();
    simd.mxcsr = 0x1f00;
    simd.xmm[2][..4].copy_from_slice(&0x7fc0_0001_u32.to_le_bytes());
    encode_simd(
        &simd,
        &mut engine.arena_mut().unwrap()[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE],
    )
    .unwrap();
    let before = engine.arena().to_vec();
    engine
        .avx_execute(descriptor(11, 3), 3 | 2 << 6, 0, 0, 0, 2)
        .unwrap();
    let mut expected = before;
    expected[SIMD_OFFSET + 16..SIMD_OFFSET + 20].copy_from_slice(&0x1f01_u32.to_le_bytes());
    encode_cpu_result(
        0,
        0,
        Err(CpuMemoryError::SimdFloatingPoint(0x1f01)),
        &mut expected[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
    )
    .unwrap();
    assert_eq!(engine.arena(), expected);
}

#[test]
fn pending_stale_closed_and_smc_authority_keep_existing_lifecycle() {
    let (mut engine, id) = engine();
    seed(&mut engine);
    let mut state = State32::default();
    state.registers[Register32::Esp.index()] = 0x8000;
    let frame = CallFrame32::capture(
        engine.memory().unwrap(),
        state,
        CallingConvention32::Cdecl,
        0,
    )
    .unwrap();
    engine.pending_call = Some(PendingCall {
        token: 1,
        owner: PendingOwner::Resident(id),
        frame,
        state: engine.arena()[..56].try_into().unwrap(),
        exit: engine.arena()[56..96].try_into().unwrap(),
        extended: ExtendedCpuState::capture(engine.arena()).unwrap(),
    });
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.avx_execute(0, 0, 0, 0, 0, 2),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    engine.pending_call = None;
    address(&mut engine, SegmentRegister::Ds, 0x1000);
    assert_eq!(
        engine.avx_execute_resident(91, id, descriptor(3, 8), 2 << 6 | 1 << 13, 0, 0, 0, 2),
        Ok(StoreCompletion::CodeInvalidated)
    );
    assert_eq!(packet(&engine, 16), 0);
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.avx_execute(descriptor(3, 2), 0, 0, 0, 0, 2),
        Err(HostError::CodeInvalidated)
    );
    assert_eq!(engine.arena(), before);
    engine.close();
    let before = engine.arena().to_vec();
    assert_eq!(engine.avx_execute(0, 0, 0, 0, 0, 2), Err(HostError::Closed));
    assert_eq!(engine.arena(), before);
}

#[test]
fn every_closed_semantic_family_has_a_production_dispatch_path() {
    let (mut engine, _) = engine();
    encode_simd(
        &SimdState::default(),
        &mut engine.arena_mut().unwrap()[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE],
    )
    .unwrap();
    encode_ymm(
        &YmmUpperState::default(),
        &mut engine.arena_mut().unwrap()[YMM_OFFSET..YMM_OFFSET + YMM_SIZE],
    )
    .unwrap();
    for family in 1..=17 {
        address(&mut engine, SegmentRegister::Ds, 0x5000);
        let operands = match family {
            13 => 1 << 13,
            14 => 1 << 12,
            _ => 0,
        };
        let before = engine.arena()[..100].to_vec();
        assert_eq!(
            engine.avx_execute(descriptor(family, 1), operands, 0, 0, 0, 2),
            Ok(StoreCompletion::Complete),
            "family {family}"
        );
        assert_eq!(packet(&engine, 16), 0, "family {family}");
        assert_eq!(&engine.arena()[..100], &before);
    }
}

#[test]
fn exhausted_store_version_preserves_all_staged_outputs_and_ram() {
    let (mut engine, _) = engine();
    seed(&mut engine);
    address(&mut engine, SegmentRegister::Ds, 0x5000);
    let mut ram = [0; 32];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x5000), &mut ram)
        .unwrap();
    engine.memory.as_mut().unwrap().exhaust_versions_for_test();
    let before = engine.arena().to_vec();
    engine
        .avx_execute(descriptor(3, 8), 2 << 6 | 1 << 13, 0, 0, 0, 2)
        .unwrap();
    let mut expected = before;
    encode_cpu_result(
        32,
        0,
        Err(CpuMemoryError::Memory(
            crate::memory::MemoryError::VersionExhausted,
        )),
        &mut expected[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
    )
    .unwrap();
    assert_eq!(engine.arena(), expected);
    let mut after = [0; 32];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x5000), &mut after)
        .unwrap();
    assert_eq!(after, ram);
}
