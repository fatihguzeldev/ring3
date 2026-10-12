use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::x86::{
        decode::decode_one,
        ir::Operation,
        simd::{self, SimdError, SimdOperation, SimdState},
    },
    memory::GuestAddress,
    process::EngineInstance,
};

fn decode(bytes: &[u8]) -> SimdOperation {
    let mut engine = EngineInstance::new(1, 0x5353_4533_0000_0001).unwrap();
    engine.map(0x2000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(0x2000, bytes.len() as u32).unwrap();
    engine.protect(0x2000, 1, 5).unwrap();
    let instruction = decode_one(engine.memory().unwrap(), GuestAddress(0x2000)).unwrap();
    assert_eq!(usize::from(instruction.length()), bytes.len());
    let Operation::Simd(op) = instruction.operation() else {
        panic!("typed SIMD expected")
    };
    let (descriptor, operands, immediate) = op.descriptor();
    let restored = simd::decode_descriptor(descriptor, operands, immediate).unwrap();
    assert_eq!(restored.kind, op.kind);
    assert_eq!(restored.lane_bytes, op.lane_bytes);
    assert_eq!(restored.memory_bytes, op.memory_bytes);
    *op
}

fn dwords(values: [u32; 4]) -> [u8; 16] {
    let mut bytes = [0; 16];
    for (index, value) in values.iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn run(op: SimdOperation, state: &mut SimdState) -> Result<simd::SimdResult, SimdError> {
    simd::execute(op, state, &mut [[0; 8]; 8], &mut [0; 16], 0, 0, 0xcd7)
}

#[test]
fn sse3_decodes_legacy_load_duplicate_and_horizontal_forms() {
    for bytes in [
        &[0xf2, 0x0f, 0xf0, 0x00][..],
        &[0xf2, 0x0f, 0x12, 0xc1][..],
        &[0xf3, 0x0f, 0x12, 0xc1][..],
        &[0xf3, 0x0f, 0x16, 0xc1][..],
        &[0xf2, 0x0f, 0xd0, 0xc1][..],
        &[0x66, 0x0f, 0xd0, 0xc1][..],
        &[0xf2, 0x0f, 0x7c, 0xc1][..],
        &[0x66, 0x0f, 0x7c, 0xc1][..],
        &[0xf2, 0x0f, 0x7d, 0xc1][..],
        &[0x66, 0x0f, 0x7d, 0xc1][..],
    ] {
        decode(bytes);
    }
    assert_eq!(decode(&[0xf2, 0x0f, 0x12, 0x00]).memory_bytes, 8);
    assert!(!decode(&[0xf2, 0x0f, 0xf0, 0x00]).aligned);
}

#[test]
fn sse3_horizontal_rounding_and_unmasked_exception_are_transactional() {
    let mut state = SimdState {
        mxcsr: 0x5f80,
        ..SimdState::default()
    };
    state.xmm[0] = dwords([0x3f800000, 0x33800000, 0x40000000, 0x40400000]);
    state.xmm[1] = dwords([0x40800000, 0x40a00000, 0x40c00000, 0x40e00000]);
    let result = run(decode(&[0xf2, 0x0f, 0x7c, 0xc1]), &mut state).unwrap();
    assert_eq!(
        state.xmm[0],
        dwords([0x3f800001, 0x40a00000, 0x41100000, 0x41500000])
    );
    assert_eq!(state.mxcsr, 0x5fa0);
    assert_eq!(result.flags, 0xcd7);

    state.mxcsr = 0x1f00;
    state.xmm[0] = dwords([0x7f800000, 0x3f800000, 0x40000000, 0x40400000]);
    state.xmm[1] = dwords([0x7f800000, 0x3f800000, 0x40000000, 0x40400000]);
    let before = state.xmm;
    assert_eq!(
        run(decode(&[0xf2, 0x0f, 0xd0, 0xc1]), &mut state),
        Err(SimdError::FloatingPoint(0x1f01))
    );
    assert_eq!(state.xmm, before);
    assert_eq!(state.mxcsr, 0x1f01);
}

#[test]
fn sse3_duplicate_preserves_signaling_nan_bits_without_exceptions() {
    let mut state = SimdState::default();
    state.xmm[1] = dwords([0x7f800001, 0x12345678, 0x7f800002, 0x87654321]);
    run(decode(&[0xf3, 0x0f, 0x12, 0xc1]), &mut state).unwrap();
    assert_eq!(
        state.xmm[0],
        dwords([0x7f800001, 0x7f800001, 0x7f800002, 0x7f800002])
    );
    assert_eq!(state.mxcsr, 0x1f80);
    run(decode(&[0xf2, 0x0f, 0x12, 0xc1]), &mut state).unwrap();
    assert_eq!(
        state.xmm[0],
        dwords([0x7f800001, 0x12345678, 0x7f800001, 0x12345678])
    );
    assert_eq!(state.mxcsr, 0x1f80);
}
