use crate::process::windows::user32::{
    User32Outcome32,
    tests::{create, fixture},
};
use crate::{memory::GuestAddress, windows::controls::ControlKind32};

impl super::super::User32Context32 {
    pub(in crate::process) fn standard_control_gate(
        &self,
        kind: ControlKind32,
    ) -> (u16, GuestAddress) {
        (
            self.state.standard_atom(kind),
            self.builtin_procedures.unwrap()[1],
        )
    }
}
fn result(value: User32Outcome32) -> u32 {
    match value {
        User32Outcome32::Return(value) => value,
        _ => panic!("unexpected callback"),
    }
}
#[test]
fn standard_classes_do_not_consume_guest_class_quota_and_preserve_ansi_caption_and_edit_limits() {
    let (mut context, mut ram, call) = fixture();
    context.builtin_procedures = Some([GuestAddress(0x70000), GuestAddress(0x70020)]);
    context
        .state
        .install_standard_classes(GuestAddress(0x70020))
        .unwrap();
    let atom = context.state.standard_atom(ControlKind32::Edit);
    let window = create(&mut context, &mut ram, call, atom as u32);
    ram.write(GuestAddress(0x10100), b"hello\0").unwrap();
    assert_eq!(
        result(
            context
                .control_message(&[window, 0x0c, 0, 0x10100], false, &mut ram, call)
                .unwrap()
        ),
        1
    );
    assert_eq!(
        result(
            context
                .control_message(&[window, 0x0e, 0, 0], false, &mut ram, call)
                .unwrap()
        ),
        5
    );
    context
        .control_message(&[window, 0xc5, 3, 0], false, &mut ram, call)
        .unwrap();
    assert_eq!(
        result(
            context
                .control_message(&[window, 0xd5, 0, 0], false, &mut ram, call)
                .unwrap()
        ),
        3
    );
    context
        .control_message(&[window, 0xb1, 0, u32::MAX], false, &mut ram, call)
        .unwrap();
    context
        .control_message(&[window, 0xc2, 0, 0x10100], false, &mut ram, call)
        .unwrap();
    assert_eq!(
        context
            .state
            .text(crate::windows::window::WindowHandle32::try_from(window).unwrap())
            .unwrap(),
        &[104, 101, 108]
    );
    crate::process::windows::user32::tests::register(&mut context, &mut ram, call);
}
#[test]
fn combo_box_message_numbers_share_exact_count_selection_and_owned_item_storage() {
    let (mut context, mut ram, call) = fixture();
    context.builtin_procedures = Some([GuestAddress(0x70000), GuestAddress(0x70020)]);
    context
        .state
        .install_standard_classes(GuestAddress(0x70020))
        .unwrap();
    let atom = context.state.standard_atom(ControlKind32::ComboBox);
    let window = create(&mut context, &mut ram, call, atom as u32);
    ram.write(GuestAddress(0x10100), b"first\0").unwrap();
    assert_eq!(
        result(
            context
                .control_message(&[window, 0x143, 0, 0x10100], false, &mut ram, call)
                .unwrap()
        ),
        0
    );
    assert_eq!(
        result(
            context
                .control_message(&[window, 0x146, 0, 0], false, &mut ram, call)
                .unwrap()
        ),
        1
    );
    assert_eq!(
        result(
            context
                .control_message(&[window, 0x14e, 0, 0], false, &mut ram, call)
                .unwrap()
        ),
        0
    );
    assert_eq!(
        result(
            context
                .control_message(&[window, 0x147, 0, 0], false, &mut ram, call)
                .unwrap()
        ),
        0
    );
    assert_eq!(
        result(
            context
                .control_message(&[window, 0x148, 0, 0x10200], false, &mut ram, call)
                .unwrap()
        ),
        5
    );
    let mut bytes = [0; 6];
    ram.read(GuestAddress(0x10200), &mut bytes).unwrap();
    assert_eq!(&bytes, b"first\0");
    context
        .control_message(&[window, 0x144, 0, 0], false, &mut ram, call)
        .unwrap();
    assert_eq!(
        result(
            context
                .control_message(&[window, 0x147, 0, 0], false, &mut ram, call)
                .unwrap()
        ),
        u32::MAX
    );
}
