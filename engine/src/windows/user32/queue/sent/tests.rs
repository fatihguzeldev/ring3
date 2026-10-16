use super::*;
use crate::windows::user32::Message32;

#[test]
fn sent_status_is_thread_owned_observable_and_independent_of_posted_message_capacity() {
    let mut queue = MessageQueue32::new(1).unwrap();
    let first = queue.prepare_sent_change(7, 1).unwrap();
    assert_eq!(first.thread(), 7);
    assert_eq!(first.queue_status(), (0x40, 0x40, 1));
    assert_eq!(queue.status(7), (0, 0, 0));
    queue.commit_sent_change(first);
    queue.observe(8, 0x40);
    queue.observe(7, 1);
    assert_eq!(queue.status(7), (0x40, 0x40, 1));
    assert_eq!(queue.status(8), (0, 0, 1));
    queue.observe(7, 0x40);
    assert_eq!(queue.status(7), (0x40, 0, 1));
    let second = queue.prepare_sent_change(7, 1).unwrap();
    queue.commit_sent_change(second);
    queue
        .post(Message32 {
            thread_id: 7,
            message: 0x400,
            ..Default::default()
        })
        .unwrap();
    assert!(queue.can_post(1).is_err());
    assert_eq!(queue.status(7), (0x148, 0x148, 3));
    let removal = queue.prepare_sent_change(7, -1).unwrap();
    assert_eq!(removal.queue_status(), (0x148, 0x148, 4));
    queue.commit_sent_change(removal);
    queue.observe(7, 0x40);
    let last = queue.prepare_sent_change(7, -1).unwrap();
    assert_eq!(last.queue_status(), (0x108, 0x108, 5));
    queue.commit_sent_change(last);
    assert_eq!(queue.find(7, u32::MAX, |_| true).unwrap().1.message, 0x400);
    assert_eq!(queue.status(7), (0x108, 0x108, 5));
}

#[test]
fn sent_admission_and_retirement_are_bounded_and_failure_neutral() {
    let mut queue = MessageQueue32::new(1).unwrap();
    let first = queue.prepare_sent_change(7, 1).unwrap();
    queue.commit_sent_change(first);
    assert!(queue.prepare_sent_change(8, 1).is_err());
    assert!(queue.prepare_sent_change(0, 1).is_err());
    assert!(queue.prepare_sent_change(7, 0).is_err());
    assert!(queue.prepare_sent_change(7, -2).is_err());
    assert_eq!(queue.status(7), (0x40, 0x40, 1));
    assert_eq!(queue.project_without_input(7), queue.status(7));
    queue.retire_thread(7);
    assert_eq!(queue.status(7), (0, 0, 1));
    let replacement = queue.prepare_sent_change(8, 1).unwrap();
    queue.commit_sent_change(replacement);
    queue.generation = u64::MAX;
    assert!(queue.prepare_sent_change(8, 1).is_err());
    assert_eq!(queue.status(8), (0x40, 0x40, u64::MAX));
}
