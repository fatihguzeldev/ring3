use super::*;
use crate::{
    input::{InputLimits, TimedInputEvent},
    process::{EngineInstance, MediaConfig},
};

#[test]
fn accepted_input_backpressure_keeps_independent_cursor_and_retry_delivers_once() {
    let (mut context, memory, call, window) = focused();
    for index in 0..6 {
        context
            .state
            .post(crate::windows::user32::Message32 {
                window,
                message: 0x400,
                wparam: index,
                thread_id: call.thread_id,
                ..Default::default()
            })
            .unwrap();
    }
    let mut engine = EngineInstance::new(16, 42).unwrap();
    engine.memory = Some(memory);
    engine.user32 = Some(context);
    engine.configure_cpu_clock(1_000_000, 0).unwrap();
    engine
        .configure_media(MediaConfig {
            input: InputLimits {
                queue_events: 1024,
                batch_events: 16,
            },
            ..Default::default()
        })
        .unwrap();
    let initial_sequence = engine.input().unwrap().source().tail().sequence;
    engine.observe_cpu_clock(4_000_000).unwrap();
    let epoch = engine.media_epoch().unwrap();
    let events = [
        InputKind::Focus { focused: true },
        InputKind::Key {
            code: 0x1e,
            pressed: true,
        },
        InputKind::Key {
            code: 0x30,
            pressed: true,
        },
        InputKind::Key {
            code: 0x2e,
            pressed: true,
        },
    ];
    let events = events
        .into_iter()
        .enumerate()
        .map(|(index, kind)| TimedInputEvent {
            timestamp_ns: (index as u64 + 1) * 1_000_000,
            kind,
        })
        .collect::<Vec<_>>();
    engine.submit_input(epoch, 1, &events).unwrap();
    assert!(engine.pending_user32_input());
    assert_eq!(
        engine.input().unwrap().source().tail().sequence,
        initial_sequence + 4
    );
    assert_eq!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .input
            .cursor
            .unwrap()
            .sequence,
        initial_sequence + 3
    );
    let first_cursor = engine.user32.as_ref().unwrap().input.cursor;
    assert!(engine.sync_windows_input().is_err());
    assert_eq!(engine.user32.as_ref().unwrap().input.cursor, first_cursor);
    let source_tail = engine.input().unwrap().source().tail();
    assert!(
        engine
            .submit_input(
                epoch,
                2,
                &[TimedInputEvent {
                    timestamp_ns: 4_000_000,
                    kind: InputKind::Key {
                        code: 0x20,
                        pressed: true
                    }
                }]
            )
            .is_err()
    );
    assert_eq!(engine.input().unwrap().source().tail(), source_tail);
    let context = engine.user32.as_mut().unwrap();
    let memory = engine.memory.as_mut().unwrap();
    query(
        context,
        memory,
        call,
        User32Api32::PeekMessageW,
        &[0x10200, window, 0, 0, 1],
    );
    engine.sync_windows_input().unwrap();
    assert!(!engine.pending_user32_input());
    let context = engine.user32.as_mut().unwrap();
    let memory = engine.memory.as_mut().unwrap();
    for expected in [65, 66, 67] {
        assert_eq!(
            query(
                context,
                memory,
                call,
                User32Api32::PeekMessageW,
                &[0x10200, window, 0x100, 0x100, 1]
            ),
            1
        );
        assert_eq!(
            super::super::super::memory::message(memory, 0x10200)
                .unwrap()
                .wparam,
            expected
        );
    }
    assert_eq!(
        query(
            context,
            memory,
            call,
            User32Api32::PeekMessageW,
            &[0x10200, window, 0x100, 0x100, 1]
        ),
        0
    );
}

#[test]
fn media_reset_discards_old_hardware_messages_preserves_posts_and_accepts_new_epoch() {
    let (mut context, memory, call, window) = focused();
    context
        .state
        .post(crate::windows::user32::Message32 {
            window,
            message: 0x400,
            thread_id: call.thread_id,
            ..Default::default()
        })
        .unwrap();
    key(&mut context, 1, 0x1e, true);
    let mut engine = EngineInstance::new(16, 43).unwrap();
    engine.memory = Some(memory);
    engine.user32 = Some(context);
    engine.configure_cpu_clock(1_000_000, 0).unwrap();
    engine.configure_media(MediaConfig::default()).unwrap();
    engine.sync_windows_input().unwrap();
    let old = engine.media_epoch().unwrap();
    let next = engine.reset_media().unwrap();
    assert!(next > old);
    assert!(!engine.pending_user32_input());
    let context = engine.user32.as_mut().unwrap();
    let memory = engine.memory.as_mut().unwrap();
    assert_eq!(
        query(
            context,
            memory,
            call,
            User32Api32::PeekMessageW,
            &[0x10200, window, 0x100, 0x100, 1]
        ),
        0
    );
    assert_eq!(
        query(
            context,
            memory,
            call,
            User32Api32::PeekMessageW,
            &[0x10200, window, 0x400, 0x400, 1]
        ),
        1
    );
    engine.observe_cpu_clock(1_000_000).unwrap();
    engine
        .submit_input(
            next,
            1,
            &[TimedInputEvent {
                timestamp_ns: 1_000_000,
                kind: InputKind::Focus { focused: true },
            }],
        )
        .unwrap();
    assert!(!engine.pending_user32_input());
}

#[test]
fn no_sync_media_reset_preflights_exclusive_cursor_revision_before_replacing_any_owner() {
    let (mut context, memory, call, window) = focused();
    context.input.mouse_exclusive = true;
    context.input.cursor_revision = u64::MAX;
    context
        .state
        .post(crate::windows::user32::Message32 {
            window,
            message: 0x400,
            thread_id: call.thread_id,
            ..Default::default()
        })
        .unwrap();
    let mut engine = EngineInstance::new(16, 44).unwrap();
    engine.memory = Some(memory);
    engine.user32 = Some(context);
    engine.configure_cpu_clock(1_000_000, 0).unwrap();
    engine.configure_media(MediaConfig::default()).unwrap();
    let epoch = engine.media_epoch().unwrap();
    let tail = engine.input().unwrap().source().tail();
    let queue = engine
        .user32
        .as_ref()
        .unwrap()
        .state
        .queue_status(call.thread_id);
    assert!(engine.synchronization.is_none());
    assert_eq!(
        engine.reset_media(),
        Err(crate::process::HostError::User32(User32Error32::Capacity))
    );
    assert_eq!(engine.media_epoch().unwrap(), epoch);
    assert_eq!(engine.input().unwrap().source().tail(), tail);
    let context = engine.user32.as_ref().unwrap();
    assert!(context.input.mouse_exclusive);
    assert_eq!(context.input.cursor_revision, u64::MAX);
    assert_eq!(context.state.queue_status(call.thread_id), queue);
}
