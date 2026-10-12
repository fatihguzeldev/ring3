use super::far_support as support;
use ring3_engine::cpu::x86::{
    FarWidth, SegmentRegister,
    decode::DecodeError,
    ir::{AddressSize, FarPointer, Operation},
};

#[test]
fn immediate_far_calls_keep_typed_width_selector_and_offset() {
    for (bytes, width, offset) in [
        (
            &[0x9a, 0x78, 0x56, 0x34, 0x12, 0x47, 0][..],
            FarWidth::Dword,
            0x12345678,
        ),
        (
            &[0x66, 0x9a, 0x78, 0x56, 0x47, 0][..],
            FarWidth::Word,
            0x5678,
        ),
    ] {
        assert_eq!(
            support::decode(bytes),
            Ok(Operation::FarCall {
                width,
                target: FarPointer::Immediate {
                    offset,
                    selector: 0x47
                }
            })
        );
    }
}

#[test]
fn far_call_memory_preserves_default_stack_segment_overrides_and_address_size() {
    for word in [false, true] {
        for (prefix, operand, segment, size) in [
            (
                &[][..],
                &[0xff, 0x1f][..],
                SegmentRegister::Ds,
                AddressSize::Bits32,
            ),
            (
                &[0x64][..],
                &[0xff, 0x1f],
                SegmentRegister::Fs,
                AddressSize::Bits32,
            ),
            (
                &[][..],
                &[0xff, 0x5d, 0],
                SegmentRegister::Ss,
                AddressSize::Bits32,
            ),
            (
                &[0x67][..],
                &[0xff, 0x1a],
                SegmentRegister::Ss,
                AddressSize::Bits16,
            ),
        ] {
            let mut bytes = if word { vec![0x66] } else { vec![] };
            bytes.extend(prefix);
            bytes.extend(operand);
            let Operation::FarCall {
                width,
                target: FarPointer::Memory(address),
            } = support::decode(&bytes).unwrap()
            else {
                panic!("far memory CALL")
            };
            assert_eq!(
                width,
                if word {
                    FarWidth::Word
                } else {
                    FarWidth::Dword
                }
            );
            assert_eq!(address.resolved_segment(), segment);
            assert_eq!(address.address_size, size);
        }
    }
}

#[test]
fn every_far_call_form_terminates_all_bound_profiles_without_stack_or_target_mapping() {
    for bytes in [
        &[0x9a, 0, 0x20, 0, 0, 0x43, 0][..],
        &[0x66, 0x9a, 0, 0x20, 0x43, 0],
        &[0xff, 0x1f],
        &[0x66, 0xff, 0x1f],
    ] {
        support::bound(bytes);
    }
}

#[test]
fn far_call_invalid_register_lock_and_reserved_repeat_encodings_stay_closed() {
    for bytes in [
        &[0xff, 0xd8][..],
        &[0xf0, 0xff, 0x1f],
        &[0xf0, 0x9a, 0, 0, 0, 0, 0, 0],
    ] {
        assert_eq!(support::decode(bytes), Err(DecodeError::InvalidEncoding));
    }
    for prefix in [0xf2, 0xf3] {
        assert!(matches!(
            support::decode(&[prefix, 0xff, 0x1f]),
            Err(DecodeError::Unsupported(_))
        ));
    }
}
