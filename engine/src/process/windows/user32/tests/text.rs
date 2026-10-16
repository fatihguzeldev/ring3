use super::*;
use crate::windows::text::CodePage32;
#[test]
fn ansi_class_names_decode_complete_dbcs_strings_with_configured_process_acp() {
    let (mut context, mut memory, mut call) = fixture();
    call.ansi = CodePage32::from_id(932).unwrap();
    memory
        .write(
            GuestAddress(0x10000),
            &[0x82, 0xa0, b'c', b'l', b'a', b's', b's', 0],
        )
        .unwrap();
    let fields = [0, 0x70000, 0, 0, call.instance, 0, 0, 0, 0, 0x10000];
    for (index, value) in fields.into_iter().enumerate() {
        memory
            .write(
                GuestAddress(0x10100 + index as u32 * 4),
                &value.to_le_bytes(),
            )
            .unwrap();
    }
    let User32Outcome32::Return(atom) = context
        .prepare_arguments(
            crate::windows::user32::User32Api32::RegisterClassA,
            &[0x10100],
            &mut memory,
            call,
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        context
            .state
            .class(
                call.instance,
                &"あclass".encode_utf16().collect::<Vec<_>>(),
                None
            )
            .unwrap()
            .atom,
        u16::try_from(atom).unwrap()
    );
    assert_eq!(
        super::super::memory::string(&memory, 0x10000, false, 6, call.ansi).unwrap(),
        "あclass".encode_utf16().collect::<Vec<_>>()
    );
    assert!(super::super::memory::string(&memory, 0x10000, false, 5, call.ansi).is_err());
}
