use super::*;
use crate::{
    loader::ResourceIdentifier32,
    memory::{GuestAddress, PageRange},
    process::windows::{
        gdi::captured_gdi_call_for_test,
        user32::images::tests::{fixture_image, resource_image, word},
    },
    windows::{gdi::GdiApi32, user32::User32Api32},
};

fn fixture() -> (EngineInstance, User32CallContext32, u16) {
    let mut image = resource_image(false);
    // the independently authored bitmap resource has the same ordinal as the launch case.
    word(&mut image, 0x470, 136);
    let mut engine = fixture_image(image);
    let context = engine.user32.as_mut().unwrap();
    context
        .state
        .install_standard_classes(GuestAddress(0x10000))
        .unwrap();
    context.builtin_procedures = Some([GuestAddress(0x10000); 2]);
    let atom = context.state.standard_atom(ControlKind32::Static);
    (
        engine,
        User32CallContext32 {
            thread_id: 1,
            instance: 0x400000,
            ..Default::default()
        },
        atom,
    )
}

fn complete(
    engine: &mut EngineInstance,
    call: User32CallContext32,
    mut outcome: User32Outcome32,
) -> (u32, Vec<[u32; 4]>) {
    let mut messages = Vec::new();
    for _ in 0..32 {
        let User32Outcome32::Callback(callback) = outcome else {
            let User32Outcome32::Return(result) = outcome else {
                panic!("no wait expected");
            };
            return (result, messages);
        };
        let value = if callback.count == 3 {
            0
        } else {
            messages.push(callback.arguments);
            let outcome = engine
                .execute_static_image_message(&callback.arguments, false, call)
                .unwrap()
                .unwrap_or_else(|| {
                    engine
                        .user32
                        .as_mut()
                        .unwrap()
                        .control_message(
                            &callback.arguments,
                            false,
                            engine.memory.as_mut().unwrap(),
                            call,
                        )
                        .unwrap()
                });
            let User32Outcome32::Return(value) = outcome else {
                panic!("no nested builtin callback expected");
            };
            value
        };
        outcome = engine
            .user32
            .as_mut()
            .unwrap()
            .complete_with_gdi(
                callback.continuation,
                value,
                engine.memory.as_mut().unwrap(),
                engine.gdi.as_mut(),
            )
            .unwrap();
    }
    panic!("bounded control lifecycle");
}

fn create(
    engine: &mut EngineInstance,
    call: User32CallContext32,
    atom: u16,
    flags: u32,
) -> (u32, Vec<[u32; 4]>) {
    let outcome = engine
        .user32
        .as_mut()
        .unwrap()
        .prepare_arguments(
            User32Api32::CreateWindowExA,
            &[
                0,
                u32::from(atom),
                136,
                0x8000_000e | flags,
                10,
                20,
                5,
                6,
                0,
                0,
                call.instance,
                0,
            ],
            engine.memory.as_mut().unwrap(),
            call,
        )
        .unwrap();
    complete(engine, call, outcome)
}

fn message(
    engine: &mut EngineInstance,
    call: User32CallContext32,
    window: u32,
    message: u32,
    wparam: u32,
    lparam: u32,
) -> u32 {
    let Some(User32Outcome32::Return(value)) = engine
        .execute_static_image_message(&[window, message, wparam, lparam], false, call)
        .unwrap()
    else {
        panic!("image message required");
    };
    value
}

#[test]
fn ordinal_bitmap_loads_through_cbt_and_sizes_before_existing_size_callback() {
    let (mut engine, call, atom) = fixture();
    engine
        .user32
        .as_mut()
        .unwrap()
        .prepare_arguments(
            User32Api32::SetWindowsHookExA,
            &[5, 0x10000, 0, call.thread_id],
            engine.memory.as_mut().unwrap(),
            call,
        )
        .unwrap();
    let (window, messages) = create(&mut engine, call, atom, 0);
    assert_ne!(window, 0);
    let handle = message(&mut engine, call, window, 0x173, 0, 0);
    assert_ne!(handle, 0);
    assert!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .state
            .control_state(WindowHandle32::try_from(window).unwrap())
            .unwrap()
            .image_owned
    );
    let info = engine
        .gdi
        .as_ref()
        .unwrap()
        .bitmap_info(engine.memory.as_ref().unwrap(), handle)
        .unwrap();
    assert_eq!((info.width, info.height), (2, 2));
    assert!(
        messages
            .iter()
            .any(|args| args[1] == 5 && args[3] == 0x0002_0002)
    );
    assert_eq!(message(&mut engine, call, window, 0x0e, 0, 0), 0);
    let outcome = engine
        .user32
        .as_mut()
        .unwrap()
        .prepare_arguments(
            User32Api32::DestroyWindow,
            &[window],
            engine.memory.as_mut().unwrap(),
            call,
        )
        .unwrap();
    assert_eq!(complete(&mut engine, call, outcome).0, 1);
    assert!(
        engine
            .gdi
            .as_ref()
            .unwrap()
            .bitmap_info(engine.memory.as_ref().unwrap(), handle)
            .is_err()
    );
}

#[test]
fn replacing_static_image_transfers_ownership_and_returns_the_live_previous_handle() {
    let (mut engine, call, atom) = fixture();
    let (window, _) = create(&mut engine, call, atom, 0);
    let initial = message(&mut engine, call, window, 0x173, 0, 0);
    let replacement = engine
        .load_static_control_image(
            call.instance,
            ResourceIdentifier32::Id(136),
            ImageKind32::Bitmap,
            14,
        )
        .unwrap();
    assert_ne!(initial, replacement);
    assert_eq!(
        message(&mut engine, call, window, 0x172, 0, replacement),
        initial
    );
    assert_eq!(message(&mut engine, call, window, 0x173, 0, 0), replacement);
    let outcome = engine
        .user32
        .as_mut()
        .unwrap()
        .prepare_arguments(
            User32Api32::DestroyWindow,
            &[window],
            engine.memory.as_mut().unwrap(),
            call,
        )
        .unwrap();
    assert_eq!(complete(&mut engine, call, outcome).0, 1);
    for handle in [initial, replacement] {
        engine
            .gdi
            .as_ref()
            .unwrap()
            .bitmap_info(engine.memory.as_ref().unwrap(), handle)
            .unwrap();
        engine
            .release_static_control_image(handle, ImageKind32::Bitmap)
            .unwrap();
    }
}

#[test]
fn centered_bitmap_preserves_requested_control_size_and_wrong_kind_does_not_replace_it() {
    let (mut engine, call, atom) = fixture();
    let (window, messages) = create(&mut engine, call, atom, 0x200);
    assert!(
        messages
            .iter()
            .any(|args| args[1] == 5 && args[3] == 0x0006_0005)
    );
    let initial = message(&mut engine, call, window, 0x173, 0, 0);
    assert_eq!(message(&mut engine, call, window, 0x172, 1, 0xdeadbeef), 0);
    assert_eq!(message(&mut engine, call, window, 0x173, 0, 0), initial);
}

#[test]
fn forced_thread_exit_releases_initial_images_after_retiring_its_selected_dc() {
    let (mut engine, call, atom) = fixture();
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine
        .configure_synchronization(Default::default())
        .unwrap();
    if engine.windows_thread_info(call.thread_id).is_err() {
        let primary = engine
            .threads
            .prepare_primary_thread(0x10000, PageRange::new(GuestAddress(0x11000), 1).unwrap())
            .unwrap();
        engine.threads.commit_primary_thread(primary);
    }
    let (window, _) = create(&mut engine, call, atom, 0);
    let handle = message(&mut engine, call, window, 0x173, 0, 0);
    let dc = captured_gdi_call_for_test(&mut engine, GdiApi32::CreateCompatibleDC.id(), &[0]);
    assert_ne!(dc, 0);
    assert_ne!(
        captured_gdi_call_for_test(&mut engine, GdiApi32::SelectObject.id(), &[dc, handle]),
        0
    );
    engine
        .terminate_windows_thread_for_test(call.thread_id, 99)
        .unwrap();
    assert!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .windows()
            .get(WindowHandle32::try_from(window).unwrap())
            .is_err()
    );
    assert!(
        engine
            .gdi
            .as_ref()
            .unwrap()
            .bitmap_info(engine.memory.as_ref().unwrap(), handle)
            .is_err()
    );
}

#[test]
fn destroying_a_selected_initial_image_defers_its_release_until_the_live_dc_retires() {
    let (mut engine, call, atom) = fixture();
    let (window, _) = create(&mut engine, call, atom, 0);
    let handle = message(&mut engine, call, window, 0x173, 0, 0);
    let dc = captured_gdi_call_for_test(&mut engine, GdiApi32::CreateCompatibleDC.id(), &[0]);
    assert_ne!(
        captured_gdi_call_for_test(&mut engine, GdiApi32::SelectObject.id(), &[dc, handle]),
        0
    );
    let outcome = engine
        .user32
        .as_mut()
        .unwrap()
        .prepare_arguments(
            User32Api32::DestroyWindow,
            &[window],
            engine.memory.as_mut().unwrap(),
            call,
        )
        .unwrap();
    assert_eq!(complete(&mut engine, call, outcome).0, 1);
    engine
        .gdi
        .as_ref()
        .unwrap()
        .bitmap_info(engine.memory.as_ref().unwrap(), handle)
        .unwrap();
    assert_eq!(
        captured_gdi_call_for_test(&mut engine, GdiApi32::DeleteDC.id(), &[dc]),
        1
    );
    captured_gdi_call_for_test(&mut engine, GdiApi32::GetStockObject.id(), &[13]);
    assert!(
        engine
            .gdi
            .as_ref()
            .unwrap()
            .bitmap_info(engine.memory.as_ref().unwrap(), handle)
            .is_err()
    );
}
