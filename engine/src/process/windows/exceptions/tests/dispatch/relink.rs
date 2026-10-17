use super::*;
use crate::{process::HostError, windows::exceptions::ExceptionError32};

fn unwind_to(engine: &mut EngineInstance, target: u32) {
    let mut caller = engine.windows_context_state().unwrap();
    caller.state.eip = 0x401010;
    caller.state.registers[4] -= 32;
    caller.state.registers[0] = 0;
    let mut dispatch = engine
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
    while dispatch.phase != ExceptionPhase32::Resumed {
        returned(engine, dispatch, 1, true);
        let Some(next) = engine
            .finish_windows_exception_handler(77, dispatch.id)
            .unwrap()
        else {
            break;
        };
        dispatch = next;
    }
    assert_eq!(engine.windows_context_state().unwrap(), caller);
    assert_eq!(word(engine, 0x7e000000), target);
}

fn relinked_handler() -> (EngineInstance, ExceptionDispatch32, [u32; 3], u32) {
    let mut engine = engine();
    let (original, frames) = chain(&mut engine);
    let first = engine
        .begin_exception_delivery(
            ExceptionRecord32::new(0xe0530030, 0, original.state.eip, &[]).unwrap(),
            original,
            PendingOwner::Replacement(0),
            None,
            None,
            None,
        )
        .unwrap();
    returned(&mut engine, first, 1, true);
    let parent = engine
        .finish_windows_exception_handler(77, first.id)
        .unwrap()
        .unwrap();
    assert_eq!(parent.procedure, 0x401001);
    let marker = word(&engine, 0x7e000000);
    assert_eq!(word(&engine, marker), frames[0]);
    unwind_to(&mut engine, frames[1]);
    // the MSCRT runtime reconnects the marker to the completed unwind head.
    engine
        .memory
        .as_mut()
        .unwrap()
        .write_spans(&[
            (GuestAddress(marker), &frames[1].to_le_bytes()),
            (GuestAddress(0x7e000000), &marker.to_le_bytes()),
        ])
        .unwrap();
    (engine, parent, frames, marker)
}

#[test]
fn msvcrt_relinked_dispatcher_marker_returns_and_pops_its_current_validated_link() {
    let (mut engine, parent, frames, marker) = relinked_handler();
    returned(&mut engine, parent, 1, true);
    let next = engine
        .finish_windows_exception_handler(77, parent.id)
        .unwrap()
        .unwrap();
    assert_eq!(next.procedure, 0x401002);
    assert_eq!(word(&engine, 0x7e000000), marker);
    assert_eq!(word(&engine, marker), frames[1]);
    returned(&mut engine, next, 1, true);
    let unhandled = engine
        .finish_windows_exception_handler(77, parent.id)
        .unwrap()
        .unwrap();
    assert_eq!(unhandled.phase, ExceptionPhase32::Unhandled);
    assert_eq!(word(&engine, 0x7e000000), frames[1]);
}

#[test]
fn relinked_marker_survives_nested_delivery_and_a_second_completed_unwind() {
    let (mut engine, parent, frames, marker) = relinked_handler();
    let original = engine.windows_context_state().unwrap();
    let child = engine
        .begin_exception_delivery(
            ExceptionRecord32::new(0xe0530031, 0, original.state.eip, &[]).unwrap(),
            original,
            PendingOwner::Replacement(0),
            None,
            None,
            None,
        )
        .unwrap();
    assert_eq!(child.procedure, 0x401001);
    assert_eq!(word(&engine, child.record + 4), 0x10);
    returned(&mut engine, child, 0, true);
    assert_eq!(
        engine
            .finish_windows_exception_handler(77, child.id)
            .unwrap(),
        None
    );
    assert_eq!(engine.windows_context_state().unwrap(), original);
    assert_eq!(word(&engine, 0x7e000000), marker);
    assert_eq!(word(&engine, marker), frames[1]);

    unwind_to(&mut engine, frames[2]);
    // its detached bytes now retain the first completed unwind head.
    assert_eq!(word(&engine, marker), frames[1]);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write_spans(&[
            (GuestAddress(marker), &frames[2].to_le_bytes()),
            (GuestAddress(0x7e000000), &marker.to_le_bytes()),
        ])
        .unwrap();
    returned(&mut engine, parent, 1, true);
    let next = engine
        .finish_windows_exception_handler(77, parent.id)
        .unwrap()
        .unwrap();
    assert_eq!(next.procedure, 0x401002);
    assert_eq!(word(&engine, marker), frames[2]);
    returned(&mut engine, next, 1, true);
    let unhandled = engine
        .finish_windows_exception_handler(77, parent.id)
        .unwrap()
        .unwrap();
    assert_eq!(unhandled.phase, ExceptionPhase32::Unhandled);
    assert_eq!(word(&engine, 0x7e000000), frames[2]);
}

#[test]
fn relinked_marker_rejects_unauthorized_links_and_changed_identity_atomically() {
    for invalid in 0..4 {
        let (mut engine, parent, frames, marker) = relinked_handler();
        let (address, value) = match invalid {
            0 => (marker, frames[2]),
            1 => (0x7e000000, frames[1]),
            2 => (marker + 4, 0x401002),
            _ => (marker + 8, frames[2]),
        };
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(address), &value.to_le_bytes())
            .unwrap();
        returned(&mut engine, parent, 1, true);
        let arena = engine.arena().to_vec();
        let version = engine.memory().unwrap().transaction_version();
        let pending = engine.pending_windows_exception();
        assert_eq!(
            engine.finish_windows_exception_handler(77, parent.id),
            Err(HostError::Exception(ExceptionError32::InvalidChain)),
            "invalid marker case {invalid}"
        );
        assert_eq!(engine.arena(), arena);
        assert_eq!(engine.memory().unwrap().transaction_version(), version);
        assert_eq!(engine.pending_windows_exception(), pending);
        assert_eq!(word(&engine, address), value);
    }
}
