use super::*;

#[test]
fn private_continuation_cannot_authorize_a_public_return_without_full_upgrade() {
    let (mut engine, owner) = fixture(true);
    let epoch = engine.begin_exception_detection(owner).unwrap();
    stop(&mut engine, ExitReason::NeedCode, 0x1000, 0, 0);
    let arena = engine.arena();
    let continued = ContinuedDetection32 {
        epoch,
        owner,
        callback_token: None,
        state: arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]
            .try_into()
            .unwrap(),
        exit: arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE]
            .try_into()
            .unwrap(),
        work: arena[WORK_OFFSET..WORK_OFFSET + WORK_SIZE]
            .try_into()
            .unwrap(),
        compiled_token: 97,
    };
    engine.exception_detection.as_mut().unwrap()[0] = DetectionSlot32::Continued(continued);
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.validate_detected_return_epoch(epoch),
        Err(HostError::Call(CallError::StateChanged))
    );
    assert_eq!(
        engine
            .prepare_execution_terminal(epoch, 0, true, 0, 0, 10)
            .map(|_| ()),
        Err(HostError::Call(CallError::StateChanged))
    );
    assert_eq!(
        engine.consume_exception_detection(epoch).map(|_| ()),
        Err(HostError::Call(CallError::InvalidToken))
    );
    assert_eq!(
        engine.materialize_compiled_continuation(),
        Err(HostError::Call(CallError::StateChanged))
    );
    assert!(engine.has_compiled_light_continuation());
    assert_eq!(engine.arena(), before);
}
