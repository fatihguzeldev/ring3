use ring3_engine::{
    cpu::x86::{
        decode::decode_one,
        ir::Operation,
        simd::{self, SimdError, SimdOperand, SimdOperation, SimdRegister, SimdState},
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

#[test]
fn sse4_decodes_widen_blend_insert_and_extract_memory_widths() {
    for base in [0x20, 0x30] {
        for (offset, width) in [8, 4, 2, 8, 4, 8].into_iter().enumerate() {
            let opcode = base + offset as u8;
            decode(&[0x66, 0x0f, 0x38, opcode, 0xc1]);
            let memory = decode(&[0x66, 0x0f, 0x38, opcode, 0x00]);
            assert_eq!(memory.memory_bytes, width);
            assert!(!memory.aligned);
        }
    }
    for opcode in [0x0c, 0x0d, 0x0e] {
        decode(&[0x66, 0x0f, 0x3a, opcode, 0xc1, 0xff]);
        let memory = decode(&[0x66, 0x0f, 0x3a, opcode, 0x00, 0xff]);
        assert_eq!(memory.memory_bytes, 16);
        assert!(memory.aligned);
    }
    for opcode in [0x10, 0x14, 0x15] {
        decode(&[0x66, 0x0f, 0x38, opcode, 0xc1]);
        let memory = decode(&[0x66, 0x0f, 0x38, opcode, 0x00]);
        assert_eq!(memory.memory_bytes, 16);
        assert!(memory.aligned);
    }
    for (opcode, width) in [
        (0x14, 1),
        (0x15, 2),
        (0x16, 4),
        (0x17, 4),
        (0x20, 1),
        (0x21, 4),
        (0x22, 4),
    ] {
        decode(&[0x66, 0x0f, 0x3a, opcode, 0xc1, 0xff]);
        let memory = decode(&[0x66, 0x0f, 0x3a, opcode, 0x00, 0xff]);
        assert_eq!(memory.memory_bytes, width);
        assert!(!memory.aligned);
    }
}

fn dwords(values: [u32; 4]) -> [u8; 16] {
    let mut bytes = [0; 16];
    for (index, value) in values.into_iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn run(
    op: SimdOperation,
    state: &mut SimdState,
    memory: &mut [u8; 16],
    gpr: u32,
) -> simd::SimdResult {
    simd::execute(op, state, &mut [[0; 8]; 8], memory, gpr, 0xa5a5_a5a5, 0xcd7).unwrap()
}

#[test]
fn sse4_widenings_extend_only_low_source_lanes() {
    let source = [
        0x80, 0x7f, 0xff, 0, 1, 0xfe, 0x81, 0x55, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x66, 0x77,
    ];
    for (base, signed) in [(0x20, true), (0x30, false)] {
        for (offset, (from, to)) in [(1, 2), (1, 4), (1, 8), (2, 4), (2, 8), (4, 8)]
            .into_iter()
            .enumerate()
        {
            let mut state = SimdState {
                mxcsr: 0xdfe5,
                ..SimdState::default()
            };
            state.xmm[0] = [0xa5; 16];
            state.xmm[1] = source;
            let result = run(
                decode(&[0x66, 0x0f, 0x38, base + offset as u8, 0xc1]),
                &mut state,
                &mut [0; 16],
                0,
            );
            let mut expected = [0; 16];
            for lane in 0..16 / to {
                let input = &source[lane * from..(lane + 1) * from];
                let fill = if signed && input[from - 1] & 0x80 != 0 {
                    0xff
                } else {
                    0
                };
                expected[lane * to..(lane + 1) * to].fill(fill);
                expected[lane * to..lane * to + from].copy_from_slice(input);
            }
            assert_eq!(state.xmm[0], expected);
            assert_eq!(state.mxcsr, 0xdfe5);
            assert_eq!(result.flags, 0xcd7);
        }
    }
}

#[test]
fn sse4_variable_blend_captures_implicit_mask_before_destination_write() {
    let mut state = SimdState::default();
    state.xmm[0] = std::array::from_fn(|index| if index % 2 == 0 { 0x80 } else { 1 });
    state.xmm[1] = [0x55; 16];
    run(
        decode(&[0x66, 0x0f, 0x38, 0x10, 0xc1]),
        &mut state,
        &mut [0; 16],
        0,
    );
    assert_eq!(
        state.xmm[0],
        std::array::from_fn(|index| if index % 2 == 0 { 0x55 } else { 1 })
    );
    state.xmm[0] = dwords([0x8000_0000, 0x7fff_ffff, 0xffff_ffff, 0]);
    state.xmm[1] = dwords([0x7f80_0001, 0x8000_0000, 0x7fc0_1234, 0x3f80_0000]);
    state.xmm[2] = dwords([1, 2, 3, 4]);
    state.mxcsr = 0x1f00;
    run(
        decode(&[0x66, 0x0f, 0x38, 0x14, 0xd1]),
        &mut state,
        &mut [0; 16],
        0,
    );
    assert_eq!(state.xmm[2], dwords([0x7f80_0001, 2, 0x7fc0_1234, 4]));
    assert_eq!(state.mxcsr, 0x1f00);
}

#[test]
fn sse4_insert_extract_mask_selectors_preserve_bits_and_memory_canaries() {
    let mut state = SimdState::default();
    state.xmm[0] = dwords([1, 2, 3, 4]);
    state.xmm[1] = dwords([10, 20, 30, 40]);
    run(
        decode(&[0x66, 0x0f, 0x3a, 0x21, 0xc1, 0xb6]),
        &mut state,
        &mut [0; 16],
        0,
    );
    assert_eq!(state.xmm[0], dwords([1, 0, 0, 30]));
    state.xmm[0] = dwords([1, 2, 3, 4]);
    let mut memory = dwords([0x7f80_1234, 0xdead_beef, 0xffff_ffff, 0x1111_1111]);
    run(
        decode(&[0x66, 0x0f, 0x3a, 0x21, 0x00, 0xc0]),
        &mut state,
        &mut memory,
        0,
    );
    assert_eq!(state.xmm[0], dwords([0x7f80_1234, 2, 3, 4]));
    state.xmm[0] = [0; 16];
    run(
        decode(&[0x66, 0x0f, 0x3a, 0x20, 0xc1, 0xff]),
        &mut state,
        &mut memory,
        0xdead_beef,
    );
    assert_eq!(state.xmm[0][15], 0xef);
    assert_eq!(&state.xmm[0][..15], &[0; 15]);
    let extracted = run(
        decode(&[0x66, 0x0f, 0x3a, 0x14, 0xc1, 0xff]),
        &mut state,
        &mut memory,
        0,
    );
    assert_eq!(extracted.gpr, 0xef);
    for (opcode, width, value) in [
        (0x14, 1, 0x78u32),
        (0x15, 2, 0x5678),
        (0x16, 4, 0x1234_5678),
        (0x17, 4, 0x1234_5678),
    ] {
        state.xmm[0] = [0; 16];
        state.xmm[0][16 - width..].copy_from_slice(&value.to_le_bytes()[..width]);
        memory = [0xa5; 16];
        let before = state;
        let result = run(
            decode(&[0x66, 0x0f, 0x3a, opcode, 0x00, 0xff]),
            &mut state,
            &mut memory,
            0,
        );
        assert_eq!(&memory[..width], &value.to_le_bytes()[..width]);
        assert_eq!(&memory[width..], &[0xa5; 16][width..]);
        assert_eq!(state, before);
        assert_eq!(result.flags, 0xcd7);
    }
}

#[test]
fn sse4_movement_descriptors_reject_wrong_widths_and_mmx_without_effects() {
    let mut bad_memory_width = decode(&[0x66, 0x0f, 0x38, 0x22, 0x00]);
    bad_memory_width.memory_bytes = 4;
    let mut bad_widen = decode(&[0x66, 0x0f, 0x38, 0x20, 0xc1]);
    bad_widen.lane_bytes = 1;
    let mut bad_mmx = decode(&[0x66, 0x0f, 0x38, 0x30, 0xc1]);
    bad_mmx.vector_bytes = 8;
    bad_mmx.destination = SimdOperand::Register(SimdRegister::Mmx(0));
    bad_mmx.source = SimdOperand::Register(SimdRegister::Mmx(1));
    let mut bad_insert = decode(&[0x66, 0x0f, 0x3a, 0x20, 0xc1, 0]);
    bad_insert.lane_bytes = 2;
    let mut bad_blend = decode(&[0x66, 0x0f, 0x38, 0x10, 0xc1]);
    bad_blend.immediate = Some(1);
    let mut bad_extract = decode(&[0x66, 0x0f, 0x3a, 0x15, 0x00, 0]);
    bad_extract.vector_bytes = 8;
    bad_extract.source = SimdOperand::Register(SimdRegister::Mmx(0));
    for op in [
        bad_memory_width,
        bad_widen,
        bad_mmx,
        bad_insert,
        bad_blend,
        bad_extract,
    ] {
        let mut state = SimdState::default();
        state.xmm[0] = [0xa5; 16];
        let before = state;
        let mut mmx = [[0x77; 8]; 8];
        let mut memory = [0x33; 16];
        assert_eq!(
            simd::execute(op, &mut state, &mut mmx, &mut memory, 0, 0, 0xcd7),
            Err(SimdError::InvalidDescriptor)
        );
        assert_eq!(state, before);
        assert_eq!(mmx, [[0x77; 8]; 8]);
        assert_eq!(memory, [0x33; 16]);
    }
}
