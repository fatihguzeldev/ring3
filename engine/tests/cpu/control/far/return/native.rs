use super::far_support as support;
use ring3_engine::cpu::x86::{FarWidth, decode::DecodeError, ir::Operation};

#[test]
fn return_forms_keep_unsigned_adjustment_and_operand_width() {
    for (bytes, width, stack_adjust) in [
        (&[0xcb][..], FarWidth::Dword, 0),
        (&[0x66, 0xcb], FarWidth::Word, 0),
        (&[0xca, 0xff, 0xff], FarWidth::Dword, 0xffff),
        (&[0x66, 0xca, 0x34, 0x12], FarWidth::Word, 0x1234),
    ] {
        assert_eq!(
            support::decode(bytes),
            Ok(Operation::FarReturn {
                width,
                stack_adjust
            })
        );
    }
}

#[test]
fn return_capture_terminates_all_bound_profiles_without_reading_stack() {
    for bytes in [
        &[0xcb][..],
        &[0x66, 0xcb],
        &[0xca, 8, 0],
        &[0x66, 0xca, 8, 0],
    ] {
        support::bound(bytes);
    }
}

#[test]
fn return_lock_and_reserved_repeat_encodings_stay_closed() {
    assert_eq!(
        support::decode(&[0xf0, 0xcb]),
        Err(DecodeError::InvalidEncoding)
    );
    for prefix in [0xf2, 0xf3] {
        assert!(matches!(
            support::decode(&[prefix, 0xcb]),
            Err(DecodeError::Unsupported(_))
        ));
    }
}
