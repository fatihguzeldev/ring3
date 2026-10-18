use super::*;
use crate::windows::{
    synchronization::SynchronizationLimits32,
    user32::{Message32, QueuedKey32},
};

#[test]
fn suppressed_keyboard_callback_refreshes_the_engine_message_wait_projection() {
    let (mut engine, _, callback, _) = engine();
    engine
        .configure_synchronization(SynchronizationLimits32::default())
        .unwrap();
    let call = User32CallContext32 {
        thread_id: 1,
        ..fixture().2
    };
    let context = engine.user32.as_mut().unwrap();
    assert!(matches!(
        context
            .prepare_arguments(
                User32Api32::SetWindowsHookExA,
                &[2, CALLBACK_PC, 0, 1],
                engine.memory.as_mut().unwrap(),
                call,
            )
            .unwrap(),
        User32Outcome32::Return(value) if value != 0
    ));
    let mut state = [0; 256];
    state[0x28] = 0x80;
    context
        .state
        .post_input(
            Message32 {
                message: 0x100,
                wparam: 0x28,
                lparam: 1,
                thread_id: 1,
                ..Default::default()
            },
            Some(QueuedKey32 {
                virtual_key: 0x28,
                pressed: true,
                repeated: false,
            }),
            state,
        )
        .unwrap();
    let call_pc = CALL_PC + 0x30;
    let outer = unit(
        &mut engine,
        &[call_pc, 2, call_pc, User32Api32::PeekMessageA.id()],
        1,
        1,
    );
    let mut stack = [0; 24];
    for (index, value) in [0x11000u32, 0, 0x100, 0x109, 0].into_iter().enumerate() {
        stack[4 + index * 4..8 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(STACK), &stack)
        .unwrap();
    stop(
        &mut engine,
        call_pc,
        User32Api32::PeekMessageA.id(),
        STACK,
        0,
    );
    let token = engine
        .capture_resident_call(KEY, outer, crate::windows::CallingConvention32::Stdcall, 5)
        .unwrap()
        .token;
    engine
        .complete_resident_windows_call(KEY, outer, token)
        .unwrap();
    assert_eq!(engine.synchronization().unwrap().queue_state(1).0 & 1, 1);
    let request = engine.pending_windows_callback().unwrap();
    assert_eq!(request.arguments[..3], [3, 0x28, 1]);
    let record = engine
        .begin_windows_resident_callback(KEY, callback, RETURN_PC, RETURN_ID)
        .unwrap();
    engine
        .authorize_resident_callback(KEY, callback, record.token)
        .unwrap();
    stop(&mut engine, RETURN_PC, RETURN_ID, STACK, 1);
    engine
        .finish_windows_resident_callback(KEY, callback, record.token)
        .unwrap();
    assert_eq!(
        engine.user32.as_ref().unwrap().wait_queue_status(1).0 & 1,
        0
    );
    assert_eq!(engine.synchronization().unwrap().queue_state(1).0 & 1, 0);
    assert_eq!(engine.pending_windows_callback(), None);
}
