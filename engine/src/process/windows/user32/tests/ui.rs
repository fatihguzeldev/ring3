use super::super::{User32CallContext32, User32Context32, User32Outcome32};
use crate::windows::user32::User32Error32 as Error;
use crate::{memory::AddressSpace, windows::user32::User32Api32 as Api};

fn fixture() -> (User32Context32, AddressSpace, User32CallContext32, u32, u32) {
    let (mut context, mut ram, call) = super::fixture();
    context.continuation_limit = 32;
    let atom = super::register(&mut context, &mut ram, call);
    let root = super::create(&mut context, &mut ram, call, atom);
    let mut outcome = context
        .prepare_arguments(
            Api::CreateWindowExA,
            &[
                0,
                atom,
                0,
                0x4000_0000,
                0,
                0,
                80,
                30,
                root,
                1,
                call.instance,
                0,
            ],
            &mut ram,
            call,
        )
        .unwrap();
    let child = loop {
        match outcome {
            User32Outcome32::Return(window) => {
                assert_ne!(window, 0);
                break window;
            }
            User32Outcome32::Callback(callback) => {
                outcome = context
                    .complete(
                        callback.continuation,
                        u32::from(callback.arguments[1] == 0x81),
                        &mut ram,
                    )
                    .unwrap();
            }
            _ => panic!("unexpected creation wait"),
        }
    };
    (context, ram, call, root, child)
}

fn finish(
    context: &mut User32Context32,
    ram: &mut AddressSpace,
    call: User32CallContext32,
    mut outcome: User32Outcome32,
    events: &mut Vec<[u32; 4]>,
    budget: &mut usize,
) -> u32 {
    loop {
        assert!(*budget != 0, "UI propagation must terminate");
        *budget -= 1;
        outcome = match outcome {
            User32Outcome32::Return(value) => return value,
            User32Outcome32::Wait { continuation } => {
                context.resume_wait(continuation, ram).unwrap()
            }
            User32Outcome32::Callback(callback) => {
                events.push(callback.arguments);
                let nested = context
                    .prepare_arguments(Api::DefWindowProcW, &callback.arguments, ram, call)
                    .unwrap();
                let result = finish(context, ram, call, nested, events, budget);
                context
                    .complete(callback.continuation, result, ram)
                    .unwrap()
            }
        };
    }
}

fn send(
    context: &mut User32Context32,
    ram: &mut AddressSpace,
    call: User32CallContext32,
    window: u32,
    message: u32,
    wparam: u32,
) -> Vec<[u32; 4]> {
    let outcome = context
        .prepare_arguments(
            Api::DefWindowProcW,
            &[window, message, wparam, 0],
            ram,
            call,
        )
        .unwrap();
    let mut events = Vec::new();
    assert_eq!(
        finish(context, ram, call, outcome, &mut events, &mut 128),
        0
    );
    assert!(context.pending.is_empty());
    events
}

#[test]
fn fresh_windows_start_visible_and_ui_changes_follow_real_parent_child_callbacks() {
    let (mut context, mut ram, call, root, child) = fixture();
    assert_eq!(bits(&mut context, &mut ram, call, [root, child]), [0, 0]);
    let version = ram.transaction_version();
    assert_eq!(
        send(&mut context, &mut ram, call, root, 0x127, 0x30001),
        [[root, 0x128, 0x30001, 0], [child, 0x128, 0x30001, 0]]
    );
    assert_eq!(bits(&mut context, &mut ram, call, [root, child]), [3, 3]);
    assert_eq!(
        send(&mut context, &mut ram, call, root, 0x127, 0x20002),
        [[root, 0x128, 0x20002, 0], [child, 0x128, 0x20002, 0]]
    );
    assert_eq!(bits(&mut context, &mut ram, call, [root, child]), [1, 1]);
    assert_eq!(
        send(&mut context, &mut ram, call, child, 0x127, 0x20001),
        [
            [root, 0x127, 0x20001, 0],
            [root, 0x128, 0x20001, 0],
            [child, 0x128, 0x20001, 0]
        ]
    );
    assert_eq!(bits(&mut context, &mut ram, call, [root, child]), [3, 3]);
    assert_eq!(
        send(&mut context, &mut ram, call, root, 0x128, 0x10002),
        [[child, 0x128, 0x10002, 0]]
    );
    assert_eq!(bits(&mut context, &mut ram, call, [root, child]), [2, 2]);
    assert!(send(&mut context, &mut ram, call, root, 0x128, 0x70055).is_empty());
    assert_eq!(bits(&mut context, &mut ram, call, [root, child]), [2, 2]);
    assert_eq!(
        send(&mut context, &mut ram, call, root, 0x127, 0x70003),
        [[root, 0x128, 0x30001, 0], [child, 0x128, 0x30001, 0]]
    );
    assert_eq!(bits(&mut context, &mut ram, call, [root, child]), [3, 3]);
    context
        .deliver_input(
            crate::input::source::SourceEvent32 {
                kind: crate::input::source::SourceKind32::Physical(crate::input::InputKind::Key {
                    code: 0x1e,
                    pressed: true,
                }),
                ..Default::default()
            },
            0,
        )
        .unwrap();
    assert_eq!(
        send(&mut context, &mut ram, call, root, 0x127, 0x70003),
        [[root, 0x128, 0x30002, 0], [child, 0x128, 0x30002, 0]]
    );
    assert_eq!(bits(&mut context, &mut ram, call, [root, child]), [0, 0]);
    assert_eq!(ram.transaction_version(), version);
}

#[test]
fn ui_update_reserves_nested_callback_capacity_before_changing_any_window() {
    let (mut context, mut ram, call, root, child) = fixture();
    context.continuation_limit = 1;
    let token = context.next_token;
    assert_eq!(
        context.def_window_proc(&[root, 0x128, 0x30001, 0], &mut ram, call),
        Err(Error::Capacity)
    );
    assert_eq!(context.next_token, token);
    assert!(context.pending.is_empty());
    for raw in [root, child] {
        assert_eq!(bits(&mut context, &mut ram, call, [raw, raw]), [0, 0]);
    }
}

fn bits(
    context: &mut User32Context32,
    ram: &mut AddressSpace,
    call: User32CallContext32,
    windows: [u32; 2],
) -> [u32; 2] {
    windows.map(|window| {
        match context
            .def_window_proc(&[window, 0x129, 0, 0], ram, call)
            .unwrap()
        {
            User32Outcome32::Return(value) => value,
            _ => panic!("UI query must return"),
        }
    })
}
