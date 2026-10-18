use super::support::*;
use crate::{
    graphics::d3d8::api::HResult,
    memory::GuestAddress,
    process::{CallError, HostError, windows::d3d8::runtime::ReplyKind32},
};

#[test]
fn fullscreen_reset_uses_position_callbacks_once_before_the_original_success_return() {
    let mut fixture = Fixture::new(0x9000_0000);
    let window = fixture.window;
    fixture.parameters(window, 640, 480, false);
    fixture.create_device(window);
    fixture.drain(|_, _| {});
    let device = words(&fixture.engine, OUT, 1)[0];
    fixture.drain_batches(device);
    let previous_generation = fixture
        .engine
        .d3d8
        .as_ref()
        .unwrap()
        .device(GuestAddress(device))
        .unwrap()
        .generation();
    fixture.parameters(window, 800, 600, false);
    let (owner, token) = fixture.reset(device);
    assert_eq!(
        fixture.engine.pending_windows_callback().unwrap().arguments[1],
        0x46
    );
    let generation = fixture
        .engine
        .d3d8
        .as_ref()
        .unwrap()
        .device(GuestAddress(device))
        .unwrap()
        .generation();
    assert_eq!(generation, previous_generation + 1);
    fixture.assert_pending_retry(owner, token);
    assert_eq!(
        fixture
            .engine
            .d3d8
            .as_ref()
            .unwrap()
            .device(GuestAddress(device))
            .unwrap()
            .generation(),
        generation
    );
    assert_eq!(fixture.drain(|_, _| {}), [0x46, 0x83, 0x47]);
    fixture.assert_return(2);
    let info = fixture.engine.user32_window_info(window).unwrap().unwrap();
    assert_eq!(
        (
            info.outer.left,
            info.outer.top,
            info.outer.right,
            info.outer.bottom
        ),
        (0, 0, 800, 600)
    );
    assert_eq!(
        (info.client_extent.width, info.client_extent.height),
        (800, 600)
    );
}

#[test]
fn rejected_and_windowed_reset_do_not_invoke_the_fullscreen_position_chain() {
    let mut fixture = Fixture::new(0x9000_0000);
    let window = fixture.window;
    fixture.parameters(window, 640, 480, false);
    fixture.create_device(window);
    fixture.drain(|_, _| {});
    let device = words(&fixture.engine, OUT, 1)[0];
    fixture.drain_batches(device);
    let before = fixture.engine.user32_window_info(window).unwrap();
    fixture.parameters(window, 639, 480, false);
    fixture.reset(device);
    assert_eq!(
        state(&fixture.engine).registers[0],
        HResult::NOT_AVAILABLE.0
    );
    assert!(fixture.engine.pending_windows_callback().is_none());
    assert_eq!(fixture.engine.user32_window_info(window).unwrap(), before);
    fixture.parameters(window, 800, 600, true);
    write(&mut fixture.engine, PP + 44, &[0]);
    write(&mut fixture.engine, PP + 48, &[0]);
    fixture.reset(device);
    fixture.assert_return(2);
    assert!(fixture.engine.pending_windows_callback().is_none());
    assert_eq!(fixture.engine.user32_window_info(window).unwrap(), before);
}

#[test]
fn accepted_reset_drains_packets_before_position_callbacks_without_replaying_backend_mutation() {
    let mut fixture = Fixture::new(0x9000_0000);
    fixture
        .engine
        .d3d8
        .as_mut()
        .unwrap()
        .config
        .device
        .commands
        .batch_bytes = 128;
    let window = fixture.window;
    fixture.parameters(window, 640, 480, false);
    fixture.create_device(window);
    fixture.drain(|_, _| {});
    let device = words(&fixture.engine, OUT, 1)[0];
    fixture.drain_batches(device);
    let old_generation = fixture
        .engine
        .d3d8
        .as_ref()
        .unwrap()
        .device(GuestAddress(device))
        .unwrap()
        .generation();
    fixture.parameters(window, 800, 600, false);
    let (owner, token) = fixture.capture(0x4020e, &[device, PP]);
    assert_eq!(
        fixture
            .engine
            .complete_resident_windows_call(KEY, owner, token),
        Err(HostError::Call(CallError::Busy))
    );
    assert!(matches!(
        fixture.engine.d3d8_reply.unwrap().kind,
        ReplyKind32::Drain(0)
    ));
    assert!(fixture.engine.d3d8_reply.unwrap().window.is_some());
    assert!(fixture.engine.pending_windows_callback().is_none());
    assert_eq!(
        fixture
            .engine
            .d3d8
            .as_ref()
            .unwrap()
            .device(GuestAddress(device))
            .unwrap()
            .generation(),
        old_generation + 1
    );
    let info = fixture.engine.user32_window_info(window).unwrap().unwrap();
    assert_eq!(
        (info.client_extent.width, info.client_extent.height),
        (640, 480)
    );
    assert_eq!(
        fixture
            .engine
            .complete_resident_windows_call(KEY, owner, token),
        Err(HostError::Call(CallError::Busy))
    );
    fixture.drain_batches(device);
    fixture
        .engine
        .complete_resident_windows_call(KEY, owner, token)
        .unwrap();
    assert_eq!(
        fixture
            .engine
            .d3d8
            .as_ref()
            .unwrap()
            .device(GuestAddress(device))
            .unwrap()
            .generation(),
        old_generation + 1
    );
    assert_eq!(fixture.drain(|_, _| {}), [0x46, 0x83, 0x47]);
    fixture.assert_return(2);
    let info = fixture.engine.user32_window_info(window).unwrap().unwrap();
    assert_eq!(
        (info.client_extent.width, info.client_extent.height),
        (800, 600)
    );
}
