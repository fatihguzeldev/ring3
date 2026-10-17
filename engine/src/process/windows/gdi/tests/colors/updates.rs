use super::super::{memory, paint};
use crate::windows::{gdi::GdiApi32, user32::User32Api32 as Api, window::WindowHandle32};

const SET_COLORS: u32 = 0x0002_00cd;

#[test]
fn captured_system_colors_notify_real_top_level_windows_before_invalidating_visible_children() {
    let module = crate::windows::user32::user32_module(0x71000000);
    assert!(
        module
            .exports
            .iter()
            .any(|entry| { entry.name.as_deref() == Some("SetSysColors") })
    );
    let (mut engine, callback, parent) = paint::fixture_with_startup(true);
    let mut created = Vec::new();
    for (style, owner) in [(0x50000000, parent), (0x80000000, 0)] {
        paint::capture(
            &mut engine,
            Api::CreateWindowExA.id(),
            &[0, 0x12100, 0, style, 0, 0, 4, 3, owner, 0, 0x400000, 0],
        );
        while let Some(request) = engine.pending_windows_callback() {
            paint::finish(
                &mut engine,
                callback,
                u32::from(request.arguments[1] == 0x81),
            );
        }
        created.push(paint::eax(&engine));
    }
    let child = created[0];
    let hidden = created[1];
    for window in [parent, child, hidden] {
        paint::capture(&mut engine, GdiApi32::ValidateRect.id(), &[window, 0]);
    }
    paint::capture(&mut engine, GdiApi32::GetSysColorBrush.id(), &[15]);
    let brush = paint::eax(&engine);
    memory::write_words(engine.memory.as_mut().unwrap(), 0x12200, &[15, 15, 25]).unwrap();
    memory::write_words(
        engine.memory.as_mut().unwrap(),
        0x12220,
        &[0x112233, 0x445566, 0x778899],
    )
    .unwrap();
    for count in [3, 0] {
        paint::capture(&mut engine, SET_COLORS, &[count, 0x12200, 0x12220]);
        let mut notified = Vec::new();
        for _ in 0..32 {
            if let Some(request) = engine.pending_windows_callback() {
                assert_eq!(&request.arguments[1..4], &[0x15, 0, 0]);
                assert_eq!(engine.gdi.as_ref().unwrap().system_color(15), 0x445566);
                notified.push(request.arguments[0]);
                paint::finish(&mut engine, callback, 0);
            } else if let Some(wait) = engine.waiting_user32_message() {
                engine.resume_user32_wait(paint::KEY, wait).unwrap();
            } else {
                break;
            }
        }
        assert!(engine.pending_call.is_none());
        assert_eq!(notified, [parent, hidden]);
        assert_eq!(paint::eax(&engine), 1);
        assert_eq!(engine.windows_thread.last_error(), 0);
        for (window, expected) in [(parent, true), (child, true), (hidden, false)] {
            let region = engine
                .user32
                .as_ref()
                .unwrap()
                .gdi_update_region(WindowHandle32::try_from(window).unwrap())
                .unwrap()
                .0;
            assert_eq!(region.complexity() != 1, expected);
            paint::capture(&mut engine, GdiApi32::ValidateRect.id(), &[window, 0]);
        }
    }
    paint::capture(
        &mut engine,
        GdiApi32::GetObjectW.id(),
        &[brush, 12, 0x12280],
    );
    assert_eq!(
        memory::words::<3>(engine.memory.as_ref().unwrap(), 0x12280).unwrap(),
        [0, 0x445566, 0]
    );
}

#[test]
fn color_array_fault_preserves_profile_and_captured_call_until_mapping_repair() {
    use crate::{
        memory::GuestAddress,
        process::HostError,
        windows::{CallingConvention32, user32::User32Error32},
    };
    let (mut engine, callback, _) = paint::fixture_with_startup(true);
    memory::write_words(engine.memory.as_mut().unwrap(), 0x13ffc, &[15]).unwrap();
    memory::write_words(
        engine.memory.as_mut().unwrap(),
        0x12220,
        &[0x112233, 0x445566],
    )
    .unwrap();
    let pc = 0x103c0;
    let owner = paint::unit(&mut engine, &[pc, 2, pc, SET_COLORS], 1, 1);
    memory::write_words(
        engine.memory.as_mut().unwrap(),
        0x11f00,
        &[pc + 2, 2, 0x13ffc, 0x12220],
    )
    .unwrap();
    paint::stop(&mut engine, pc, SET_COLORS, 0);
    let token = engine
        .capture_resident_call(paint::KEY, owner, CallingConvention32::Stdcall, 3)
        .unwrap()
        .token;
    let frame = engine.pending_call.as_ref().unwrap().frame;
    let previous = engine.gdi.as_ref().unwrap().system_color(15);
    assert!(matches!(
        engine.complete_resident_windows_call(paint::KEY, owner, token),
        Err(HostError::User32(User32Error32::Memory(_)))
    ));
    assert_eq!(engine.pending_call.as_ref().unwrap().frame, frame);
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    assert_eq!(engine.gdi.as_ref().unwrap().system_color(15), previous);
    assert!(engine.pending_windows_callback().is_none());
    assert!(engine.waiting_user32_message().is_none());
    engine.map(0x14000, 1, 3).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x14000), &16u32.to_le_bytes())
        .unwrap();
    engine
        .complete_resident_windows_call(paint::KEY, owner, token)
        .unwrap();
    for _ in 0..16 {
        if engine.pending_windows_callback().is_some() {
            paint::finish(&mut engine, callback, 0);
        } else if let Some(wait) = engine.waiting_user32_message() {
            engine.resume_user32_wait(paint::KEY, wait).unwrap();
        } else {
            break;
        }
    }
    assert!(engine.pending_call.is_none());
    assert_eq!(paint::eax(&engine), 1);
    assert_eq!(engine.gdi.as_ref().unwrap().system_color(15), 0x112233);
    assert_eq!(engine.gdi.as_ref().unwrap().system_color(16), 0x445566);
}
