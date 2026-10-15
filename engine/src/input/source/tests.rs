use super::*;
use crate::input::{InputKind, MIN_QUEUE_EVENTS, TimedInputEvent};
fn event(kind: InputKind) -> TimedInputEvent {
    TimedInputEvent {
        timestamp_ns: 1,
        kind,
    }
}

#[test]
fn independent_readers_keep_identical_events_and_relative_baselines() {
    let mut source = InputSource32::new(1, 0, MIN_QUEUE_EVENTS).unwrap();
    let first = source.tail();
    let second = first;
    source
        .publish_physical(
            [
                event(InputKind::Focus { focused: true }),
                event(InputKind::Relative {
                    dx: 4,
                    dy: -2,
                    wheel: 120,
                }),
            ]
            .into_iter(),
        )
        .unwrap();
    let mut records = [SourceEvent32::default(); 4];
    let a = source.read(first, &mut records).unwrap();
    let b = source.read(second, &mut records).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.count, 2);
    assert_eq!(source.snapshot().relative.dx, 4);
    assert_eq!(source.read(a.next, &mut records).unwrap().count, 0);
    assert_eq!(source.read(second, &mut records).unwrap().count, 2);
}

#[test]
fn journal_overflow_is_local_to_lagging_readers_and_focus_releases_are_retained() {
    let mut source = InputSource32::new(1, 0, MIN_QUEUE_EVENTS).unwrap();
    let old = source.tail();
    source
        .publish_physical(
            [
                event(InputKind::Focus { focused: true }),
                event(InputKind::Key {
                    code: 30,
                    pressed: true,
                }),
            ]
            .into_iter(),
        )
        .unwrap();
    let recent = source.tail();
    for _ in 0..MIN_QUEUE_EVENTS {
        source
            .publish_physical(
                [event(InputKind::Relative {
                    dx: 1,
                    dy: 0,
                    wheel: 0,
                })]
                .into_iter(),
            )
            .unwrap();
    }
    let mut records = [SourceEvent32::default(); 1];
    assert!(source.read(old, &mut records).unwrap().overflow);
    assert!(!source.read(recent, &mut records).unwrap().overflow);
    let before = source.tail();
    source
        .publish_physical([event(InputKind::Focus { focused: false })].into_iter())
        .unwrap();
    let mut records = [SourceEvent32::default(); 4];
    let read = source.read(before, &mut records).unwrap();
    assert_eq!(read.count, 2);
    assert_eq!(
        records[0].kind,
        SourceKind32::Physical(InputKind::Key {
            code: 30,
            pressed: false
        })
    );
    assert_eq!(source.snapshot().presses[30], 1);
    assert_eq!(source.snapshot().relative.dx, MIN_QUEUE_EVENTS as i64);
}

#[test]
fn legacy_destructive_reads_and_full_queue_do_not_change_canonical_consumers() {
    use crate::input::{InputContext, InputLimits, InputRecord};
    let mut input = InputContext::new(
        1,
        0,
        InputLimits {
            queue_events: MIN_QUEUE_EVENTS,
            batch_events: MIN_QUEUE_EVENTS,
        },
    )
    .unwrap();
    let first = input.source().tail();
    let mut events = vec![event(InputKind::Focus { focused: true })];
    events.extend((0..MIN_QUEUE_EVENTS - 1).map(|_| {
        event(InputKind::Relative {
            dx: 1,
            dy: 0,
            wheel: 0,
        })
    }));
    input.submit_batch(1, 1, &events).unwrap();
    assert_eq!(input.queued_events(), MIN_QUEUE_EVENTS);
    input
        .submit_windows_batch(
            1,
            2,
            &[event(InputKind::Relative {
                dx: 9,
                dy: 0,
                wheel: 0,
            })],
        )
        .unwrap();
    assert_eq!(input.queued_events(), MIN_QUEUE_EVENTS);
    let mut old = [InputRecord::default(); MIN_QUEUE_EVENTS];
    input.read(MIN_QUEUE_EVENTS, &mut old).unwrap();
    assert_eq!(input.drain_relative().dx, (MIN_QUEUE_EVENTS - 1 + 9) as i64);
    let mut records = [SourceEvent32::default(); MIN_QUEUE_EVENTS];
    let read = input.source().read(first, &mut records).unwrap();
    assert!(read.overflow);
    assert_eq!(
        input.source().snapshot().relative.dx,
        (MIN_QUEUE_EVENTS - 1 + 9) as i64
    );
    assert_eq!(input.source().read(first, &mut records).unwrap(), read);
}
