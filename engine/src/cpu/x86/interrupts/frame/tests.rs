use super::*;
use crate::cpu::x86::{ExceptionCause32, SelectorErrorCode32};

#[test]
fn optional_error_is_a_fourth_gate_width_slot_and_b16_preserves_high_esp() {
    let input = FrameInput32 {
        eip: 0x12345678,
        esp: 0xa5a50003,
        flags: 0x254fd7,
        cs: 0x1b,
    };
    let fault = EventOrigin32::Exception(ExceptionCause32::GeneralProtection(
        SelectorErrorCode32::descriptor(0x4b, false),
    ));
    let frame = EventFrame32::new(fault, input, FarWidth::Dword, false, true);
    assert_eq!(frame.offsets(), [0xffff, 0xfffb, 0xfff7, 0xfff3]);
    assert_eq!(frame.values(), [0x254fd7, 0x1b, 0x12345678, 0x48]);
    assert_eq!(
        (frame.slot(), frame.esp(), frame.flags(), frame.info()),
        (4, 0xa5a5fff3, 0x240cd7, 0x304)
    );
    let word = EventFrame32::new(fault, input, FarWidth::Word, false, false);
    assert_eq!(word.offsets(), [1, 0xffff, 0xfffd, 0xfffb]);
    assert_eq!(word.values(), [0x254fd7, 0x1b, 0x5678, 0x48]);
    assert_eq!(
        (word.esp(), word.flags(), word.info()),
        (0xa5a5fffb, 0x240ed7, 0x202)
    );
}

#[test]
fn software_vector_does_not_invent_error_slot_and_b32_uses_full_pointer() {
    let input = FrameInput32 {
        eip: 0x1200,
        esp: 0x10003,
        flags: 0x254fd7,
        cs: 0x1b,
    };
    let frame = EventFrame32::new(
        EventOrigin32::SoftwareInt { vector: 13 },
        input,
        FarWidth::Dword,
        true,
        false,
    );
    assert_eq!(frame.offsets(), [0xffff, 0xfffb, 0xfff7]);
    assert_eq!(frame.values(), [0x244fd7, 0x1b, 0x1200]);
    assert_eq!(
        (frame.esp(), frame.flags(), frame.info()),
        (0xfff7, 0x240ed7, 4)
    );
}
