mod delivery;
mod queries;

use crate::process::windows::user32::{User32CallContext32, User32Context32};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::{rawinput::RawInputApi32 as Api, window::WindowHandle32},
};
fn fixture() -> (User32Context32, AddressSpace, User32CallContext32, u32) {
    let (mut context, mut memory, call) = crate::process::windows::user32::tests::fixture();
    let atom = crate::process::windows::user32::tests::register(&mut context, &mut memory, call);
    let window =
        crate::process::windows::user32::tests::create(&mut context, &mut memory, call, atom);
    context
        .state
        .windows_mut()
        .set_focus(
            call.thread_id,
            Some(WindowHandle32::try_from(window).unwrap()),
        )
        .unwrap();
    context.input.focus = window;
    context.input.physical_focus = true;
    (context, memory, call, window)
}
fn register(
    context: &mut User32Context32,
    memory: &mut AddressSpace,
    usage: u16,
    flags: u32,
    window: u32,
) {
    let mut bytes = [0; 12];
    bytes[..2].copy_from_slice(&1u16.to_le_bytes());
    bytes[2..4].copy_from_slice(&usage.to_le_bytes());
    bytes[4..8].copy_from_slice(&flags.to_le_bytes());
    bytes[8..12].copy_from_slice(&window.to_le_bytes());
    memory.write(GuestAddress(0x10300), &bytes).unwrap();
    assert_eq!(
        context.raw_input_call(Api::RegisterRawInputDevices, &[0x10300, 1, 12], memory, 7),
        Ok(1)
    );
}
fn dword(memory: &AddressSpace, address: u32) -> u32 {
    let mut bytes = [0; 4];
    memory.read(GuestAddress(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}
fn key(context: &mut User32Context32, code: u16) {
    context
        .deliver_input(
            crate::input::source::SourceEvent32 {
                kind: crate::input::source::SourceKind32::Physical(crate::input::InputKind::Key {
                    code,
                    pressed: true,
                }),
                ..Default::default()
            },
            0,
        )
        .unwrap();
}
