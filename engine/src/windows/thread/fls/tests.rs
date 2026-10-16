use super::*;

fn next_callback(context: &mut FlsContext32, operation: u64) -> Option<FlsStep32> {
    loop {
        let step = context.step(operation).unwrap();
        context.commit_step(step);
        if step.complete {
            return None;
        }
        if step.procedure != 0 {
            return Some(step);
        }
    }
}
#[test]
fn free_calls_all_nonnull_thread_instances_in_caller_order_and_reuses_the_index() {
    let mut context = FlsContext32::new(4, 3).unwrap();
    let index = context.allocate(1, 0x401000).unwrap();
    assert_eq!(index, 1);
    context.set(1, index, 11).unwrap();
    context.set(3, index, 33).unwrap();
    let operation = context.begin_free(1, index).unwrap();
    assert_eq!(next_callback(&mut context, operation).unwrap().value, 11);
    assert_eq!(context.get(1, index), Ok(11));
    context.set(1, index, 44).unwrap();
    context.complete_callback(operation);
    assert_eq!(context.get(1, index), Ok(0));
    assert_eq!(next_callback(&mut context, operation).unwrap().value, 33);
    context.complete_callback(operation);
    assert_eq!(next_callback(&mut context, operation), None);
    assert_eq!(context.allocate(2, 0), Ok(index));
    assert_eq!(context.get(3, index), Ok(0));
}
#[test]
fn exit_observes_later_slot_changes_and_does_not_repeat_same_slot_replacement() {
    let mut context = FlsContext32::new(4, 2).unwrap();
    let first = context.allocate(1, 0x401000).unwrap();
    let second = context.allocate(1, 0x401010).unwrap();
    context.set(1, first, 11).unwrap();
    context.set(1, second, 22).unwrap();
    context.set(2, second, 99).unwrap();
    let operation = context.begin_exit(1).unwrap().unwrap();
    assert_eq!(next_callback(&mut context, operation).unwrap().value, 11);
    context.set(1, first, 33).unwrap();
    context.set(1, second, 44).unwrap();
    context.complete_callback(operation);
    assert_eq!(next_callback(&mut context, operation).unwrap().value, 44);
    context.complete_callback(operation);
    assert_eq!(next_callback(&mut context, operation), None);
    assert_eq!(context.get(1, first), Ok(0));
    assert_eq!(context.get(2, second), Ok(99));
}
#[test]
fn stale_prepared_step_and_exhausted_operation_identity_preserve_values_and_indices() {
    let mut context = FlsContext32::new(3, 1).unwrap();
    let index = context.allocate(1, 0x401000).unwrap();
    context.set(1, index, 11).unwrap();
    let operation = context.begin_free(1, index).unwrap();
    let step = context.step(operation).unwrap();
    context.set(1, index, 22).unwrap();
    assert_eq!(context.check_step(step), Err(ThreadError32::Stale));
    assert_eq!(context.get(1, index), Ok(22));
    assert_eq!(context.begin_free(1, index), Err(ThreadError32::Busy));
    context.next = u64::MAX;
    let other = context.allocate(1, 0).unwrap();
    assert_eq!(context.begin_free(1, other), Err(ThreadError32::Exhausted));
    assert!(!context.slots[other as usize].unwrap().freeing);
}

#[test]
fn forced_retirement_releases_only_its_owned_callback_operations_without_running_callbacks() {
    let mut context = FlsContext32::new(4, 2).unwrap();
    let index = context.allocate(1, 0x401000).unwrap();
    context.set(1, index, 11).unwrap();
    context.set(2, index, 22).unwrap();
    let operation = context.begin_free(1, index).unwrap();
    assert!(next_callback(&mut context, operation).is_some());
    context.retire_thread(1);
    assert_eq!(context.step(operation), Err(ThreadError32::Stale));
    assert_eq!(context.get(1, index), Ok(0));
    assert_eq!(context.get(2, index), Ok(22));
    assert!(context.begin_free(2, index).is_ok());
}

#[test]
fn bounded_rows_follow_monotonic_thread_ids_without_allocating_empty_thread_values() {
    let mut context = FlsContext32::new(4, 4).unwrap();
    let index = context.allocate(1, 0x401000).unwrap();
    for thread in [2, 3, 4, 5, 100_000, u32::MAX] {
        assert_eq!(context.get(thread, index), Ok(0));
        assert_eq!(context.begin_exit(thread), Ok(None));
        assert_eq!(context.set(thread, index, 0), Ok(()));
        assert_eq!(context.rows.iter().flatten().count(), 0);
        context.set(thread, index, thread).unwrap();
        let operation = context.begin_exit(thread).unwrap().unwrap();
        let step = next_callback(&mut context, operation).unwrap();
        assert_eq!((step.thread, step.value), (thread, thread));
        context.complete_callback(operation);
        assert_eq!(next_callback(&mut context, operation), None);
        assert_eq!(context.rows.iter().flatten().count(), 0);
        assert_eq!(context.rows.len(), 4);
    }
    assert_eq!(context.get(0, index), Err(ThreadError32::InvalidParameter));
    assert_eq!(context.begin_exit(0), Err(ThreadError32::InvalidParameter));
}

#[test]
fn occupied_row_capacity_is_neutral_and_zero_or_retirement_releases_only_its_owner() {
    let mut context = FlsContext32::new(4, 4).unwrap();
    let index = context.allocate(1, 0x401000).unwrap();
    for thread in [11, 55, 777, u32::MAX] {
        context.set(thread, index, thread).unwrap();
    }
    assert_eq!(context.get(100_000, index), Ok(0));
    assert_eq!(context.begin_exit(100_000), Ok(None));
    assert_eq!(context.set(100_000, index, 0), Ok(()));
    assert_eq!(context.set(100_000, index, 9), Err(ThreadError32::Capacity));
    assert_eq!(context.rows.iter().flatten().count(), 4);
    for thread in [11, 55, 777, u32::MAX] {
        assert_eq!(context.get(thread, index), Ok(thread));
    }
    assert_eq!(context.allocate(100_000, 0), Ok(2));
    assert_eq!(context.rows.iter().flatten().count(), 4);
    context.set(55, index, 0).unwrap();
    context.set(100_000, index, 9).unwrap();
    context.retire_thread(u32::MAX);
    context.set(2_000_000, index, 17).unwrap();
    assert_eq!(context.get(55, index), Ok(0));
    assert_eq!(context.get(u32::MAX, index), Ok(0));
    assert_eq!(context.get(11, index), Ok(11));
    assert_eq!(context.get(777, index), Ok(777));
    assert_eq!(context.get(100_000, index), Ok(9));
    assert_eq!(context.get(2_000_000, index), Ok(17));
    assert_eq!(context.rows.len(), 4);
}

#[test]
fn free_callbacks_use_actual_thread_ids_after_row_reuse_and_preserve_other_indices() {
    let mut context = FlsContext32::new(4, 4).unwrap();
    let index = context.allocate(1, 0x401000).unwrap();
    let other = context.allocate(1, 0).unwrap();
    for thread in [u32::MAX, 7, 80] {
        context.set(thread, index, thread).unwrap();
    }
    context.set(80, other, 777).unwrap();
    context.retire_thread(7);
    context.set(500, index, 500).unwrap();
    let operation = context.begin_free(1, index).unwrap();
    for thread in [80, 500, u32::MAX] {
        let step = next_callback(&mut context, operation).unwrap();
        assert_eq!((step.thread, step.value), (thread, thread));
        context.complete_callback(operation);
    }
    assert_eq!(next_callback(&mut context, operation), None);
    assert_eq!(context.get(80, other), Ok(777));
    assert_eq!(context.rows.iter().flatten().count(), 1);
    assert_eq!(context.allocate(100_000, 0), Ok(index));
    assert_eq!(context.get(500, index), Ok(0));
    assert_eq!(context.get(u32::MAX, index), Ok(0));
}

#[test]
fn callback_completion_does_not_clear_a_new_thread_reusing_the_retired_rows_storage() {
    let mut context = FlsContext32::new(4, 2).unwrap();
    let index = context.allocate(1, 0x401000).unwrap();
    context.set(100, index, 10).unwrap();
    context.set(200, index, 20).unwrap();
    let operation = context.begin_free(9, index).unwrap();
    assert_eq!(next_callback(&mut context, operation).unwrap().thread, 100);
    context.retire_thread(100);
    context.set(300, index, 30).unwrap();
    context.complete_callback(operation);
    assert_eq!(context.get(300, index), Ok(30));
    for (thread, value) in [(200, 20), (300, 30)] {
        let step = next_callback(&mut context, operation).unwrap();
        assert_eq!((step.thread, step.value), (thread, value));
        context.complete_callback(operation);
    }
    assert_eq!(next_callback(&mut context, operation), None);
    assert_eq!(context.rows.iter().flatten().count(), 0);
}

#[test]
fn zeroing_the_last_value_preserves_the_active_exit_callback_operation() {
    let mut context = FlsContext32::new(4, 4).unwrap();
    let index = context.allocate(1, 0x401000).unwrap();
    context.set(100_000, index, 17).unwrap();
    let operation = context.begin_exit(100_000).unwrap().unwrap();
    let step = next_callback(&mut context, operation).unwrap();
    context.set(100_000, index, 0).unwrap();
    assert_eq!(context.rows.iter().flatten().count(), 0);
    assert_eq!(context.begin_exit(100_000), Ok(Some(operation)));
    assert_eq!(context.check_callback(operation), Ok(step));
    context.complete_callback(operation);
    assert_eq!(next_callback(&mut context, operation), None);
    assert_eq!(context.begin_exit(100_000), Ok(None));
}
