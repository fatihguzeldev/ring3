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
fn sse4_decodes_packed_load_compare_minmax_multiply_and_pack() {
    for opcode in [
        0x28, 0x29, 0x2b, 0x37, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 0x40,
    ] {
        decode(&[0x66, 0x0f, 0x38, opcode, 0xc1]);
        let memory = decode(&[0x66, 0x0f, 0x38, opcode, 0x00]);
        assert_eq!(memory.memory_bytes, 16);
        assert!(memory.aligned);
    }
    let load = decode(&[0x66, 0x0f, 0x38, 0x2a, 0x00]);
    assert_eq!(load.memory_bytes, 16);
    assert!(load.aligned);
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

#[test]
fn sse4_signed_products_and_unsigned_narrowing_preserve_boundaries() {
    let mut state = SimdState::default();
    let mut mmx = [[0; 8]; 8];
    let mut memory = [0; 16];
    state.xmm[0] = dwords([0xffff_fffe, 0xdeadbeef, 0x8000_0000, 0x12345678]);
    state.xmm[1] = dwords([0xffff_fffd, 0x87654321, 0xffff_ffff, 0x7fff_ffff]);
    let result = simd::execute(
        decode(&[0x66, 0x0f, 0x38, 0x28, 0xc1]),
        &mut state,
        &mut mmx,
        &mut memory,
        0,
        0,
        0xcd7,
    )
    .unwrap();
    assert_eq!(state.xmm[0], qwords([6, 0x8000_0000]));
    assert_eq!(result.flags, 0xcd7);
    state.xmm[0] = dwords([0xffff_ffff, 0, 65535, 65536]);
    state.xmm[1] = dwords([0x8000_0000, 0x7fff_ffff, 42, 0xffff_ffd6]);
    simd::execute(
        decode(&[0x66, 0x0f, 0x38, 0x2b, 0xc1]),
        &mut state,
        &mut mmx,
        &mut memory,
        0,
        0,
        0xcd7,
    )
    .unwrap();
    assert_eq!(
        state.xmm[0],
        [0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 255, 255, 42, 0, 0, 0]
    );
}

#[test]
fn sse4_only_widths_reject_mmx_descriptors_without_effects() {
    for opcode in [
        0x28, 0x29, 0x2b, 0x37, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 0x40,
    ] {
        let mut op = decode(&[0x66, 0x0f, 0x38, opcode, 0xc1]);
        op.destination = SimdOperand::Register(SimdRegister::Mmx(0));
        op.source = SimdOperand::Register(SimdRegister::Mmx(1));
        op.vector_bytes = 8;
        let mut state = SimdState::default();
        let before = state;
        let mut mmx = [[0x55; 8]; 8];
        let mut memory = [0x33; 16];
        assert_eq!(
            simd::execute(op, &mut state, &mut mmx, &mut memory, 0, 0, 0xcd7),
            Err(SimdError::InvalidDescriptor)
        );
        assert_eq!(state, before);
        assert_eq!(mmx, [[0x55; 8]; 8]);
        assert_eq!(memory, [0x33; 16]);
    }
}
