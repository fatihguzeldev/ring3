use super::fixtures::{EPOCH, event, focused, queued};
use ring3_engine::input::{InputError, InputKind, InputLimits, InputOrigin, RelativeTotals};

#[test]
fn press_and_release_in_one_batch_remain_two_ordered_events() {
    let mut context = focused(InputLimits::default());
    let events = [
        event(
            101,
            InputKind::Key {
                code: 42,
                pressed: true,
            },
        ),
        event(
            102,
            InputKind::Key {
                code: 42,
                pressed: false,
            },
        ),
    ];
    context.submit_batch(EPOCH, 2, &events).unwrap();
    assert!(!context.state().keys[42]);
    let records = queued(&context);
    assert_eq!(records.len(), 2);
    for (record, expected) in records.iter().zip(events) {
        assert_eq!(record.event, expected);
        assert_eq!(record.batch_sequence, 2);
        assert_eq!(record.origin, InputOrigin::Host);
    }
}

#[test]
fn physical_keys_buttons_and_relative_axes_have_independent_state() {
    let mut context = focused(InputLimits::default());
    context
        .submit_batch(
            EPOCH,
            2,
            &[
                event(
                    101,
                    InputKind::Key {
                        code: 160,
                        pressed: true,
                    },
                ),
                event(
                    101,
                    InputKind::Key {
                        code: 161,
                        pressed: true,
                    },
                ),
                event(
                    101,
                    InputKind::Button {
                        button: 7,
                        pressed: true,
                    },
                ),
                event(
                    102,
                    InputKind::Relative {
                        dx: 12,
                        dy: -9,
                        wheel: 120,
                    },
                ),
                event(
                    103,
                    InputKind::Relative {
                        dx: -4,
                        dy: 1,
                        wheel: -240,
                    },
                ),
            ],
        )
        .unwrap();
    assert!(context.state().keys[160]);
    assert!(context.state().keys[161]);
    assert!(context.state().buttons[7]);
    let relative = RelativeTotals {
        dx: 8,
        dy: -8,
        wheel: -120,
    };
    assert_eq!(context.state().relative, relative);
    let records = queued(&context);
    assert_eq!(context.drain_relative(), relative);
    assert_eq!(context.drain_relative(), RelativeTotals::default());
    assert_eq!(queued(&context), records);
    assert!(context.state().buttons[7]);
}

#[test]
fn a_late_invalid_event_does_not_publish_earlier_transitions() {
    let mut context = focused(InputLimits::default());
    let state = *context.state();
    assert_eq!(
        context.submit_batch(
            EPOCH,
            2,
            &[
                event(
                    101,
                    InputKind::Key {
                        code: 42,
                        pressed: true
                    }
                ),
                event(
                    102,
                    InputKind::Key {
                        code: 256,
                        pressed: true
                    }
                ),
            ]
        ),
        Err(InputError::InvalidEvent)
    );
    assert_eq!(*context.state(), state);
    assert_eq!(context.queued_events(), 0);
    assert_eq!(context.last_sequence(), 1);
    assert_eq!(context.last_timestamp_ns(), 100);
    assert_eq!(
        context.submit_batch(
            EPOCH,
            2,
            &[
                event(101, InputKind::Focus { focused: false }),
                event(
                    102,
                    InputKind::Button {
                        button: 0,
                        pressed: true
                    }
                ),
            ]
        ),
        Err(InputError::NotFocused)
    );
    assert_eq!(*context.state(), state);
    assert_eq!(context.queued_events(), 0);
    assert_eq!(context.last_sequence(), 1);
}

#[test]
fn batches_require_current_epoch_contiguous_sequence_and_monotonic_time() {
    let mut context = focused(InputLimits::default());
    let key = event(
        101,
        InputKind::Key {
            code: 42,
            pressed: true,
        },
    );
    for (epoch, sequence, timestamp, error) in [
        (EPOCH - 1, 2, 101, InputError::StaleSession),
        (EPOCH, 1, 101, InputError::SequenceOrder),
        (EPOCH, 3, 101, InputError::SequenceOrder),
        (EPOCH, 2, 99, InputError::TimeOrder),
    ] {
        let before = *context.state();
        assert_eq!(
            context.submit_batch(epoch, sequence, &[event(timestamp, key.kind)]),
            Err(error)
        );
        assert_eq!(*context.state(), before);
        assert_eq!(context.queued_events(), 0);
        assert_eq!(context.last_sequence(), 1);
    }
    assert_eq!(
        context.submit_batch(EPOCH, 2, &[key, event(100, key.kind)]),
        Err(InputError::TimeOrder)
    );
    assert_eq!(context.queued_events(), 0);
    context.submit_batch(EPOCH, 2, &[key, key]).unwrap();
    assert_eq!(context.queued_events(), 2);
    assert_eq!(context.last_sequence(), 2);
    assert_eq!(context.last_timestamp_ns(), 101);
}
