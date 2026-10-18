use super::*;
use crate::{
    input::{InputKind, TimedInputEvent},
    process::{EngineInstance, MediaConfig},
    windows::synchronization::{
        INFINITE, MessageWait32, SynchronizationLimits32, WaitMode32, WaitOutcome32,
    },
};

#[test]
fn physical_activation_projection_preserves_due_window_timer_flags() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let window = create(&mut context, &mut memory, call, atom);
    context.input.physical_focus = true;
    let outcome = context
        .prepare_arguments(User32Api32::ShowWindow, &[window, 5], &mut memory, call)
        .unwrap();
    drain_show(&mut context, &mut memory, outcome);
    context
        .prepare_arguments(
            User32Api32::SetTimer,
            &[window, 1, 10, 0],
            &mut memory,
            call,
        )
        .unwrap();
    let mut engine = EngineInstance::new(16, 43).unwrap();
    engine.memory = Some(memory);
    engine.user32 = Some(context);
    engine.configure_cpu_clock(1_000_000, 0).unwrap();
    engine.configure_media(MediaConfig::default()).unwrap();
    engine
        .configure_synchronization(SynchronizationLimits32::default())
        .unwrap();
    engine.sync_windows_input().unwrap();
    engine.observe_cpu_clock(11_000_000).unwrap();
    assert_eq!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .wait_queue_status(call.thread_id)
            .0
            & 0x50,
        0x10
    );
    engine
        .submit_input(
            engine.media_epoch().unwrap(),
            1,
            &[TimedInputEvent {
                timestamp_ns: 11_000_000,
                kind: InputKind::Focus { focused: false },
            }],
        )
        .unwrap();
    let context = engine.user32.as_ref().unwrap();
    let actual = context.wait_queue_status(call.thread_id);
    let projected = engine
        .synchronization()
        .unwrap()
        .queue_state(call.thread_id);
    assert_eq!(actual.0 & 0x50, 0x50);
    assert_eq!(actual.1 & 0x50, 0x50);
    assert_eq!(projected.0 & 0x50, 0x50);
    assert_eq!(projected.1 & 0x50, 0x50);
}

#[test]
fn show_projects_sent_wake_to_previous_gui_thread_without_running_its_window_procedure() {
    let (mut context, mut memory, mut old_call) = fixture();
    old_call.thread_id = 2;
    let atom = register(&mut context, &mut memory, old_call);
    let old_active = create(&mut context, &mut memory, old_call, atom);
    let old_remaining = create(&mut context, &mut memory, old_call, atom);
    context.input.physical_focus = true;
    let outcome = context
        .prepare_arguments(
            User32Api32::ShowWindow,
            &[old_active, 5],
            &mut memory,
            old_call,
        )
        .unwrap();
    drain_show(&mut context, &mut memory, outcome);
    let outcome = context
        .prepare_arguments(
            User32Api32::DestroyWindow,
            &[old_active],
            &mut memory,
            old_call,
        )
        .unwrap();
    drain_show(&mut context, &mut memory, outcome);
    assert_eq!(context.windows().active(), None);
    let call = User32CallContext32 {
        thread_id: 1,
        ..old_call
    };
    let window = create(&mut context, &mut memory, call, atom);
    let mut engine = EngineInstance::new(16, 42).unwrap();
    engine.memory = Some(memory);
    engine.user32 = Some(context);
    engine.configure_cpu_clock(1_000_000, 0).unwrap();
    engine
        .configure_synchronization(SynchronizationLimits32::default())
        .unwrap();
    let sync = engine.synchronization_mut().unwrap();
    sync.set_current_thread(old_call.thread_id).unwrap();
    let WaitOutcome32::Pending(wait) = sync
        .wait(
            &[],
            WaitMode32::Any,
            INFINITE,
            false,
            Some(MessageWait32 {
                mask: 0x40,
                input_available: false,
            }),
        )
        .unwrap()
    else {
        panic!("old GUI thread starts without pending sent notifications")
    };
    sync.set_current_thread(call.thread_id).unwrap();
    let queue = engine.prepare_user32_activation_queues().unwrap();
    let context = engine.user32.as_mut().unwrap();
    let memory = engine.memory.as_mut().unwrap();
    let outcome = context
        .prepare_arguments(User32Api32::ShowWindow, &[window, 5], memory, call)
        .unwrap();
    let (_, messages) = drain_show(context, memory, outcome);
    assert!(messages.contains(&[window, 0x1c, 1, old_call.thread_id]));
    assert!(messages.iter().all(|message| message[0] != old_remaining));
    assert_eq!(context.wait_queue_status(old_call.thread_id).0 & 0x40, 0x40);
    engine.commit_user32_queue_change(queue);
    assert!(
        engine
            .synchronization()
            .unwrap()
            .wait_status(wait)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        engine
            .synchronization()
            .unwrap()
            .queue_state(old_call.thread_id)
            .0
            & 0x40,
        0x40
    );
}
