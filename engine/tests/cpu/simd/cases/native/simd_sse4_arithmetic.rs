use ring3_engine::{
    cpu::x86::{
        decode::decode_one,
        ir::Operation,
        simd::{self, SimdError, SimdOperation, SimdState},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn decode(bytes: &[u8]) -> SimdOperation {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    let instruction = decode_one(&memory, GuestAddress(0x1000)).unwrap();
    assert_eq!(usize::from(instruction.length()), bytes.len());
    let Operation::Simd(op) = instruction.operation() else {
        panic!("typed SIMD expected")
    };
    let (descriptor, operands, immediate) = op.descriptor();
    let restored = simd::decode_descriptor(descriptor, operands, immediate).unwrap();
    assert_eq!(restored.kind, op.kind);
    assert_eq!(restored.vector_bytes, 16);
    assert_eq!(restored.lane_bytes, op.lane_bytes);
    *op
}

fn run(op: SimdOperation, state: &mut SimdState) -> Result<simd::SimdResult, SimdError> {
    simd::execute(
        op,
        state,
        &mut [[0; 8]; 8],
        &mut [0; 16],
        0,
        0x1020_3040,
        0xcd7,
    )
}
fn dwords(values: [u32; 4]) -> [u8; 16] {
    let mut bytes = [0; 16];
    for (index, value) in values.into_iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}
fn qwords(values: [u64; 2]) -> [u8; 16] {
    let mut bytes = [0; 16];
    for (index, value) in values.into_iter().enumerate() {
        bytes[index * 8..index * 8 + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}
fn words(values: [u16; 8]) -> [u8; 16] {
    let mut bytes = [0; 16];
    for (index, value) in values.into_iter().enumerate() {
        bytes[index * 2..index * 2 + 2].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn sse4_decodes_round_dot_sad_minimum_and_test_with_exact_memory_widths() {
    for (opcode, width) in [
        (0x08, 16),
        (0x09, 16),
        (0x0a, 4),
        (0x0b, 8),
        (0x40, 16),
        (0x41, 16),
        (0x42, 16),
    ] {
        decode(&[0x66, 0x0f, 0x3a, opcode, 0xc1, 0xff]);
        let memory = decode(&[0x66, 0x0f, 0x3a, opcode, 0x00, 0xff]);
        assert_eq!(memory.memory_bytes, width);
        assert_eq!(memory.aligned, width == 16);
    }
    for opcode in [0x17, 0x41] {
        decode(&[0x66, 0x0f, 0x38, opcode, 0xc1]);
        let memory = decode(&[0x66, 0x0f, 0x38, opcode, 0x00]);
        assert_eq!(memory.memory_bytes, 16);
        assert!(memory.aligned);
    }
}

#[test]
fn sse4_round_uses_immediate_or_mxcsr_mode_and_preserves_scalar_upper_bits() {
    let source = dwords([0x3fc0_0000, 0xbfc0_0000, 0x4020_0000, 0xbf00_0000]);
    for (immediate, mxcsr, expected) in [
        (
            0,
            0x1f80,
            [0x4000_0000, 0xc000_0000, 0x4000_0000, 0x8000_0000],
        ),
        (
            1,
            0x1f80,
            [0x3f80_0000, 0xc000_0000, 0x4000_0000, 0xbf80_0000],
        ),
        (
            2,
            0x1f80,
            [0x4000_0000, 0xbf80_0000, 0x4040_0000, 0x8000_0000],
        ),
        (
            3,
            0x1f80,
            [0x3f80_0000, 0xbf80_0000, 0x4000_0000, 0x8000_0000],
        ),
        (
            4,
            0x5f80,
            [0x4000_0000, 0xbf80_0000, 0x4040_0000, 0x8000_0000],
        ),
    ] {
        let mut state = SimdState::default();
        state.mxcsr = mxcsr;
        state.xmm[1] = source;
        assert_eq!(
            run(
                decode(&[0x66, 0x0f, 0x3a, 0x08, 0xc1, immediate]),
                &mut state
            )
            .unwrap()
            .flags,
            0xcd7
        );
        assert_eq!(state.xmm[0], dwords(expected));
        assert_eq!(state.mxcsr, mxcsr | 0x20);
    }
    let mut state = SimdState::default();
    state.xmm[0] = dwords([1, 2, 3, 4]);
    state.xmm[1] = dwords([0x3fc0_0000, 0x7f80_0001, 0, 0]);
    run(decode(&[0x66, 0x0f, 0x3a, 0x0a, 0xc1, 0]), &mut state).unwrap();
    assert_eq!(state.xmm[0], dwords([0x4000_0000, 2, 3, 4]));
    assert_eq!(state.mxcsr, 0x1fa0);
    state.mxcsr = 0x1f80;
    state.xmm[0] = qwords([1, 0x1234_5678_9abc_def0]);
    state.xmm[1] = qwords([0xbff8_0000_0000_0000, 0x7ff0_0000_0000_0001]);
    run(decode(&[0x66, 0x0f, 0x3a, 0x0b, 0xc1, 1]), &mut state).unwrap();
    assert_eq!(
        state.xmm[0],
        qwords([0xc000_0000_0000_0000, 0x1234_5678_9abc_def0])
    );
    assert_eq!(state.mxcsr, 0x1fa0);
}

#[test]
fn sse4_round_suppresses_precision_but_preserves_invalid_fault_atomicity() {
    let mut state = SimdState::default();
    state.mxcsr = 0x0f80;
    state.xmm[0] = [0xa5; 16];
    state.xmm[1] = dwords([0x3fc0_0000; 4]);
    let destination = state.xmm[0];
    assert_eq!(
        run(decode(&[0x66, 0x0f, 0x3a, 0x08, 0xc1, 0]), &mut state),
        Err(SimdError::FloatingPoint(0x0fa0))
    );
    assert_eq!(state.xmm[0], destination);
    state.mxcsr = 0x0f80;
    run(decode(&[0x66, 0x0f, 0x3a, 0x08, 0xc1, 8]), &mut state).unwrap();
    assert_eq!(state.xmm[0], dwords([0x4000_0000; 4]));
    assert_eq!(state.mxcsr, 0x0f80);
    state.mxcsr = 0x1f80;
    state.xmm[1] = dwords([0x7f80_1234, 0x7fc0_5678, 0x7f80_0000, 0x8000_0000]);
    run(decode(&[0x66, 0x0f, 0x3a, 0x08, 0xc1, 8]), &mut state).unwrap();
    assert_eq!(
        state.xmm[0],
        dwords([0x7fc0_1234, 0x7fc0_5678, 0x7f80_0000, 0x8000_0000])
    );
    assert_eq!(state.mxcsr, 0x1f81);
    state.mxcsr = 0x1f00;
    let destination = state.xmm[0];
    assert_eq!(
        run(decode(&[0x66, 0x0f, 0x3a, 0x08, 0xc1, 8]), &mut state),
        Err(SimdError::FloatingPoint(0x1f01))
    );
    assert_eq!(state.xmm[0], destination);
    state.mxcsr = 0x1e80;
    state.xmm[1] = dwords([1, 0x8000_0001, 0, 0x8000_0000]);
    run(decode(&[0x66, 0x0f, 0x3a, 0x08, 0xc1, 2]), &mut state).unwrap();
    assert_eq!(
        state.xmm[0],
        dwords([0x3f80_0000, 0x8000_0000, 0, 0x8000_0000])
    );
    assert_eq!(state.mxcsr, 0x1ea0);
    state.mxcsr = 0x1ec0;
    run(decode(&[0x66, 0x0f, 0x3a, 0x08, 0xc1, 2]), &mut state).unwrap();
    assert_eq!(state.xmm[0], dwords([0, 0x8000_0000, 0, 0x8000_0000]));
    assert_eq!(state.mxcsr, 0x1ec0);
}

#[test]
fn sse4_dot_products_use_pairwise_rounding_and_suppress_masked_inputs() {
    let mut state = SimdState::default();
    state.xmm[0] = dwords([0x4b80_0000, 0x3f80_0000, 0x3f80_0000, 0xcb80_0000]);
    state.xmm[1] = dwords([0x3f80_0000; 4]);
    run(decode(&[0x66, 0x0f, 0x3a, 0x40, 0xc1, 0xff]), &mut state).unwrap();
    assert_eq!(state.xmm[0], dwords([0x3f80_0000; 4]));
    assert_eq!(state.mxcsr, 0x1fa0);
    state.mxcsr = 0x1f00;
    state.xmm[0] = dwords([0x3f80_0000, 0x7f80_0001, 0x7f80_0000, 0x4080_0000]);
    state.xmm[1] = dwords([0x40a0_0000, 0, 0, 0x4100_0000]);
    run(decode(&[0x66, 0x0f, 0x3a, 0x40, 0xc1, 0x11]), &mut state).unwrap();
    assert_eq!(state.xmm[0], dwords([0x40a0_0000, 0, 0, 0]));
    assert_eq!(state.mxcsr, 0x1f00);
    state.mxcsr = 0x1f80;
    state.xmm[0] = qwords([0x3ff0_0000_0000_0000, 0x4000_0000_0000_0000]);
    state.xmm[1] = qwords([0x4008_0000_0000_0000, 0x4010_0000_0000_0000]);
    run(decode(&[0x66, 0x0f, 0x3a, 0x41, 0xc1, 0x33]), &mut state).unwrap();
    assert_eq!(state.xmm[0], qwords([0x4026_0000_0000_0000; 2]));
    assert_eq!(state.mxcsr, 0x1f80);
    state.mxcsr = 0x1f00;
    state.xmm[0] = qwords([0x3ff0_0000_0000_0000, 0x7ff0_0000_0000_0001]);
    state.xmm[1] = qwords([0x4008_0000_0000_0000, 0]);
    run(decode(&[0x66, 0x0f, 0x3a, 0x41, 0xc1, 0xd6]), &mut state).unwrap();
    assert_eq!(state.xmm[0], qwords([0, 0x4008_0000_0000_0000]));
    assert_eq!(state.mxcsr, 0x1f00);
}

#[test]
fn sse4_dot_product_delivers_first_unmasked_stage_before_later_invalid() {
    let mut state = SimdState::default();
    state.mxcsr = 0x0f00;
    state.xmm[0] = dwords([0x4b80_0000, 0x3f80_0000, 0x7f80_0000, 0xff80_0000]);
    state.xmm[1] = dwords([0x3f80_0000; 4]);
    let destination = state.xmm[0];
    assert_eq!(
        run(decode(&[0x66, 0x0f, 0x3a, 0x40, 0xc1, 0xff]), &mut state),
        Err(SimdError::FloatingPoint(0x0f20))
    );
    assert_eq!(state.xmm[0], destination);
}

#[test]
fn sse4_dot_product_preserves_completed_stage_sticky_flags() {
    let mut state = SimdState::default();
    state.mxcsr = 0x1f00;
    state.xmm[0] = dwords([0x4b80_0000, 0x3f80_0000, 0x7f80_0000, 0xff80_0000]);
    state.xmm[1] = dwords([0x3f80_0000; 4]);
    let destination = state.xmm[0];
    assert_eq!(
        run(decode(&[0x66, 0x0f, 0x3a, 0x40, 0xc1, 0xff]), &mut state),
        Err(SimdError::FloatingPoint(0x1f21))
    );
    assert_eq!(state.xmm[0], destination);
}

#[test]
fn sse4_double_dot_product_delivers_multiply_stage_before_invalid_sum() {
    let mut state = SimdState::default();
    state.mxcsr = 0x0f00;
    state.xmm[0] = qwords([0x7fef_ffff_ffff_ffff, 0xfff0_0000_0000_0000]);
    state.xmm[1] = qwords([0x4000_0000_0000_0000, 0x3ff0_0000_0000_0000]);
    let destination = state.xmm[0];
    assert_eq!(
        run(decode(&[0x66, 0x0f, 0x3a, 0x41, 0xc1, 0x33]), &mut state),
        Err(SimdError::FloatingPoint(0x0f28))
    );
    assert_eq!(state.xmm[0], destination);
}

#[test]
fn sse4_dot_product_aggregates_masked_and_unmasked_precomputation_flags() {
    for (input, expected) in [
        ([0x7f80_1234, 1, 2, 3], 0x1f03),
        ([0x7f80_1234, 0, 0, 0], 0x1f01),
    ] {
        let mut state = SimdState::default();
        state.mxcsr = 0x1f00;
        state.xmm[0] = dwords(input);
        state.xmm[1] = dwords([0x3f80_0000, 0, 0, 0]);
        let destination = state.xmm[0];
        assert_eq!(
            run(decode(&[0x66, 0x0f, 0x3a, 0x40, 0xc1, 0xff]), &mut state),
            Err(SimdError::FloatingPoint(expected))
        );
        assert_eq!(state.xmm[0], destination);
    }
}

#[test]
fn sse4_packed_sad_offsets_and_minimum_ties_have_exact_lane_order() {
    let mut state = SimdState::default();
    state.xmm[0] = std::array::from_fn(|i| i as u8);
    state.xmm[1] = std::array::from_fn(|i| 100 + i as u8);
    run(decode(&[0x66, 0x0f, 0x3a, 0x42, 0xc1, 0]), &mut state).unwrap();
    assert_eq!(
        state.xmm[0],
        words([400, 396, 392, 388, 384, 380, 376, 372])
    );
    state.xmm[0] = std::array::from_fn(|i| i as u8);
    run(decode(&[0x66, 0x0f, 0x3a, 0x42, 0xc1, 0xff]), &mut state).unwrap();
    assert_eq!(
        state.xmm[0],
        words([432, 428, 424, 420, 416, 412, 408, 404])
    );
    state.xmm[1] = words([0xffff, 7, 5, 5, 6, 9, 5, 8]);
    run(decode(&[0x66, 0x0f, 0x38, 0x41, 0xc1]), &mut state).unwrap();
    assert_eq!(state.xmm[0], words([5, 2, 0, 0, 0, 0, 0, 0]));
}

#[test]
fn sse4_ptest_sets_only_defined_arithmetic_flags_without_vector_writeback() {
    for (a, b, flags) in [
        (0, 0, 0x443),
        (0, 0xff, 0x442),
        (0xff, 0xff, 0x403),
        (0x0f, 0x33, 0x402),
    ] {
        let mut state = SimdState::default();
        state.mxcsr = 0xdfe5;
        state.xmm[0] = [a; 16];
        state.xmm[1] = [b; 16];
        let before = state;
        let result = run(decode(&[0x66, 0x0f, 0x38, 0x17, 0xc1]), &mut state).unwrap();
        assert_eq!(result.flags, flags);
        assert_eq!(result.gpr, 0x1020_3040);
        assert_eq!(state, before);
    }
}
