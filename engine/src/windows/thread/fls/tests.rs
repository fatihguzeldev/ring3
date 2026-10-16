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
