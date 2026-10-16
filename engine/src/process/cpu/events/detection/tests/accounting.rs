use super::*;

#[test]
fn returned_need_code_duration_authority_is_exact_bounded_and_replaced_by_next_entry() {
    for resident in [false, true] {
        let (mut engine, owner) = fixture(resident);
        let epoch = engine.begin_exception_detection(owner).unwrap();
        stop(&mut engine, ExitReason::Budget, 0x1000, 1, 0);
        assert_eq!(engine.validate_execution_return_epoch(epoch), Ok(owner));
        stop(&mut engine, ExitReason::NeedCode, 0x1000, 1, 0);
        engine
            .finish_generated_detection(epoch, [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
            .unwrap();
        let before = engine.arena().to_vec();
        assert!(!engine.has_retained_detection());
        assert_eq!(engine.validate_execution_return_epoch(epoch), Ok(owner));
        engine.arena_mut().unwrap()[STATE_OFFSET + 16] ^= 1;
        assert_eq!(
            engine.validate_execution_return_epoch(epoch),
            Err(HostError::Call(CallError::StateChanged))
        );
        engine.arena_mut().unwrap().copy_from_slice(&before);
        engine.arena_mut().unwrap()[TRANSFER_OFFSET] ^= 1;
        assert_eq!(engine.validate_execution_return_epoch(epoch), Ok(owner));
        let next = engine.begin_exception_detection(owner).unwrap();
        assert!(next > epoch);
        assert_eq!(
            engine.validate_execution_return_epoch(epoch),
            Err(HostError::Call(CallError::InvalidToken))
        );
        assert_eq!(
            engine.validate_execution_return_epoch(0),
            Err(HostError::Call(CallError::InvalidToken))
        );
    }
}

#[test]
fn ready_and_consumed_event_duration_authority_preserves_original_owner_after_source_write() {
    for resident in [false, true] {
        let (mut engine, owner) = fixture(resident);
        let epoch = engine.begin_exception_detection(owner).unwrap();
        stop(&mut engine, ExitReason::InvalidOpcode, 0x1000, 0, 0);
        engine
            .finish_exception_detection(epoch, ud(), None)
            .unwrap();
        assert_eq!(engine.validate_execution_return_epoch(epoch), Ok(owner));
        engine.cpu_event = Some(
            engine
                .consume_exception_detection(epoch)
                .unwrap()
                .into_event(),
        );
        engine
            .memory
            .as_mut()
            .unwrap()
            .cpu_write(GuestAddress(0x1000), &[0x90])
            .unwrap();
        assert_eq!(engine.validate_execution_return_epoch(epoch), Ok(owner));
        engine.arena_mut().unwrap()[EXIT_OFFSET + 16] ^= 1;
        assert_eq!(
            engine.validate_execution_return_epoch(epoch),
            Err(HostError::Call(CallError::StateChanged))
        );
    }
}
