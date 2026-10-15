use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::x86::{
        decode::decode_one,
        ir::Operation,
        simd::{self, SimdOperation, SimdState},
    },
    memory::GuestAddress,
    process::EngineInstance,
};

fn decode(bytes: &[u8]) -> SimdOperation {
    let mut engine = EngineInstance::new(1, 0x5353_5345_3300_0001).unwrap();
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
    let (descriptor, operands, immediate) = op.descriptor().unwrap();
    let decoded = simd::decode_descriptor(descriptor, operands, immediate).unwrap();
    assert_eq!(decoded.kind, op.kind);
    assert_eq!(decoded.destination, op.destination);
    assert_eq!(decoded.source, op.source);
    assert_eq!(decoded.lane_bytes, op.lane_bytes);
    assert_eq!(decoded.vector_bytes, op.vector_bytes);
    *op
}

fn words(values: &[i16]) -> [u8; 16] {
    let mut bytes = [0; 16];
    for (index, value) in values.iter().enumerate() {
        bytes[index * 2..index * 2 + 2].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn ssse3_decodes_every_legacy_mnemonic_for_mmx_and_xmm() {
    for opcode in [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x1c, 0x1d, 0x1e,
    ] {
        for prefix in [false, true] {
            let mut bytes = Vec::new();
            if prefix {
                bytes.push(0x66);
            }
            bytes.extend([0x0f, 0x38, opcode, 0xc1]);
            let op = decode(&bytes);
            assert_eq!(op.vector_bytes, if prefix { 16 } else { 8 });
        }
    }
    for prefix in [false, true] {
        let mut bytes = Vec::new();
        if prefix {
            bytes.push(0x66);
        }
        bytes.extend([0x0f, 0x3a, 0x0f, 0xc1, 0xff]);
        assert_eq!(decode(&bytes).immediate, Some(255));
    }
}

#[test]
fn ssse3_rounding_wraps_the_positive_boundary_and_saturation_is_explicit() {
    let mut state = SimdState::default();
    let mut mmx = [[0; 8]; 8];
    let mut memory = [0; 16];
    state.xmm[0] = words(&[i16::MIN, 16384, -16384, 1, -1, 32767, 3, -3]);
    state.xmm[1] = words(&[i16::MIN, 16384, 16384, 16384, 16384, 32767, 16384, 16384]);
    let op = decode(&[0x66, 0x0f, 0x38, 0x0b, 0xc1]);
    let result = simd::execute(op, &mut state, &mut mmx, &mut memory, 0, 0, 0xcd7).unwrap();
    assert_eq!(
        state.xmm[0],
        words(&[i16::MIN, 8192, -8192, 1, 0, 32766, 2, -1])
    );
    assert_eq!(result.flags, 0xcd7);
    state.xmm[0] = words(&[32767, 1, -32768, -1, 10, 20, -10, -20]);
    state.xmm[1] = words(&[30000, 30000, -30000, -30000, 1, 2, 3, 4]);
    let op = decode(&[0x66, 0x0f, 0x38, 0x03, 0xc1]);
    simd::execute(op, &mut state, &mut mmx, &mut memory, 0, 0, 0).unwrap();
    assert_eq!(
        state.xmm[0],
        words(&[32767, -32768, 30, -30, 32767, -32768, 3, 7])
    );
}

#[test]
fn ssse3_rejects_malformed_lane_and_immediate_descriptors_without_effects() {
    let mut state = SimdState::default();
    state.xmm[0] = [0x55; 16];
    state.xmm[1] = [0xaa; 16];
    for (bytes, invalid_width, invalid_immediate) in [
        (&[0x66, 0x0f, 0x38, 0x08, 0xc1][..], 8, None),
        (&[0x66, 0x0f, 0x38, 0x03, 0xc1][..], 4, None),
        (&[0x66, 0x0f, 0x38, 0x04, 0xc1][..], 2, None),
        (&[0x66, 0x0f, 0x38, 0x0b, 0xc1][..], 4, None),
        (&[0x66, 0x0f, 0x38, 0x00, 0xc1][..], 1, Some(1)),
        (&[0x66, 0x0f, 0x3a, 0x0f, 0xc1, 0][..], 2, Some(0)),
    ] {
        let mut op = decode(bytes);
        op.lane_bytes = invalid_width;
        op.immediate = invalid_immediate;
        let before = state;
        let mut mmx = [[0; 8]; 8];
        let mut memory = [0x33; 16];
        assert_eq!(
            simd::execute(op, &mut state, &mut mmx, &mut memory, 0, 0, 0),
            Err(simd::SimdError::InvalidDescriptor)
        );
        assert_eq!(state, before);
        assert_eq!(memory, [0x33; 16]);
        assert_eq!(mmx, [[0; 8]; 8]);
    }
}
