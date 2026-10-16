use super::*;

#[test]
fn protected_handles_reject_guest_close_but_internal_retirement_releases_all_ownership() {
    let mut context = context();
    let handle = context.create_event(true, false, true, None).unwrap().0;
    context.set_handle_flags(handle, 2, 2).unwrap();
    assert_eq!(context.handle_flags(handle), Ok(3));
    let revision = context.revision();
    assert_eq!(
        context.close_handle(handle),
        Err(SynchronizationError32::InvalidHandle)
    );
    assert_eq!(context.revision(), revision);
    let prepared = context.prepare_close_handles(&[handle]).unwrap();
    context.commit_close_handles(prepared);
    assert_eq!(
        context.handle_flags(handle),
        Err(SynchronizationError32::InvalidHandle)
    );
    for _ in 0..8 {
        context.create_event(false, false, false, None).unwrap();
    }
}

#[test]
fn duplicated_protected_source_stays_open_and_copy_gets_independent_flags() {
    let mut context = context();
    let source = context.create_event(true, false, true, None).unwrap().0;
    context.set_handle_flags(source, 2, 2).unwrap();
    let prepared = context
        .prepare_duplicate(source, 0, false, true, true, true)
        .unwrap();
    let copy = prepared.handle().unwrap();
    context.commit_duplicate(prepared);
    assert_eq!(context.handle_flags(source), Ok(3));
    assert_eq!(context.handle_flags(copy), Ok(0));
    context.close_handle(copy).unwrap();
    context.set_handle_flags(source, 2, 0).unwrap();
    assert_eq!(context.handle_flags(source), Ok(1));
    context.close_handle(source).unwrap();
}
