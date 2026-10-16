use super::*;
#[test]
fn literal_flags_deadlines_and_single_live_callback_preserve_periodic_identity() {
    let mut timers = MultimediaTimerState32::new(2);
    let target = MultimediaTimerTarget32::Function {
        procedure: 0x1000,
        user: 17,
    };
    assert!(timers.prepare_create(0, 0, 0, 0, target).is_err());
    assert!(timers.prepare_create(0, 10, 0, 2, target).is_err());
    let prepared = timers.prepare_create(100, 10, 0, 0x101, target).unwrap();
    let id = prepared.id();
    assert_eq!(timers.commit_create(prepared), id);
    assert_eq!(timers.deadline(), Some(10_000_100));
    assert!(timers.prepare_due(10_000_099).unwrap().is_none());
    let due = timers.prepare_due(50_000_100).unwrap().unwrap();
    assert_eq!(due.occurrences, 5);
    assert_eq!(due.target, target);
    assert_eq!(timers.commit_due(due), None);
    assert_eq!(timers.deadline(), None);
    assert_eq!(timers.can_kill(id).unwrap(), (Some(due.identity), None));
    timers.commit_kill(id);
    assert!(timers.callback_pending(due.identity));
    assert!(timers.can_kill(id).is_err());
    timers.finish_callback(due.identity).unwrap();
    assert!(!timers.callback_pending(due.identity));
    assert_eq!(
        timers.finish_callback(due.identity),
        Err(MultimediaTimerError32::Stale)
    );
}
#[test]
fn one_shot_expires_before_callback_and_failed_capacity_does_not_consume_ids() {
    let mut timers = MultimediaTimerState32::new(1);
    let target = MultimediaTimerTarget32::Function {
        procedure: 0x1000,
        user: 0,
    };
    let prepared = timers.prepare_create(0, 1, 0, 0, target).unwrap();
    let id = timers.commit_create(prepared);
    assert!(timers.prepare_create(0, 1, 0, 0, target).is_err());
    let due = timers.prepare_due(1_000_000).unwrap().unwrap();
    timers.commit_due(due);
    assert!(timers.can_kill(id).is_err());
    timers.finish_callback(due.identity).unwrap();
    let prepared = timers.prepare_create(1_000_000, 1, 0, 0, target).unwrap();
    assert_eq!(prepared.id(), id + 1);
}
