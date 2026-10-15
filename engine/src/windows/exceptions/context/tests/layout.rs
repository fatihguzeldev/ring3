use super::*;

#[test]
fn complete_context_has_literal_windows_offsets_and_round_trips_selected_families() {
    let source = state();
    let context = Win32Context32::capture(CONTEXT_ALL32, &source).unwrap();
    let bytes = context.bytes();
    assert_eq!(bytes.len(), 716);
    assert_eq!(word(bytes, 0), 0x1003f);
    for (offset, value) in [
        (4, 0x1000),
        (20, 0xffff0ff0),
        (24, 0x400),
        (28, 0x37f),
        (32, 0x1800),
        (36, 0),
        (40, 0x10203040),
        (44, 0x0123001b),
        (48, 0x50607080),
        (52, 0x23),
        (136, 0),
        (140, 0),
        (144, 0x3b),
        (148, 0x23),
        (152, 0x23),
        (156, 0x88888888),
        (160, 0x77777777),
        (164, 0x44444444),
        (168, 0x33333333),
        (172, 0x22222222),
        (176, 0x11111111),
        (180, 0x66666666),
        (184, 0x12345678),
        (188, 0x1b),
        (192, 0x202),
        (196, 0x55555555),
        (200, 0x23),
    ] {
        assert_eq!(word(bytes, offset), value, "offset{offset}");
    }
    assert_eq!(&bytes[56..66], &source.x87.registers[3]);
    assert_eq!(&bytes[126..136], &source.x87.registers[2]);
    assert_eq!(word(bytes, 204 + 24), 0x1f80);
    assert_eq!(&bytes[364..380], &[1; 16]);
    assert_eq!(&bytes[476..492], &[8; 16]);
    assert_eq!(context.apply(&source, source.segments).unwrap(), source);
    assert_eq!(Win32Context32::from_bytes(bytes).unwrap(), context);
}
#[test]
fn invalid_layout_flags_and_selected_mxcsr_are_rejected_without_applying_any_state() {
    let source = state();
    let good = Win32Context32::capture(CONTEXT_ALL32, &source).unwrap();
    assert_eq!(
        Win32Context32::from_bytes(&good.bytes()[..715]),
        Err(ContextError32::Length)
    );
    for flags in [0, 0x10040, 0x10080, 0x20001] {
        let mut bytes = *good.bytes();
        write(&mut bytes, 0, flags);
        assert_eq!(
            Win32Context32::from_bytes(&bytes),
            Err(ContextError32::UnsupportedFlags)
        );
    }
    let mut bytes = *good.bytes();
    write(&mut bytes, 228, 0x80000000);
    assert_eq!(
        Win32Context32::from_bytes(&bytes),
        Err(ContextError32::InvalidFloatingPoint)
    );
    write(&mut bytes, 0, CONTEXT_INTEGER32);
    assert!(
        Win32Context32::from_bytes(&bytes).is_ok(),
        "unselected extended state is opaque"
    );
}
