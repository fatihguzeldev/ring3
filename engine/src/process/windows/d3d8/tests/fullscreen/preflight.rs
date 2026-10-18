use super::support::*;
use crate::{
    cpu::x86::State32,
    graphics::d3d8::api::HResult,
    process::windows::user32::{User32CallContext32, User32Outcome32},
    windows::{CallFrame32, CallingConvention32, user32::User32Api32},
};

#[test]
fn fullscreen_position_capacity_returns_hresult_before_device_or_guest_output_publication() {
    let mut fixture = Fixture::new(0x8000_0000);
    let window = fixture.window;
    fixture.parameters(window, 640, 480, false);
    let mut continuations = Vec::new();
    for _ in 0..16 {
        write(
            &mut fixture.engine,
            0x8e00,
            &[0x9f00, window, 0, 0, 0, 1, 1, 0x14],
        );
        let mut state = State32::default();
        state.registers[4] = 0x8e00;
        let frame = CallFrame32::capture(
            fixture.engine.memory().unwrap(),
            state,
            CallingConvention32::Stdcall,
            7,
        )
        .unwrap();
        let engine = &mut fixture.engine;
        let outcome = engine
            .user32
            .as_mut()
            .unwrap()
            .prepare(
                User32Api32::SetWindowPos,
                &frame,
                engine.memory.as_mut().unwrap(),
                User32CallContext32 {
                    thread_id: 1,
                    ..Default::default()
                },
            )
            .unwrap();
        let User32Outcome32::Callback(request) = outcome else {
            panic!("position capacity fixture owns a pending callback")
        };
        continuations.push(request.continuation);
    }
    write(&mut fixture.engine, OUT, &[0x12345678]);
    let before = fixture.engine.user32_window_info(window).unwrap();
    let pages = fixture.engine.memory().unwrap().mapped_pages();
    let devices = fixture.engine.d3d8.as_ref().unwrap().device_slots.len();
    fixture.create_device(window);
    assert_eq!(
        state(&fixture.engine).registers[0],
        HResult::OUT_OF_MEMORY.0
    );
    assert_eq!(words(&fixture.engine, OUT, 1), [0x12345678]);
    assert_eq!(fixture.engine.user32_window_info(window).unwrap(), before);
    assert_eq!(fixture.engine.memory().unwrap().mapped_pages(), pages);
    assert_eq!(
        fixture.engine.d3d8.as_ref().unwrap().device_slots.len(),
        devices
    );
    assert!(fixture.engine.pending_windows_callback().is_none());
    assert!(fixture.engine.d3d8_reply.is_none());
    for continuation in continuations.into_iter().rev() {
        let engine = &mut fixture.engine;
        engine
            .user32
            .as_mut()
            .unwrap()
            .cancel_presentation_position(continuation, engine.memory.as_mut().unwrap())
            .unwrap();
    }
}
