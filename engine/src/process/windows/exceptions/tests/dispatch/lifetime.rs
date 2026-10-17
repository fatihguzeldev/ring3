use super::*;
use crate::{process::HostError, windows::exceptions::ExceptionApi32};

fn returning_unwind(engine: &mut EngineInstance, target: u32) {
    let mut caller = engine.windows_context_state().unwrap();
    caller.state.eip = 0x401010;
    caller.state.registers[4] -= 32;
    caller.state.registers[0] = 0;
    let resumed = engine
        .begin_exception_delivery(
            ExceptionRecord32::new(0xc0000027, 2, caller.state.eip, &[]).unwrap(),
            caller,
            PendingOwner::Replacement(0),
            None,
            Some(Unwind32 {
                target,
                return_value: 0,
            }),
            None,
        )
        .unwrap();
    assert_eq!(resumed.phase, ExceptionPhase32::Resumed);
    assert_eq!(engine.windows_context_state().unwrap(), caller);
}

#[test]
fn returning_i386_unwind_keeps_the_popped_handlers_exact_return_receipt() {
    let mut engine = engine();
    let (original, frames) = chain(&mut engine);
    let parent = engine
        .begin_exception_delivery(
            ExceptionRecord32::new(0xe0530020, 0, original.state.eip, &[]).unwrap(),
            original,
            PendingOwner::Replacement(0),
            None,
            None,
            None,
        )
        .unwrap();
    returning_unwind(&mut engine, frames[0]);
    assert_eq!(word(&engine, 0x7e000000), frames[0]);
    assert_eq!(engine.pending_windows_exception().unwrap().id, parent.id);
    assert!(
        engine.exceptions.deliveries[0]
            .active
            .unwrap()
            .registration_popped
    );

    returned(&mut engine, parent, 0, true);
    let returned = engine.windows_context_state().unwrap();
    let mut wrong_stack = returned;
    wrong_stack.state.registers[4] -= 4;
    engine.publish_windows_context(&wrong_stack).unwrap();
    let arena = engine.arena().to_vec();
    let version = engine.memory().unwrap().transaction_version();
    assert_eq!(
        engine.finish_windows_exception_handler(77, parent.id),
        Err(HostError::Exception(
            crate::windows::exceptions::ExceptionError32::Stale
        ))
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    engine.publish_windows_context(&returned).unwrap();
    assert_eq!(
        engine
            .finish_windows_exception_handler(77, parent.id)
            .unwrap(),
        None
    );
    assert_eq!(engine.windows_context_state().unwrap(), original);
    assert_eq!(word(&engine, 0x7e000000), frames[0]);
    assert!(engine.pending_windows_exception().is_none());
    assert!(
        engine
            .finish_windows_exception_handler(77, parent.id)
            .is_err()
    );
}

#[test]
fn a_nonlocal_jump_past_a_popped_child_preserves_the_older_exact_handler_return() {
    let mut engine = engine();
    let (original, frames) = chain(&mut engine);
    let parent = engine
        .begin_exception_delivery(
            ExceptionRecord32::new(0xe0530021, 0, original.state.eip, &[]).unwrap(),
            original,
            PendingOwner::Replacement(0),
            None,
            None,
            None,
        )
        .unwrap();
    let parent_return_esp = engine.exceptions.deliveries[0].active.unwrap().return_esp;
    returning_unwind(&mut engine, frames[0]);
    let handler = engine.windows_context_state().unwrap();
    let child = engine
        .begin_exception_delivery(
            ExceptionRecord32::new(0xe0530022, 0, handler.state.eip, &[]).unwrap(),
            handler,
            PendingOwner::Replacement(0),
            None,
            None,
            None,
        )
        .unwrap();
    returning_unwind(&mut engine, frames[0]);
    assert_eq!(engine.exceptions.deliveries.len(), 2);
    let mut state = engine.windows_context_state().unwrap();
    state.state.eip = parent.return_pc;
    state.state.registers[4] = parent_return_esp;
    state.state.registers[0] = 0;
    engine.publish_windows_context(&state).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate {
                id: parent.return_id,
            },
        },
        &mut engine.arena.as_mut().get_mut()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    assert_eq!(engine.pending_windows_exception().unwrap().id, parent.id);
    assert!(
        engine
            .finish_windows_exception_handler(77, child.id)
            .is_err()
    );
    assert_eq!(engine.exceptions.deliveries.len(), 2);
    assert_eq!(
        engine
            .finish_windows_exception_handler(77, parent.id)
            .unwrap(),
        None
    );
    assert!(engine.exceptions.deliveries.is_empty());
    assert_eq!(engine.windows_context_state().unwrap(), original);
}

#[test]
fn a_successful_api_after_a_nonlocal_jump_releases_popped_returns_and_their_pins() {
    let mut engine = engine();
    let (original, frames) = chain(&mut engine);
    let parent = engine
        .begin_exception_delivery(
            ExceptionRecord32::new(0xe0530023, 0, original.state.eip, &[]).unwrap(),
            original,
            PendingOwner::Resident(123),
            None,
            None,
            None,
        )
        .unwrap();
    returning_unwind(&mut engine, frames[0]);
    assert_eq!(
        engine.exceptions.execution_pins().collect::<Vec<_>>(),
        [123]
    );
    let mut jumped = engine.windows_context_state().unwrap();
    jumped.state.eip = 0x401010;
    jumped.state.registers[4] = frames[0] - 4;
    engine.publish_windows_context(&jumped).unwrap();
    assert!(engine.pending_windows_exception().is_none());
    assert_eq!(engine.exceptions.execution_pins().count(), 1);
    super::super::review::raising::captured_call_on_stack(
        &mut engine,
        crate::windows::WindowsApi32::GetLastError.id(),
        &[],
        None,
        jumped.state.registers[4],
    )
    .unwrap();
    assert!(engine.exceptions.deliveries.is_empty());
    assert_eq!(engine.exceptions.execution_pins().count(), 0);
    assert!(
        engine
            .finish_windows_exception_handler(77, parent.id)
            .is_err()
    );
}

#[test]
fn explicit_nonlocal_context_restoration_retires_linked_and_popped_returns() {
    use crate::windows::exceptions::context::{CONTEXT_ALL32, Win32Context32};
    for api in [
        ExceptionApi32::NtContinue,
        ExceptionApi32::RtlRestoreContext,
    ] {
        let mut engine = engine();
        engine.load_library_module("ntdll.dll", None).unwrap();
        let (original, frames) = chain(&mut engine);
        let parent = engine
            .begin_exception_delivery(
                ExceptionRecord32::new(0xe0530024, 0, original.state.eip, &[]).unwrap(),
                original,
                PendingOwner::Resident(123),
                None,
                None,
                None,
            )
            .unwrap();
        returning_unwind(&mut engine, frames[0]);
        let handler = engine.windows_context_state().unwrap();
        engine
            .begin_exception_delivery(
                ExceptionRecord32::new(0xe0530025, 0, handler.state.eip, &[]).unwrap(),
                handler,
                PendingOwner::Resident(456),
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(engine.exceptions.execution_pins().count(), 2);
        let context = Win32Context32::capture(CONTEXT_ALL32, &original).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0x7f001000), context.bytes())
            .unwrap();
        super::super::review::raising::captured_call_on_stack(
            &mut engine,
            api.id(),
            &[0x7f001000, 0],
            None,
            0x7f002000,
        )
        .unwrap();
        assert_eq!(engine.windows_context_state().unwrap(), original);
        assert!(engine.exceptions.deliveries.is_empty());
        assert_eq!(engine.exceptions.execution_pins().count(), 0);
        assert!(
            engine
                .finish_windows_exception_handler(77, parent.id)
                .is_err()
        );
    }
}
