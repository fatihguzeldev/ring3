use super::*;

fn input() -> InputContext {
    InputContext::new(
        1,
        0,
        InputLimits {
            queue_events: MIN_QUEUE_EVENTS,
            batch_events: MAX_BATCH_EVENTS,
        },
    )
    .unwrap()
}
fn event(time: u64, kind: InputKind) -> TimedInputEvent {
    TimedInputEvent {
        timestamp_ns: time,
        kind,
    }
}
fn logical(input: &mut InputContext, focus: bool, capture: bool, time: u64) {
    let prepared = input.prepare_window_flags(focus, capture, time).unwrap();
    input.commit_window_flags(prepared);
}

#[test]
fn effective_flags_follow_physical_and_logical_conjunction_without_creating_pointer_lock() {
    for host_focus in [false, true] {
        for host_capture in [false, true] {
            for window_focus in [false, true] {
                for window_capture in [false, true] {
                    let mut input = input();
                    input
                        .submit_batch(
                            1,
                            1,
                            &[event(
                                1,
                                InputKind::Focus {
                                    focused: host_focus,
                                },
                            )],
                        )
                        .unwrap();
                    if host_capture && host_focus {
                        input
                            .submit_batch(1, 2, &[event(2, InputKind::Capture { captured: true })])
                            .unwrap();
                    }
                    logical(&mut input, window_focus, window_capture, 3);
                    assert_eq!(input.state().focused, host_focus && window_focus);
                    assert_eq!(
                        input.state().captured,
                        host_focus && host_capture && window_focus && window_capture
                    );
                }
            }
        }
    }
}

#[test]
fn logical_loss_releases_keys_invalidates_leases_and_does_not_consume_host_sequence() {
    let mut input = input();
    input
        .submit_batch(
            1,
            1,
            &[
                event(1, InputKind::Focus { focused: true }),
                event(2, InputKind::Capture { captured: true }),
                event(
                    3,
                    InputKind::Key {
                        code: 7,
                        pressed: true,
                    },
                ),
            ],
        )
        .unwrap();
    logical(&mut input, true, true, 3);
    let lease = input.acquire(true).unwrap();
    input.consume(input.queued_events()).unwrap();
    logical(&mut input, false, true, 4);
    assert_eq!(input.last_sequence(), 1);
    assert!(!input.state().keys[7]);
    assert!(!input.state().captured);
    assert!(!input.lease_valid(lease));
    let mut records = [InputRecord::default(); 2];
    assert_eq!(input.peek(2, &mut records).unwrap(), 2);
    assert_eq!(
        records[0].event.kind,
        InputKind::Key {
            code: 7,
            pressed: false
        }
    );
    assert_eq!(records[0].origin, InputOrigin::FocusRelease);
    assert_eq!(records[1].event.kind, InputKind::Focus { focused: false });
    assert_eq!(records[1].origin, InputOrigin::Window);
    assert!(records.iter().all(|record| record.batch_sequence == 1));
    logical(&mut input, true, true, 5);
    assert!(input.state().focused && input.state().captured);
    assert!(!input.lease_valid(lease));
    input
        .submit_batch(1, 2, &[event(6, InputKind::Focus { focused: false })])
        .unwrap();
    logical(&mut input, false, false, 7);
    logical(&mut input, true, true, 8);
    assert!(!input.state().focused && !input.state().captured);
    input
        .submit_batch(1, 3, &[event(9, InputKind::Focus { focused: true })])
        .unwrap();
    assert!(input.state().focused);
    assert!(!input.state().captured);
}

#[test]
fn full_queue_rejects_logical_transition_before_any_state_or_generation_publication() {
    let mut input = input();
    let mut events = vec![event(1, InputKind::Focus { focused: true })];
    for code in 0..KEY_COUNT {
        events.push(event(
            1,
            InputKind::Key {
                code: code as u16,
                pressed: true,
            },
        ));
    }
    for button in 0..BUTTON_COUNT {
        events.push(event(
            1,
            InputKind::Button {
                button: button as u8,
                pressed: true,
            },
        ));
    }
    input.submit_batch(1, 1, &events).unwrap();
    let state = *input.state();
    let physical = input.physical;
    let generations = (input.focus_generation, input.capture_generation);
    assert!(matches!(
        input.prepare_window_flags(false, false, 2),
        Err(InputError::QueueFull)
    ));
    assert_eq!(*input.state(), state);
    assert_eq!(input.physical, physical);
    assert_eq!(
        (input.focus_generation, input.capture_generation),
        generations
    );
    assert_eq!(input.window_flags, None);
    assert_eq!(input.last_sequence(), 1);
    assert_eq!(input.last_timestamp_ns(), 1);
}

#[test]
fn hidden_physical_events_advance_host_clock_and_sequence_without_reactivating_guest_keys() {
    let mut input = input();
    logical(&mut input, false, false, 0);
    input
        .submit_batch(
            1,
            1,
            &[
                event(1, InputKind::Focus { focused: true }),
                event(
                    2,
                    InputKind::Key {
                        code: 7,
                        pressed: true,
                    },
                ),
                event(
                    3,
                    InputKind::Relative {
                        dx: 10,
                        dy: 20,
                        wheel: 1,
                    },
                ),
            ],
        )
        .unwrap();
    assert_eq!(input.last_sequence(), 1);
    assert_eq!(input.last_timestamp_ns(), 3);
    assert_eq!(input.queued_events(), 1);
    assert!(!input.state().focused);
    assert!(!input.state().keys[7]);
    assert_eq!(input.state().relative, RelativeTotals::default());
    logical(&mut input, true, false, 4);
    assert!(input.state().focused);
    assert!(!input.state().keys[7]);
    assert!(!input.state().captured);
    input
        .submit_batch(
            1,
            2,
            &[event(
                5,
                InputKind::Key {
                    code: 7,
                    pressed: true,
                },
            )],
        )
        .unwrap();
    assert!(input.state().keys[7]);
}

#[test]
fn window_origin_has_literal_wire_value_two_and_preserves_existing_record_shape() {
    let mut input = input();
    input
        .submit_batch(1, 1, &[event(1, InputKind::Focus { focused: true })])
        .unwrap();
    input.consume(1).unwrap();
    logical(&mut input, false, false, 2);
    let mut bytes = [0xaa; 80];
    let read = input.peek_encoded(1, &mut bytes).unwrap();
    assert_eq!(read.bytes, 80);
    assert_eq!(&bytes[..8], b"R3IQ\x01\x00\x30\x00");
    assert_eq!(&bytes[16..20], &1u32.to_le_bytes());
    assert_eq!(&bytes[40..44], &4u32.to_le_bytes());
    assert_eq!(&bytes[72..76], &2u32.to_le_bytes());
    assert_eq!(&bytes[76..80], &[0; 4]);
}

#[test]
#[should_panic(expected = "prepared window input owner changed")]
fn prepared_receipt_rejects_changed_queue_owner_before_commit() {
    let mut input = input();
    input
        .submit_batch(1, 1, &[event(1, InputKind::Focus { focused: true })])
        .unwrap();
    let prepared = input.prepare_window_flags(false, false, 2).unwrap();
    input.consume(1).unwrap();
    input.commit_window_flags(prepared);
}
