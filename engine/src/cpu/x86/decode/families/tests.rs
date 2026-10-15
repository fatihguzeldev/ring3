use crate::{
    cpu::{
        UnsupportedFeature,
        x86::{
            StackFrameKind, SystemRegisterQuery, X87Operation,
            decode::{DecodeError, DecodedInstruction, decode_one},
            ir::Operation,
        },
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn decode(bytes: &[u8]) -> Result<DecodedInstruction, DecodeError> {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    decode_one(&memory, GuestAddress(0x1000))
}

#[test]
fn family_admission_keeps_legacy_x87_shapes_and_typed_operands() {
    for (bytes, expected) in [
        (&[0xdb, 0xe3][..], Operation::InitializeX87),
        (&[0xdb, 0xe2], Operation::ClearX87Exceptions),
        (&[0xdf, 0xe0], Operation::X87StatusToAx),
        (
            &[0xd9, 0xe8],
            Operation::X87 {
                operation: X87Operation::LoadConstant { index: 0 },
                address: None,
                waiting: true,
                opcode: 0x1e8,
            },
        ),
        (
            &[0xc8, 8, 0, 2],
            Operation::StackFrame {
                kind: StackFrameKind::Enter32,
                allocation: 8,
                nesting: 2,
            },
        ),
    ] {
        let decoded = decode(bytes).unwrap();
        assert_eq!(decoded.operation(), &expected);
        assert_eq!(usize::from(decoded.length()), bytes.len());
    }
    let decoded = decode(&[0x66, 0x0f, 0xfc, 0xc1]).unwrap();
    let Operation::Simd(operation) = decoded.operation() else {
        panic!("typed SIMD instruction");
    };
    assert_eq!(operation.descriptor(), Ok((0x2404, 0x0908, 0)));
}

#[test]
fn prefix_refusal_precedes_missing_system_bank_authority() {
    assert!(matches!(
        decode(&[0x0f, 0x01, 0xe0]),
        Err(DecodeError::MissingSystemRegisterAuthority(
            SystemRegisterQuery::MachineStatusWord
        ))
    ));
    for bytes in [
        &[0xf3, 0x0f, 0x01, 0xe0][..],
        &[0x66, 0x66, 0x0f, 0x01, 0xe0],
    ] {
        assert!(matches!(
            decode(bytes),
            Err(DecodeError::Unsupported(UnsupportedFeature::Opcode))
        ));
    }
}
