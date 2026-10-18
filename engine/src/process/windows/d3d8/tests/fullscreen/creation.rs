use super::support::*;
use crate::{graphics::d3d8::api::HResult, windows::user32::Rect32};

#[test]
fn fullscreen_create_shows_and_resizes_hidden_popup_before_returning_original_hresult_once() {
    let mut fixture = Fixture::new(0x8000_0000);
    let window = fixture.window;
    let before = fixture.engine.user32_window_info(window).unwrap().unwrap();
    assert!(!before.visible);
    assert_eq!(
        (before.client_extent.width, before.client_extent.height),
        (0, 0)
    );
    fixture.parameters(window, 640, 480, false);
    let devices = fixture.engine.d3d8.as_ref().unwrap().device_slots.len();
    let (owner, token) = fixture.create_device(window);
    assert_eq!(
        fixture.engine.pending_windows_callback().unwrap().arguments[1],
        0x46
    );
    let device = words(&fixture.engine, OUT, 1)[0];
    assert_ne!(device, 0);
    assert_eq!(
        fixture.engine.d3d8.as_ref().unwrap().device_slots.len(),
        devices + 1
    );
    fixture.assert_pending_retry(owner, token);
    assert_eq!(words(&fixture.engine, OUT, 1), [device]);
    assert_eq!(
        fixture.engine.d3d8.as_ref().unwrap().device_slots.len(),
        devices + 1
    );
    assert_eq!(fixture.drain(|_, _| {}), [0x46, 0x83, 0x47]);
    fixture.assert_return(7);
    let window = fixture.engine.user32_window_info(window).unwrap().unwrap();
    assert_eq!(
        window.outer,
        Rect32 {
            left: 0,
            top: 0,
            right: 640,
            bottom: 480
        }
    );
    assert_eq!(
        (window.client_extent.width, window.client_extent.height),
        (640, 480)
    );
    assert!(window.visible);
}

#[test]
fn fullscreen_device_window_is_distinct_from_focus_window_and_uses_guest_rectangle_edits() {
    let mut fixture = Fixture::new(0x9000_0000);
    let focus = fixture.window;
    let target = fixture.create_window(0x8000_0000, 0, 0, 0, 0);
    let focus_before = fixture.engine.user32_window_info(focus).unwrap();
    fixture.parameters(target, 640, 480, false);
    fixture.create_device(focus);
    let messages = fixture.drain(|engine, arguments| {
        assert_eq!(arguments[0], target);
        match arguments[1] {
            0x46 => write(engine, arguments[3] + 8, &[10, 20, 600, 400]),
            0x83 => write(engine, arguments[3], &[13, 24, 607, 416]),
            _ => {}
        }
    });
    assert_eq!(messages, [0x46, 0x83, 0x47]);
    fixture.assert_return(7);
    assert_eq!(
        fixture.engine.user32_window_info(focus).unwrap(),
        focus_before
    );
    let info = fixture.engine.user32_window_info(target).unwrap().unwrap();
    assert_eq!(
        info.outer,
        Rect32 {
            left: 10,
            top: 20,
            right: 610,
            bottom: 420
        }
    );
    assert_eq!((info.client_origin.x, info.client_origin.y), (13, 24));
    assert_eq!(
        (info.client_extent.width, info.client_extent.height),
        (594, 392)
    );
    assert!(info.visible);
}

#[test]
fn decorated_fullscreen_window_keeps_default_nonclient_calculation_in_guest_callback_chain() {
    let mut fixture = Fixture::new(0x10cf_0000);
    let window = fixture.window;
    fixture.parameters(window, 640, 480, false);
    fixture.create_device(window);
    assert_eq!(fixture.drain(|_, _| {}), [0x46, 0x83, 0x47]);
    fixture.assert_return(7);
    let info = fixture.engine.user32_window_info(window).unwrap().unwrap();
    assert_eq!(
        info.style & 0x8000_0000,
        0,
        "fullscreen never fabricates WS_POPUP"
    );
    assert_eq!((info.outer.right, info.outer.bottom), (640, 480));
    assert_eq!((info.client_origin.x, info.client_origin.y), (8, 31));
    assert_eq!(
        (info.client_extent.width, info.client_extent.height),
        (624, 441)
    );
}

#[test]
fn foreign_thread_device_window_keeps_preexisting_d3d_admission_without_local_geometry_dispatch() {
    let mut fixture = Fixture::new(0x9000_0000);
    let target = fixture.foreign_window();
    let before = fixture.engine.user32_window_info(target).unwrap().unwrap();
    assert_eq!(before.thread, 2);
    fixture.parameters(target, 640, 480, false);
    fixture.create_device(fixture.window);
    fixture.assert_return(7);
    assert_ne!(words(&fixture.engine, OUT, 1)[0], 0);
    assert!(fixture.engine.pending_windows_callback().is_none());
    assert_eq!(
        fixture.engine.user32_window_info(target).unwrap(),
        Some(before)
    );
    assert_eq!(
        fixture
            .engine
            .user32
            .as_ref()
            .unwrap()
            .thread_scratch_ranges()
            .count(),
        0
    );
}

#[test]
fn rejected_and_windowed_create_leave_canonical_geometry_without_position_callbacks() {
    let mut fixture = Fixture::new(0x9000_0000);
    let window = fixture.window;
    let before = fixture.engine.user32_window_info(window).unwrap();
    fixture.parameters(window, 639, 480, false);
    let devices = fixture.engine.d3d8.as_ref().unwrap().device_slots.len();
    fixture.create_device(window);
    assert_eq!(
        state(&fixture.engine).registers[0],
        HResult::NOT_AVAILABLE.0
    );
    assert!(fixture.engine.pending_windows_callback().is_none());
    assert_eq!(fixture.engine.user32_window_info(window).unwrap(), before);
    assert_eq!(
        fixture.engine.d3d8.as_ref().unwrap().device_slots.len(),
        devices
    );
    fixture.parameters(window, 640, 480, false);
    write(&mut fixture.engine, OUT, &[0xcccc_cccc]);
    let pages = fixture.engine.memory().unwrap().mapped_pages();
    fixture.call(0x4010f, &[fixture.parent.0, 0, 1, window, 0, PP, OUT]);
    assert_eq!(state(&fixture.engine).registers[0], HResult::INVALID_CALL.0);
    assert_eq!(words(&fixture.engine, OUT, 1), [0xcccc_cccc]);
    assert_eq!(fixture.engine.memory().unwrap().mapped_pages(), pages);
    assert_eq!(fixture.engine.user32_window_info(window).unwrap(), before);
    assert_eq!(
        fixture.engine.d3d8.as_ref().unwrap().device_slots.len(),
        devices
    );
    assert!(fixture.engine.pending_windows_callback().is_none());
    assert!(fixture.engine.d3d8_reply.is_none());
    assert_eq!(
        fixture
            .engine
            .user32
            .as_ref()
            .unwrap()
            .thread_scratch_ranges()
            .count(),
        0
    );
    fixture.parameters(window, 640, 480, true);
    write(&mut fixture.engine, PP + 44, &[0]);
    write(&mut fixture.engine, PP + 48, &[0]);
    fixture.create_device(window);
    fixture.assert_return(7);
    assert_ne!(words(&fixture.engine, OUT, 1)[0], 0);
    assert!(fixture.engine.pending_windows_callback().is_none());
    assert_eq!(fixture.engine.user32_window_info(window).unwrap(), before);
}
