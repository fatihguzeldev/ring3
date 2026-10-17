#[path = "../service/tests/support.rs"]
mod support;
use crate::{
    memory::GuestAddress,
    process::EngineInstance,
    windows::{imm32::ImmApi32 as Api, user32::User32Limits32},
};
use support::*;

fn fixture() -> EngineInstance {
    let mut instance = engine();
    instance.register_imm32_module(0x7400_0000).unwrap();
    instance.load_library_module("imm32.dll", None).unwrap();
    instance.user32 = Some(
        crate::process::windows::user32::User32Context32::new(
            User32Limits32::default(),
            range(0x3200_0000, 4),
        )
        .unwrap(),
    );
    instance
}

#[test]
fn default_ime_window_is_a_real_thread_owned_hwnd_and_control_commands_use_the_same_context() {
    use crate::windows::{user32::User32Api32 as User, window::WindowHandle32};
    let mut e = fixture();
    e.register_user32_module(0x7500_0000).unwrap();
    e.load_library_module("user32.dll", None).unwrap();
    let window = call(&mut e, Api::ImmGetDefaultIMEWnd.id(), &[0]);
    assert_ne!(window, 0);
    assert_eq!(
        call(&mut e, Api::ImmGetDefaultIMEWnd.id(), &[window]),
        window
    );
    assert_eq!(call(&mut e, Api::ImmGetDefaultIMEWnd.id(), &[u32::MAX]), 0);
    let hwnd = WindowHandle32::try_from(window).unwrap();
    let description = e.user32_windows().unwrap().get(hwnd).unwrap().description;
    assert_eq!(description.thread_id, 1);
    assert!(!description.visible);
    let form = [2u32, 11, 17, 1, 2, 31, 41];
    put(
        &mut e,
        DATA,
        &form
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        call(
            &mut e,
            User::DefWindowProcW.id(),
            &[window, 0x283, 12, DATA]
        ),
        0
    );
    assert_eq!(
        call(
            &mut e,
            User::DefWindowProcW.id(),
            &[window, 0x283, 11, DATA + 64]
        ),
        0
    );
    for (index, value) in form.into_iter().enumerate() {
        assert_eq!(word(&e, DATA + 64 + index as u32 * 4), value);
    }
    let input = call(&mut e, Api::ImmGetContext.id(), &[window]);
    assert_eq!(
        call(
            &mut e,
            Api::ImmGetCompositionWindow.id(),
            &[input, DATA + 128]
        ),
        1
    );
    for (index, value) in form.into_iter().enumerate() {
        assert_eq!(word(&e, DATA + 128 + index as u32 * 4), value);
    }
    put(&mut e, DATA + 160, &[0xcc; 28]);
    assert_eq!(
        call(
            &mut e,
            User::DefWindowProcW.id(),
            &[window, 0x283, 0xffff, DATA + 160]
        ),
        1
    );
    assert_eq!(word(&e, DATA + 160), 0xcccc_cccc);
}

#[test]
fn captured_context_status_and_composition_preferences_roundtrip_through_exact_public_gates() {
    let mut e = fixture();
    let handle = call(&mut e, Api::ImmCreateContext.id(), &[]);
    assert_ne!(handle, 0);
    assert_eq!(call(&mut e, Api::ImmGetOpenStatus.id(), &[handle]), 0);
    assert_eq!(call(&mut e, Api::ImmSetOpenStatus.id(), &[handle, 1]), 1);
    assert_eq!(call(&mut e, Api::ImmGetOpenStatus.id(), &[handle]), 1);
    assert_eq!(
        call(
            &mut e,
            Api::ImmGetConversionStatus.id(),
            &[handle, DATA, DATA + 4]
        ),
        1
    );
    assert_eq!((word(&e, DATA), word(&e, DATA + 4)), (1, 8));
    put(&mut e, DATA, &[0xcc; 32]);
    assert_eq!(
        call(&mut e, Api::ImmGetCompositionWindow.id(), &[handle, DATA]),
        0
    );
    assert_eq!(word(&e, DATA), 0xcccc_cccc);
    let value: Vec<u8> = [2u32, 11, 17, 2, 3, 97, 63]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    put(&mut e, DATA, &value);
    assert_eq!(
        call(&mut e, Api::ImmSetCompositionWindow.id(), &[handle, DATA]),
        1
    );
    assert_eq!(
        call(
            &mut e,
            Api::ImmGetCompositionWindow.id(),
            &[handle, DATA + 64]
        ),
        1
    );
    let mut bytes = [0; 28];
    e.memory()
        .unwrap()
        .read(GuestAddress(DATA + 64), &mut bytes)
        .unwrap();
    assert_eq!(bytes.as_slice(), value);
    assert_eq!(call(&mut e, Api::ImmDestroyContext.id(), &[handle]), 1);
    assert_eq!(call(&mut e, Api::ImmSetOpenStatus.id(), &[handle, 1]), 0);
}

#[test]
fn captured_candidate_and_font_use_canonical_layouts_and_bad_inputs_preserve_context() {
    let mut e = fixture();
    let h = call(&mut e, Api::ImmCreateContext.id(), &[]);
    let value: Vec<u8> = [0u32, 0x80, 7, 9, 2, 3, 47, 23]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    put(&mut e, DATA, &value);
    assert_eq!(call(&mut e, Api::ImmSetCandidateWindow.id(), &[h, DATA]), 1);
    assert_eq!(
        call(&mut e, Api::ImmGetCandidateWindow.id(), &[h, 0, DATA + 64]),
        1
    );
    assert_eq!(word(&e, DATA + 68), 0x80);
    put(&mut e, DATA, &4u32.to_le_bytes());
    assert_eq!(call(&mut e, Api::ImmSetCandidateWindow.id(), &[h, DATA]), 0);
    assert_eq!(
        call(&mut e, Api::ImmGetCandidateWindow.id(), &[h, 0, DATA + 64]),
        1
    );
    assert_eq!(word(&e, DATA + 68), 0x80);
    let mut font = [0; 92];
    font[..4].copy_from_slice(&(-18i32).to_le_bytes());
    font[16..20].copy_from_slice(&400u32.to_le_bytes());
    font[23] = 1;
    for (output, unit) in font[28..]
        .chunks_exact_mut(2)
        .zip("test face".encode_utf16())
    {
        output.copy_from_slice(&unit.to_le_bytes());
    }
    put(&mut e, DATA + 128, &font);
    assert_eq!(
        call(&mut e, Api::ImmSetCompositionFontW.id(), &[h, DATA + 128]),
        1
    );
    assert_eq!(
        call(&mut e, Api::ImmGetCompositionFontA.id(), &[h, DATA + 256]),
        1
    );
    assert_eq!(word(&e, DATA + 256), (-18i32) as u32);
    let mut face = [0; 10];
    e.memory()
        .unwrap()
        .read(GuestAddress(DATA + 284), &mut face)
        .unwrap();
    assert_eq!(&face, b"test face\0");
}
