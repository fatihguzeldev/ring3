use super::fixtures::{CLOCK_EPOCH, EPOCH, event, focused, queued};
use ring3_engine::input::{
    InputContext, InputError, InputKind, InputLimits, InputRecord, MAX_BATCH_EVENTS,
    MAX_QUEUE_EVENTS, MIN_QUEUE_EVENTS,
};

#[test]
fn limits_keep_a_complete_focus_release_representable() {
    assert!(matches!(
        InputContext::new(
            EPOCH,
            CLOCK_EPOCH,
            InputLimits {
                queue_events: MIN_QUEUE_EVENTS - 1,
                batch_events: 1
            }
        ),
        Err(InputError::InvalidLimits)
    ));
    assert!(matches!(
        InputContext::new(
            EPOCH,
            CLOCK_EPOCH,
            InputLimits {
                queue_events: MIN_QUEUE_EVENTS,
                batch_events: 0
            }
        ),
        Err(InputError::InvalidLimits)
    ));
    assert!(matches!(
        InputContext::new(0, CLOCK_EPOCH, InputLimits::default()),
        Err(InputError::InvalidLimits)
    ));
    for value in [MAX_QUEUE_EVENTS + 1, u32::MAX as usize, usize::MAX] {
        assert!(matches!(
            InputContext::new(
                EPOCH,
                CLOCK_EPOCH,
                InputLimits {
                    queue_events: value,
                    batch_events: 1,
                }
            ),
            Err(InputError::InvalidLimits)
        ));
    }
    for value in [MAX_BATCH_EVENTS + 1, u32::MAX as usize, usize::MAX] {
        assert!(matches!(
            InputContext::new(
                EPOCH,
                CLOCK_EPOCH,
                InputLimits {
                    queue_events: MIN_QUEUE_EVENTS,
                    batch_events: value,
                }
            ),
            Err(InputError::InvalidLimits)
        ));
    }
    assert!(
        InputContext::new(
            EPOCH,
            CLOCK_EPOCH,
            InputLimits {
                queue_events: MAX_QUEUE_EVENTS,
                batch_events: MAX_BATCH_EVENTS,
            }
        )
        .is_ok()
    );
}

#[test]
fn full_queue_rejects_a_blur_atomically_and_the_same_batch_retries_after_drain() {
    let limits = InputLimits {
        queue_events: MIN_QUEUE_EVENTS,
        batch_events: MIN_QUEUE_EVENTS,
    };
    let mut context = focused(limits);
    let lease = context.acquire(false).unwrap();
    let mut events = Vec::new();
    for code in 0..256 {
        events.push(event(
            101,
            InputKind::Key {
                code,
                pressed: true,
            },
        ));
    }
    for button in 0..8 {
        events.push(event(
            101,
            InputKind::Button {
                button,
                pressed: true,
            },
        ));
    }
    events.push(event(101, InputKind::Capture { captured: true }));
    context.submit_batch(EPOCH, 2, &events).unwrap();
    let state = *context.state();
    let records = queued(&context);
    let blur = [event(102, InputKind::Focus { focused: false })];
    assert_eq!(
        context.submit_batch(EPOCH, 3, &blur),
        Err(InputError::QueueFull)
    );
    assert_eq!(*context.state(), state);
    assert_eq!(queued(&context), records);
    assert_eq!(context.last_sequence(), 2);
    assert_eq!(context.last_timestamp_ns(), 101);
    assert!(context.lease_valid(lease));
    let mut output = vec![InputRecord::default(); MIN_QUEUE_EVENTS];
    assert_eq!(
        context.read(usize::MAX, &mut output).unwrap(),
        MIN_QUEUE_EVENTS
    );
    context.submit_batch(EPOCH, 3, &blur).unwrap();
    assert_eq!(context.queued_events(), MIN_QUEUE_EVENTS);
    assert!(context.state().keys.iter().all(|pressed| !pressed));
    assert!(context.state().buttons.iter().all(|pressed| !pressed));
    assert!(!context.lease_valid(lease));
}

#[test]
fn short_output_and_invalid_consumption_leave_records_and_output_untouched() {
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
    let before = queued(&context);
    let poison = InputRecord {
        batch_sequence: u64::MAX,
        ..InputRecord::default()
    };
    let mut short = [poison];
    assert_eq!(context.peek(2, &mut short), Err(InputError::OutputCapacity));
    assert_eq!(context.read(2, &mut short), Err(InputError::OutputCapacity));
    assert_eq!(short, [poison]);
    assert_eq!(context.consume(3), Err(InputError::InvalidCount));
    assert_eq!(queued(&context), before);
    assert_eq!(context.peek(1, &mut short).unwrap(), 1);
    assert_eq!(short[0], before[0]);
    assert_eq!(queued(&context), before);
    assert_eq!(context.read(1, &mut short).unwrap(), 1);
    assert_eq!(queued(&context), before[1..]);
}

#[test]
fn ring_wrap_preserves_chronological_order_without_reallocating_the_queue() {
    let limits = InputLimits {
        queue_events: MIN_QUEUE_EVENTS,
        batch_events: MIN_QUEUE_EVENTS,
    };
    let mut context = focused(limits);
    let first: Vec<_> = (0..MIN_QUEUE_EVENTS)
        .map(|index| {
            event(
                101 + index as u64,
                InputKind::Key {
                    code: 42,
                    pressed: index % 2 == 0,
                },
            )
        })
        .collect();
    context.submit_batch(EPOCH, 2, &first).unwrap();
    context.consume(100).unwrap();
    let second: Vec<_> = (0..100)
        .map(|index| {
            event(
                1000 + index as u64,
                InputKind::Button {
                    button: 0,
                    pressed: index % 2 == 0,
                },
            )
        })
        .collect();
    context.submit_batch(EPOCH, 3, &second).unwrap();
    let records = queued(&context);
    assert_eq!(records.len(), MIN_QUEUE_EVENTS);
    for (record, expected) in records.iter().zip(first[100..].iter().chain(&second)) {
        assert_eq!(&record.event, expected);
    }
}
