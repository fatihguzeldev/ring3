use super::fixtures::{EPOCH, event, focused, queued};
use ring3_engine::input::{
    InputError, InputKind, InputLimits, InputOrigin, InputState, RelativeTotals,
};

#[test]
fn capture_loss_preserves_physical_state_and_invalidates_only_capture_leases() {
    let mut context = focused(InputLimits::default());
    context
        .submit_batch(
            EPOCH,
            2,
            &[
                event(101, InputKind::Capture { captured: true }),
                event(
                    101,
                    InputKind::Key {
                        code: 42,
                        pressed: true,
                    },
                ),
                event(
                    101,
                    InputKind::Button {
                        button: 0,
                        pressed: true,
                    },
                ),
                event(
                    101,
                    InputKind::Relative {
                        dx: 3,
                        dy: 4,
                        wheel: 0,
                    },
                ),
            ],
        )
        .unwrap();
    let keyboard = context.acquire(false).unwrap();
    let captured = context.acquire(true).unwrap();
    context
        .submit_batch(
            EPOCH,
            3,
            &[event(102, InputKind::Capture { captured: false })],
        )
        .unwrap();
    assert!(context.lease_valid(keyboard));
    assert!(!context.lease_valid(captured));
    assert!(context.state().keys[42]);
    assert!(context.state().buttons[0]);
    assert_eq!(
        context.state().relative,
        RelativeTotals {
            dx: 3,
            dy: 4,
            wheel: 0
        }
    );
    assert_eq!(context.acquire(true), Err(InputError::NotCaptured));
    context
        .submit_batch(
            EPOCH,
            4,
            &[event(103, InputKind::Capture { captured: true })],
        )
        .unwrap();
    assert!(!context.lease_valid(captured));
    assert!(context.lease_valid(context.acquire(true).unwrap()));
}

#[test]
fn blur_releases_pressed_state_and_focus_return_cannot_revive_a_lease() {
    let mut context = focused(InputLimits::default());
    context
        .submit_batch(
            EPOCH,
            2,
            &[
                event(101, InputKind::Capture { captured: true }),
                event(
                    101,
                    InputKind::Key {
                        code: 200,
                        pressed: true,
                    },
                ),
                event(
                    101,
                    InputKind::Key {
                        code: 2,
                        pressed: true,
                    },
                ),
                event(
                    101,
                    InputKind::Button {
                        button: 1,
                        pressed: true,
                    },
                ),
                event(
                    101,
                    InputKind::Relative {
                        dx: 5,
                        dy: 6,
                        wheel: 7,
                    },
                ),
            ],
        )
        .unwrap();
    let lease = context.acquire(false).unwrap();
    context.consume(context.queued_events()).unwrap();
    context
        .submit_batch(EPOCH, 3, &[event(102, InputKind::Focus { focused: false })])
        .unwrap();
    assert_eq!(*context.state(), InputState::default());
    assert!(!context.lease_valid(lease));
    assert_eq!(context.acquire(false), Err(InputError::NotFocused));
    let records = queued(&context);
    assert_eq!(records.len(), 4);
    assert_eq!(
        records[0].event.kind,
        InputKind::Key {
            code: 2,
            pressed: false
        }
    );
    assert_eq!(
        records[1].event.kind,
        InputKind::Key {
            code: 200,
            pressed: false
        }
    );
    assert_eq!(
        records[2].event.kind,
        InputKind::Button {
            button: 1,
            pressed: false
        }
    );
    assert_eq!(records[3].event.kind, InputKind::Focus { focused: false });
    for (index, record) in records.iter().enumerate() {
        assert_eq!(record.event.timestamp_ns, 102);
        assert_eq!(record.batch_sequence, 3);
        assert_eq!(
            record.origin,
            if index == 3 {
                InputOrigin::Host
            } else {
                InputOrigin::FocusRelease
            }
        );
    }
    context
        .submit_batch(EPOCH, 4, &[event(103, InputKind::Focus { focused: true })])
        .unwrap();
    assert!(!context.lease_valid(lease));
    assert!(context.lease_valid(context.acquire(false).unwrap()));
    assert_eq!(context.acquire(true), Err(InputError::NotCaptured));
}

#[test]
fn reset_changes_session_identity_and_keeps_a_monotonic_admission_floor() {
    let mut context = focused(InputLimits::default());
    let lease = context.acquire(false).unwrap();
    context
        .submit_batch(
            EPOCH,
            2,
            &[event(
                110,
                InputKind::Key {
                    code: 42,
                    pressed: true,
                },
            )],
        )
        .unwrap();
    let before = *context.state();
    let records = queued(&context);
    assert_eq!(context.reset(EPOCH, 120), Err(InputError::StaleSession));
    assert_eq!(context.reset(EPOCH + 1, 109), Err(InputError::TimeOrder));
    assert_eq!(*context.state(), before);
    assert_eq!(queued(&context), records);
    assert!(context.lease_valid(lease));
    context.reset(EPOCH + 1, 120).unwrap();
    assert_eq!(*context.state(), InputState::default());
    assert_eq!(context.queued_events(), 0);
    assert_eq!(context.last_sequence(), 0);
    assert!(!context.lease_valid(lease));
    let focus = InputKind::Focus { focused: true };
    assert_eq!(
        context.submit_batch(EPOCH, 3, &[event(120, focus)]),
        Err(InputError::StaleSession)
    );
    assert_eq!(
        context.submit_batch(EPOCH + 1, 1, &[event(119, focus)]),
        Err(InputError::TimeOrder)
    );
    context
        .submit_batch(EPOCH + 1, 1, &[event(120, focus)])
        .unwrap();
    assert_eq!(context.last_timestamp_ns(), 120);
}
