mod delivery;
mod keyboard;
mod pointer;
mod translation;

use super::super::{User32CallContext32, User32Context32, User32Outcome32};
use crate::{
    input::{
        InputKind,
        source::{SourceEvent32, SourceKind32},
    },
    windows::user32::Point32,
};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::{
        user32::{User32Api32, User32Error32},
        window::WindowHandle32,
    },
};

fn event(sequence: u64, kind: InputKind) -> SourceEvent32 {
    SourceEvent32 {
        sequence,
        timestamp_ns: sequence * 1_000_000,
        kind: SourceKind32::Physical(kind),
        ..Default::default()
    }
}
fn key(context: &mut User32Context32, sequence: u64, code: u16, pressed: bool) {
    context
        .deliver_input(event(sequence, InputKind::Key { code, pressed }), 0)
        .unwrap();
}
fn query(
    context: &mut User32Context32,
    memory: &mut AddressSpace,
    call: User32CallContext32,
    api: User32Api32,
    args: &[u32],
) -> u32 {
    let User32Outcome32::Return(value) =
        context.prepare_arguments(api, args, memory, call).unwrap()
    else {
        panic!("query must complete");
    };
    value
}
fn focused() -> (User32Context32, AddressSpace, User32CallContext32, u32) {
    let (mut context, mut memory, call) = super::fixture();
    let atom = super::register(&mut context, &mut memory, call);
    let window = super::create(&mut context, &mut memory, call, atom);
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
    context.input.point = Point32 { x: 12, y: 24 };
    context.input.query_point = context.input.point;
    (context, memory, call, window)
}
