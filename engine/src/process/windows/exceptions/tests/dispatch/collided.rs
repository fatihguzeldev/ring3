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
        assert_eq!(word(&engine, 0x7e000000), frames[0]);
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
