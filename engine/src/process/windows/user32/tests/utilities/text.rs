use super::*;
#[test]
fn variadic_text_formats_signed_padding_precision_wide_strings_and_escaped_percent() {
    let (mut ctx, mut m, c) = fixture();
    m.write(GuestAddress(0x10000), b"%05d %.3s %S %% %08X\0")
        .unwrap();
    m.write(GuestAddress(0x10100), b"abcdef\0").unwrap();
    m.write(GuestAddress(0x10200), &[b'X', 0, b'Y', 0, 0, 0])
        .unwrap();
    for (i, v) in [(-23i32) as u32, 0x10100, 0x10200, 0xaau32]
        .into_iter()
        .enumerate()
    {
        m.write(GuestAddress(0x10300 + i as u32 * 4), &v.to_le_bytes())
            .unwrap();
    }
    assert_eq!(
        call(
            &mut ctx,
            &mut m,
            c,
            User32Api32::WvsprintfA,
            &[0x10400, 0x10000, 0x10300]
        ),
        23
    );
    let mut out = [0; 24];
    m.read(GuestAddress(0x10400), &mut out).unwrap();
    assert_eq!(&out, b"-0023 abc XY % 000000AA\0");
}
#[test]
fn oem_conversion_and_character_uppercase_use_the_active_codepage() {
    let (mut ctx, mut m, c) = fixture();
    m.write(GuestAddress(0x10000), &[0xe9, 0]).unwrap();
    assert_eq!(
        call(
            &mut ctx,
            &mut m,
            c,
            User32Api32::CharToOemA,
            &[0x10000, 0x10100]
        ),
        1
    );
    let mut b = [0; 2];
    m.read(GuestAddress(0x10100), &mut b).unwrap();
    assert_eq!(b, [0x82, 0]);
    assert_eq!(
        call(&mut ctx, &mut m, c, User32Api32::CharUpperA, &[0xe9]),
        0xc9
    );
    assert_eq!(
        call(&mut ctx, &mut m, c, User32Api32::CharNextA, &[0x10001]),
        0x10001
    );
}

#[test]
fn captured_wsprintf_reads_variadic_stack_and_preserves_caller_cleanup() {
    use crate::{
        cpu::x86::{Register32, State32},
        windows::{CallFrame32, CallingConvention32},
    };
    let (mut ctx, mut m, c) = fixture();
    m.write(GuestAddress(0x10000), b"value=%d\0").unwrap();
    let stack = [0x70000u32, 0x10300, 0x10000, (-17i32) as u32];
    m.write(
        GuestAddress(0x10400),
        &stack
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let info =
        crate::process::windows::calls::windows_gate_info(User32Api32::WsprintfA.id()).unwrap();
    assert_eq!(info.convention, CallingConvention32::Cdecl);
    let mut state = State32::default();
    state.registers[Register32::Esp.index()] = 0x10400;
    let frame = CallFrame32::capture(&m, state, info.convention, info.stack_words).unwrap();
    let User32Outcome32::Return(length) = ctx
        .prepare(User32Api32::WsprintfA, &frame, &mut m, c)
        .unwrap()
    else {
        panic!("formatter must return")
    };
    assert_eq!(length, 9);
    let mut output = [0u8; 10];
    m.read(GuestAddress(0x10300), &mut output).unwrap();
    assert_eq!(&output, b"value=-17\0");
    let done = frame.complete(length);
    assert_eq!(done.eip, 0x70000);
    assert_eq!(done.registers[Register32::Esp.index()], 0x10404);
}

#[test]
fn precision_limits_memory_reads_and_short_integer_formats_narrow_before_padding() {
    let (mut ctx, mut m, c) = fixture();
    m.write(GuestAddress(0x11ffd), b"abc").unwrap();
    m.write(GuestAddress(0x10000), b"%.3s %hd %hu %#08x\0")
        .unwrap();
    let values = [0x11ffdu32, 0xffff, 0x12340001, 0x12];
    m.write(
        GuestAddress(0x10200),
        &values
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let value = call(
        &mut ctx,
        &mut m,
        c,
        User32Api32::WvsprintfA,
        &[0x10400, 0x10000, 0x10200],
    );
    assert_eq!(value, 17);
    let mut output = [0u8; 18];
    m.read(GuestAddress(0x10400), &mut output).unwrap();
    assert_eq!(&output, b"abc -1 1 0x000012\0");
}
