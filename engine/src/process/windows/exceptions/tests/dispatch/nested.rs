use super::*;

#[test]
fn nested_disposition_sets_the_i386_boundary_flag_until_that_handler_returns() {
    let mut engine = engine();
    let (original, frames) = chain(&mut engine);
    let first = engine
        .begin_exception_delivery(
            ExceptionRecord32::new(0xe0530001, 0, original.state.eip, &[]).unwrap(),
            original,
            PendingOwner::Replacement(0),
            None,
            None,
            None,
        )
        .unwrap();
    dispatcher(&mut engine, frames[1]);
    returned(&mut engine, first, 2, true);
    let nested = engine
        .finish_windows_exception_handler(77, first.id)
        .unwrap()
        .unwrap();
    assert_eq!(nested.procedure, 0x401001);
    assert_eq!(word(&engine, nested.record + 4), 0x10);
    returned(&mut engine, nested, 1, true);
    let outer = engine
        .finish_windows_exception_handler(77, first.id)
        .unwrap()
        .unwrap();
    assert_eq!(outer.procedure, 0x401002);
    assert_eq!(word(&engine, outer.record + 4), 0);
    assert_eq!(word(&engine, word(&engine, 0x7e000000)), frames[0]);
}

#[test]
fn exception_inside_a_structured_handler_crosses_the_i386_wrapper_and_restores_its_parent() {
    let mut engine = engine();
    let (original, frames) = chain(&mut engine);
    let parent = engine
        .begin_exception_delivery(
            ExceptionRecord32::new(0xe0530010, 0, original.state.eip, &[]).unwrap(),
            original,
            PendingOwner::Replacement(0),
            None,
            None,
            None,
        )
        .unwrap();
    let wrapper = word(&engine, 0x7e000000);
    assert_eq!(word(&engine, wrapper), frames[0]);
    assert_eq!(word(&engine, wrapper + 8), frames[0]);
    let handler_context = engine.windows_context_state().unwrap();
    let child = engine
        .begin_exception_delivery(
            ExceptionRecord32::new(0xe0530011, 0, handler_context.state.eip, &[]).unwrap(),
            handler_context,
            PendingOwner::Replacement(0),
            None,
            None,
            None,
        )
        .unwrap();
    assert_ne!(parent.id, child.id);
    assert_eq!(child.procedure, parent.procedure);
    assert_eq!(word(&engine, child.record + 4), 0x10);
    returned(&mut engine, child, 0, true);
    assert_eq!(
        engine
            .finish_windows_exception_handler(77, child.id)
            .unwrap(),
        None
    );
    assert_eq!(engine.pending_windows_exception().unwrap().id, parent.id);
    assert_eq!(engine.windows_context_state().unwrap(), handler_context);
    assert_eq!(word(&engine, 0x7e000000), wrapper);
    returned(&mut engine, parent, 1, true);
    let next = engine
        .finish_windows_exception_handler(77, parent.id)
        .unwrap()
        .unwrap();
    assert_eq!(next.procedure, 0x401001);
    assert_eq!(word(&engine, next.record + 4), 0);
}

#[test]
fn invalid_handler_wrapper_does_not_publish_a_nested_delivery() {
    let mut engine = engine();
    let (original, _) = chain(&mut engine);
    let parent = engine
        .begin_exception_delivery(
            ExceptionRecord32::new(0xe0530010, 0, original.state.eip, &[]).unwrap(),
            original,
            PendingOwner::Replacement(0),
            None,
            None,
            None,
        )
        .unwrap();
    let wrapper = word(&engine, 0x7e000000);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(wrapper + 8), &0u32.to_le_bytes())
        .unwrap();
    let handler_context = engine.windows_context_state().unwrap();
    let arena = engine.arena().to_vec();
    let version = engine.memory().unwrap().transaction_version();
    assert!(
        engine
            .begin_exception_delivery(
                ExceptionRecord32::new(0xe0530011, 0, handler_context.state.eip, &[]).unwrap(),
                handler_context,
                PendingOwner::Replacement(0),
                None,
                None,
                None,
            )
            .is_err()
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.pending_windows_exception().unwrap().id, parent.id);
}
