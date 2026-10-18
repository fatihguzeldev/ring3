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

#[test]
fn sent_batch_transfers_full_capacity_and_projects_one_final_generation() {
    let mut queue = MessageQueue32::new(1).unwrap();
    let first = queue.prepare_sent_change(7, 1).unwrap();
    queue.commit_sent_change(first);
    queue
        .post(Message32 {
            thread_id: 7,
            message: 0x400,
            ..Default::default()
        })
        .unwrap();
    let batch = queue.prepare_sent_batch(&[(8, 1), (7, -1)]).unwrap();
    assert_eq!(batch.queue_status(9), None);
    assert_eq!(
        batch.rows().collect::<Vec<_>>(),
        [(8, 0x40, 0x40, 4), (7, 0x108, 0x108, 4)]
    );
    assert_eq!(queue.status(7), (0x148, 0x148, 2));
    assert_eq!(queue.status(8), (0, 0, 2));
    queue.commit_sent_batch(batch);
    assert_eq!(queue.status(7), (0x108, 0x108, 4));
    assert_eq!(queue.status(8), (0x40, 0x40, 4));
    assert_eq!(queue.find(7, u32::MAX, |_| true).unwrap().1.message, 0x400);
}

#[test]
fn sent_batch_rejection_preserves_all_rows_counts_and_generation() {
    let mut queue = MessageQueue32::new(1).unwrap();
    let first = queue.prepare_sent_change(7, 1).unwrap();
    queue.commit_sent_change(first);
    for changes in [
        &[(7, 1), (8, -1)][..],
        &[(7, 1), (8, 1)],
        &[(7, 1), (7, -1)],
        &[(0, 1)],
        &[(7, 0)],
        &[(7, 1), (8, 1), (9, 1)],
    ] {
        assert!(queue.prepare_sent_batch(changes).is_err());
        assert_eq!(queue.sent.len(), 1);
        assert_eq!(queue.sent[0].count, 1);
        assert_eq!(queue.status(7), (0x40, 0x40, 1));
    }
    queue.generation = u64::MAX - 1;
    assert!(queue.prepare_sent_batch(&[(7, -1), (8, 1)]).is_err());
    assert_eq!(queue.status(7), (0x40, 0x40, u64::MAX - 1));
    queue.generation = u64::MAX;
    let empty = queue.prepare_sent_batch(&[]).unwrap();
    assert_eq!(empty.rows().count(), 0);
    queue.commit_sent_batch(empty);
    assert_eq!(queue.status(7), (0x40, 0x40, u64::MAX));
}

#[test]
fn sent_batch_preserves_unobserved_freshness_and_merges_projected_flags() {
    let mut queue = MessageQueue32::new(2).unwrap();
    let batch = queue.prepare_sent_batch(&[(7, 2), (8, 3)]).unwrap();
    queue.commit_sent_batch(batch);
    queue.observe(7, 0x40);
    let mut removal = queue.prepare_sent_batch(&[(7, -1), (8, -3)]).unwrap();
    removal.add_flags(7, 0x20, 0x20);
    assert_eq!(removal.queue_status(7), Some((0x60, 0x20, 4)));
    assert_eq!(removal.queue_status(8), Some((0, 0, 4)));
    queue.commit_sent_batch(removal);
    assert_eq!(queue.status(7), (0x40, 0, 4));
    assert_eq!(queue.status(8), (0, 0, 4));
    assert_eq!(queue.sent[0].count, 1);
}

#[test]
fn stale_second_sent_row_is_rejected_before_the_first_row_changes() {
    let mut queue = MessageQueue32::new(2).unwrap();
    let first = queue.prepare_sent_batch(&[(7, 1), (8, 1)]).unwrap();
    queue.commit_sent_batch(first);
    let batch = queue.prepare_sent_batch(&[(7, 1), (8, 1)]).unwrap();
    // observation changes freshness without advancing the global queue generation.
    queue.observe(8, 0x40);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            queue.commit_sent_batch(batch);
        }))
        .is_err()
    );
    assert_eq!(queue.status(7), (0x40, 0x40, 2));
    assert_eq!(queue.status(8), (0x40, 0, 2));
    assert!(queue.sent.iter().all(|row| row.count == 1));
}
