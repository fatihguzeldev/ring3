use super::*;

#[test]
fn all_geometries_capture_exact_slots_and_scales_without_general_or_flag_effects() {
    for kind in 1..=8_u8 {
        for destination in 0..8 {
            for index in 0..8 {
                for mask in 0..8 {
                    for power in 0..4 {
                        let operands = destination | (index << 3) | (mask << 9) | (1 << 12);
                        let result = AvxInstruction::decode_descriptor(
                            0x01001400 | u32::from(kind),
                            operands,
                            power,
                        );
                        if destination == index || destination == mask || index == mask {
                            assert_eq!(result, Err(AvxDecodeError::Operands));
                            continue;
                        }
                        let value = result.unwrap();
                        let AvxSemantic::Gather(gather) = value.semantic() else {
                            panic!("gather semantic")
                        };
                        assert_eq!(gather.geometry().id(), kind);
                        assert_eq!(gather.destination().index(), destination as usize);
                        assert_eq!(gather.index().index(), index as usize);
                        assert_eq!(gather.mask().index(), mask as usize);
                        assert_eq!(gather.scale().power(), power as u8);
                        assert_eq!(value.memory(), MemoryEffect::Gather(gather));
                        assert_eq!(value.inputs(), [None, None]);
                        assert_eq!(value.output(), None);
                        assert!(!value.writes_flags());
                        assert_eq!(
                            value.encode(),
                            (0x01001400 | u32::from(kind), operands, power)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn unknown_ids_and_unused_slots_preserve_exact_error_priority() {
    let operands = 1 | (2 << 3) | (3 << 9) | (1 << 12);
    for kind in [0, 9, 0x41, 0x81, 255] {
        assert_eq!(
            AvxInstruction::decode_descriptor(0x01001400 | kind, 0x3fff, 255),
            Err(AvxDecodeError::Family)
        );
    }
    for malformed in [
        operands | (1 << 6),
        operands | (1 << 13),
        operands & !(1 << 12),
        (operands & !7) | 2,
    ] {
        assert_eq!(
            AvxInstruction::decode_descriptor(0x01001401, malformed, 4),
            Err(AvxDecodeError::Operands)
        );
    }
    for power in [4, 7, 255] {
        assert_eq!(
            AvxInstruction::decode_descriptor(0x01001401, operands, power),
            Err(AvxDecodeError::Immediate)
        );
    }
    assert_eq!(
        AvxInstruction::decode_descriptor(0x02001401, operands, 0),
        Err(AvxDecodeError::Version)
    );
    assert_eq!(
        AvxInstruction::decode_descriptor(0x01011401, operands, 0),
        Err(AvxDecodeError::ReservedBits)
    );
}

#[test]
fn opaque_operation_constructors_keep_scalar_and_vsib_addresses_isolated() {
    use crate::cpu::x86::{
        Register32, SegmentRegister,
        avx::{AvxOperation, GatherAddress},
        ir::EffectiveAddress,
    };
    let value =
        AvxInstruction::decode_descriptor(0x01001401, 1 | (2 << 3) | (3 << 9) | (1 << 12), 0)
            .unwrap();
    assert_eq!(AvxOperation::new(value, None), Err(AvxDecodeError::Address));
    assert_eq!(
        AvxOperation::new(value, Some(EffectiveAddress::default())),
        Err(AvxDecodeError::Address)
    );
    let address = GatherAddress::new(Some(Register32::Ebp), u32::MAX, Some(SegmentRegister::Fs));
    let operation = AvxOperation::new_gather(value, address).unwrap();
    assert_eq!(operation.instruction(), value);
    assert_eq!(operation.address(), None);
    assert_eq!(operation.gather_address(), Some(address));
    let ordinary = AvxInstruction::decode_descriptor(0x01000201, 0, 0).unwrap();
    assert_eq!(
        AvxOperation::new_gather(ordinary, address),
        Err(AvxDecodeError::Address)
    );
    let scalar = AvxOperation::new(ordinary, None).unwrap();
    assert_eq!(scalar.gather_address(), None);
}
