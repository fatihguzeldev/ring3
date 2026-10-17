use super::*;

fn unwind(engine: &mut EngineInstance) -> (ExceptionDispatch32, [u32; 3]) {
    let (original, frames) = chain(engine);
    let dispatch = engine
        .begin_exception_delivery(
            ExceptionRecord32::new(0xc0000027, 2, original.state.eip, &[]).unwrap(),
            original,
            PendingOwner::Replacement(0),
            None,
            Some(Unwind32 {
                target: frames[2],
                return_value: 0x13572468,
            }),
            None,
        )
        .unwrap();
    (dispatch, frames)
}

#[test]
fn collided_unwind_pops_the_dispatcher_registration_and_preserves_its_target() {
    let mut engine = engine();
    let (dispatch, frames) = unwind(&mut engine);
    dispatcher(&mut engine, frames[1]);
    returned(&mut engine, dispatch, 3, true);
    assert_eq!(
        engine
            .finish_windows_exception_handler(77, dispatch.id)
            .unwrap(),
        None
    );
    assert_eq!(word(&engine, 0x7e000000), frames[2]);
    assert_eq!(
        engine.windows_context_state().unwrap().state.registers[0],
        0x13572468
    );
    assert!(engine.pending_windows_exception().is_none());
}

#[test]
fn malformed_collision_is_neutral_and_the_same_handler_return_can_be_repaired() {
    for bad in [0, 0x7f003f11, 0x7f003ef0, 0xfffffff0] {
        let mut engine = engine();
        let (dispatch, frames) = unwind(&mut engine);
        dispatcher(&mut engine, bad);
        returned(&mut engine, dispatch, 3, true);
        let head = word(&engine, 0x7e000000);
        assert_eq!(word(&engine, head), frames[0]);
        let arena = engine.arena().to_vec();
        let version = engine.memory().unwrap().transaction_version();
        let pending = engine.pending_windows_exception();
        assert!(
            engine
                .finish_windows_exception_handler(77, dispatch.id)
                .is_err()
        );
        assert_eq!(engine.arena(), arena);
        assert_eq!(engine.memory().unwrap().transaction_version(), version);
        assert_eq!(engine.pending_windows_exception(), pending);
        assert_eq!(word(&engine, 0x7e000000), head);
        dispatcher(&mut engine, frames[1]);
        assert_eq!(
            engine
                .finish_windows_exception_handler(77, dispatch.id)
                .unwrap(),
            None
        );
        assert_eq!(word(&engine, 0x7e000000), frames[2]);
    }
}

#[test]
fn unwind_inside_an_unwind_handler_collides_without_discarding_its_physical_return() {
    let mut engine = engine();
    let (parent, frames) = unwind(&mut engine);
    let current = engine.windows_context_state().unwrap();
    let child = engine
        .begin_exception_delivery(
            ExceptionRecord32::new(0xc0000027, 2, current.state.eip, &[]).unwrap(),
            current,
            PendingOwner::Replacement(0),
            None,
            Some(Unwind32 {
                target: frames[2],
                return_value: 0x27182818,
            }),
            None,
        )
        .unwrap();
    assert_eq!(child.procedure, 0x401001);
    assert!(
        engine
            .exceptions
            .deliveries
            .iter()
            .any(|delivery| delivery.id == parent.id)
    );
    returned(&mut engine, child, 1, true);
    assert_eq!(
        engine
            .finish_windows_exception_handler(77, child.id)
            .unwrap(),
        None
    );
    assert_eq!(word(&engine, 0x7e000000), frames[2]);
    assert_eq!(
        engine.windows_context_state().unwrap().state.registers[0],
        0x27182818
    );
    assert_eq!(engine.pending_windows_exception().unwrap().id, parent.id);
    returned(&mut engine, parent, 1, true);
    let next = engine
        .finish_windows_exception_handler(77, parent.id)
        .unwrap()
        .unwrap();
    assert_eq!(next.procedure, 0x401001);
    returned(&mut engine, next, 1, true);
    assert_eq!(
        engine
            .finish_windows_exception_handler(77, parent.id)
            .unwrap(),
        None
    );
    assert!(engine.exceptions.deliveries.is_empty());
}
