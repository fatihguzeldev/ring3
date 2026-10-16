use super::*;
fn context(packets: usize) -> CompletionContext32 {
    CompletionContext32::new(4, 8, packets, 4).unwrap()
}
fn packet(value: u32) -> CompletionPacket32 {
    CompletionPacket32 {
        bytes: value,
        key: 7,
        overlapped: value + 100,
        status: 0,
        error: 0,
    }
}
fn post(context: &mut CompletionContext32, port: CompletionPortId32, value: u32) {
    let receipt = context.prepare_post(port, packet(value)).unwrap();
    context.commit_post(receipt);
}
#[test]
fn packets_are_fifo_waiters_lifo_and_concurrency_counts_only_runnable_associated_threads() {
    let mut context = context(4);
    let port = context.create(1).unwrap();
    for thread in [1, 2] {
        let wait = context.prepare_wait(port, thread).unwrap();
        context.commit_wait(wait);
    }
    post(&mut context, port, 10);
    post(&mut context, port, 20);
    assert!(!context.ready(port, 1, |_| true));
    assert!(context.ready(port, 2, |_| true));
    let take = context.prepare_take(port, 2, 1, |_| true).unwrap().unwrap();
    assert_eq!(take.packets(), &[packet(10)]);
    context.commit_take(take);
    assert!(!context.ready(port, 1, |_| true));
    assert!(context.ready(port, 1, |thread| thread != 2));
    let take = context
        .prepare_take(port, 1, 1, |thread| thread != 2)
        .unwrap()
        .unwrap();
    assert_eq!(take.packets(), &[packet(20)]);
    context.commit_take(take);
}
#[test]
fn association_survives_numeric_close_packets_are_bounded_and_last_waiter_observes_closed_port() {
    let mut context = context(1);
    let port = context.create(1).unwrap();
    let target = CompletionTarget32::File(OpenFileId(42));
    context.associate(target, port, 99).unwrap();
    context.associate(target, port, 11).unwrap();
    assert_eq!(context.association(target), Some((port, 99, 0)));
    let other = context.create(1).unwrap();
    assert_eq!(
        context.associate(target, other, 0),
        Err(FileError32::InvalidParameter)
    );
    context.set_modes(target, 1).unwrap();
    context.set_modes(target, 2).unwrap();
    assert_eq!(context.association(target), Some((port, 99, 3)));
    let wait = context.prepare_wait(port, 1).unwrap();
    context.commit_wait(wait);
    post(&mut context, port, 1);
    let revision = context.revision;
    assert!(matches!(
        context.prepare_post(port, packet(2)),
        Err(FileError32::Capacity)
    ));
    assert_eq!(context.revision, revision);
    context.retain(port).unwrap();
    context.release(port).unwrap();
    assert!(!context.is_closed(port));
    context.release(port).unwrap();
    assert!(context.is_closed(port));
    assert!(context.ready(port, 1, |_| true));
    assert_eq!(context.association(target), Some((port, 99, 3)));
    context.cancel_wait(1);
    context.release_target(target);
    assert!(context.port(port).is_err());
    post(&mut context, other, 2);
    let take = context
        .prepare_take(other, 3, 2, |_| true)
        .unwrap()
        .unwrap();
    assert_eq!(take.packets(), &[packet(2)]);
    context.commit_take(take);
}
#[test]
fn exhausted_preparations_and_invalid_masks_leave_every_queue_and_identity_unchanged() {
    let mut context = context(2);
    let port = context.create(1).unwrap();
    context
        .associate(CompletionTarget32::File(OpenFileId(1)), port, 0)
        .unwrap();
    let revision = context.revision;
    assert_eq!(
        context.set_modes(CompletionTarget32::File(OpenFileId(1)), 4),
        Err(FileError32::InvalidParameter)
    );
    assert_eq!(context.revision, revision);
    context.revision = u64::MAX;
    assert!(matches!(
        context.prepare_post(port, packet(1)),
        Err(FileError32::Exhausted)
    ));
    assert!(matches!(
        context.prepare_wait(port, 1),
        Err(FileError32::Exhausted)
    ));
    assert_eq!(context.port(port).unwrap().queued, 0);
    context.revision = revision;
    post(&mut context, port, 3);
    let take = context.prepare_take(port, 1, 1, |_| true).unwrap().unwrap();
    assert_eq!(take.packets(), &[packet(3)]);
    context.commit_take(take);
}

#[test]
fn interrupted_wait_cleanup_preserves_new_port_and_same_port_nested_active_authority() {
    let mut context = context(4);
    let old = context.create(1).unwrap();
    let new = context.create(1).unwrap();
    let wait = context.prepare_wait(old, 1).unwrap();
    context.commit_wait(wait);
    context.release(old).unwrap();
    context.cancel_wait_for(1, old);
    assert!(context.port(old).is_err());
    post(&mut context, new, 1);
    post(&mut context, new, 2);
    let take = context.prepare_take(new, 1, 1, |_| true).unwrap().unwrap();
    context.commit_take(take);
    context.cancel_wait_for(1, old);
    assert!(
        !context.ready(new, 2, |_| true),
        "outer old-port cleanup cannot revoke nested active association"
    );
    context.cancel_wait_for(1, new);
    assert!(
        !context.ready(new, 2, |_| true),
        "same-port cleanup only cancels a real waiting row"
    );
    context.retire_thread(1);
    assert!(context.ready(new, 2, |_| true));
}

#[test]
fn closed_port_drops_inaccessible_packets_so_other_sources_and_final_close_can_progress() {
    let mut context = context(1);
    let closed = context.create(1).unwrap();
    let target = CompletionTarget32::File(OpenFileId(42));
    context.associate(target, closed, 7).unwrap();
    post(&mut context, closed, 1);
    let other = context.create(1).unwrap();
    assert!(matches!(
        context.prepare_post(other, packet(2)),
        Err(FileError32::Capacity)
    ));
    context.release(closed).unwrap();
    assert!(
        context.port(closed).is_ok(),
        "association keeps canonical object identity"
    );
    let ignored = context.prepare_post(closed, packet(3)).unwrap();
    context.commit_post(ignored);
    post(&mut context, other, 2);
    let take = context
        .prepare_take(other, 1, 1, |_| true)
        .unwrap()
        .unwrap();
    assert_eq!(take.packets(), &[packet(2)]);
    context.commit_take(take);
    context.release_target(target);
    assert!(context.port(closed).is_err());
}
