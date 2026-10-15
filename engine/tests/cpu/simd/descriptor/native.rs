use ring3_engine::cpu::x86::simd::{
    self, SimdError, SimdKind, SimdOperand, SimdOperation, SimdRegister,
};

fn vector_move() -> SimdOperation {
    SimdOperation {
        kind: SimdKind::Move,
        destination: SimdOperand::Register(SimdRegister::Xmm(0)),
        source: SimdOperand::Register(SimdRegister::Xmm(1)),
        vector_bytes: 16,
        lane_bytes: 16,
        memory_bytes: 0,
        immediate: None,
        aligned: false,
        scalar: false,
        implicit_address: None,
    }
}

#[test]
fn public_encoder_rejects_invalid_widths_without_panicking() {
    for operation in [
        SimdOperation {
            lane_bytes: 3,
            ..vector_move()
        },
        SimdOperation {
            memory_bytes: 3,
            ..vector_move()
        },
    ] {
        assert_eq!(operation.descriptor(), Err(SimdError::InvalidDescriptor));
    }
}

#[test]
fn checked_encoder_preserves_literal_descriptor_and_rejects_invalid_shapes() {
    let operation = vector_move();
    let descriptor = operation.descriptor().unwrap();
    assert_eq!(descriptor, (0x3400, 0x0908, 0));
    assert_eq!(simd::decode_descriptor(descriptor.0, descriptor.1, descriptor.2), Ok(operation));
    for invalid in [
        SimdOperation { vector_bytes: 3, ..operation },
        SimdOperation { destination: SimdOperand::Register(SimdRegister::Xmm(8)), ..operation },
        SimdOperation { immediate: Some(1), ..operation },
    ] {
        assert_eq!(invalid.descriptor(), Err(SimdError::InvalidDescriptor));
    }
}
