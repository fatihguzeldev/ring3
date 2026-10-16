use super::*;
use crate::windows::exceptions::ExceptionApi32;

#[path = "raising.rs"]
mod raising;

fn chain(
    engine: &mut EngineInstance,
) -> (
    crate::windows::exceptions::context::ContextState32,
    u32,
    u32,
) {
    let mut original = engine.windows_context_state().unwrap();
    let first = original.state.registers[4] - 64;
    let second = first + 16;
    engine
        .memory
        .as_mut()
        .unwrap()
        .write_spans(&[
            (GuestAddress(0x7e000000), &first.to_le_bytes()),
            (
                GuestAddress(first),
                &[second.to_le_bytes(), 0x401000u32.to_le_bytes()].concat(),
            ),
            (
                GuestAddress(second),
                &[u32::MAX.to_le_bytes(), 0x401001u32.to_le_bytes()].concat(),
            ),
        ])
        .unwrap();
    original.state.registers[4] = first - 16;
    original.state.registers[0] = 0xdead0000;
    engine.publish_windows_context(&original).unwrap();
    (original, first, second)
}
#[test]
fn unwind_preserves_handler_changes_to_the_shared_context_and_only_removes_visited_registration() {
    let mut engine = engine();
    let (original, _, target) = chain(&mut engine);
    let dispatch = engine
        .begin_exception_delivery(
            ExceptionRecord32::new(0xc0000027, 2, original.state.eip, &[]).unwrap(),
            original,
            PendingOwner::Replacement(0),
            None,
            Some(Unwind32 {
                target,
                return_value: 0xdead0000,
            }),
            None,
        )
        .unwrap();
    assert_eq!(dispatch.phase, ExceptionPhase32::Unwind);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(dispatch.context + 176),
            &0xdead0001u32.to_le_bytes(),
        )
        .unwrap();
    returned(&mut engine, dispatch, 1, true);
    assert_eq!(
        engine
            .finish_windows_exception_handler(77, dispatch.id)
            .unwrap(),
        None
    );
    assert_eq!(
        engine.windows_context_state().unwrap().state.registers[0],
        0xdead0001
    );
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x7e000000), &mut bytes)
        .unwrap();
    assert_eq!(u32::from_le_bytes(bytes), target);
}
#[test]
fn unwind_to_current_registration_resumes_initial_context_without_reading_unpublished_scratch() {
    let mut engine = engine();
    let (original, target, _) = chain(&mut engine);
    let dispatch = engine
        .begin_exception_delivery(
            ExceptionRecord32::new(0xc0000027, 2, original.state.eip, &[]).unwrap(),
            original,
            PendingOwner::Replacement(0),
            None,
            Some(Unwind32 {
                target,
                return_value: 0xdead0000,
            }),
            None,
        )
        .unwrap();
    assert_eq!(dispatch.phase, ExceptionPhase32::Resumed);
    assert_eq!(
        engine.windows_context_state().unwrap().state,
        original.state
    );
    assert!(engine.pending_windows_exception().is_none());
}
#[test]
fn rtlraise_updates_input_and_handler_exception_address_to_the_actual_caller_pc() {
    let mut engine = engine();
    engine.load_library_module("ntdll.dll", None).unwrap();
    let id = ExceptionApi32::RtlRaiseException.id();
    let gate = engine
        .loaded_windows_modules()
        .flat_map(|module| module.gates())
        .find(|gate| gate.id == id)
        .unwrap()
        .entry
        .0;
    let mut original = engine.windows_context_state().unwrap();
    let record_address = original.state.registers[4] - 128;
    let stack = original.state.registers[4] - 256;
    let return_pc = 0x401010u32;
    engine
        .memory
        .as_mut()
        .unwrap()
        .write_spans(&[
            (
                GuestAddress(record_address),
                &ExceptionRecord32::new(0xe0001234, 0, 0, &[])
                    .unwrap()
                    .bytes(),
            ),
            (
                GuestAddress(stack),
                &[return_pc.to_le_bytes(), record_address.to_le_bytes()].concat(),
            ),
        ])
        .unwrap();
    for (index, word) in [gate, 2, gate, id].into_iter().enumerate() {
        engine.arena.as_mut().get_mut()
            [TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    engine.compile_with_gates(1, 1).unwrap();
    original.state.eip = gate;
    original.state.registers[4] = stack;
    engine.publish_windows_context(&original).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id },
        },
        &mut engine.arena.as_mut().get_mut()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    engine
        .exceptions
        .handlers
        .add(false, true, 0x401000)
        .unwrap();
    let token = engine
        .capture_windows_call(77, engine.generation)
        .unwrap()
        .token;
    engine
        .complete_windows_call(77, engine.generation, token)
        .unwrap();
    let dispatch = engine.pending_windows_exception().unwrap();
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(dispatch.record + 12), &mut bytes)
        .unwrap();
    assert_eq!(u32::from_le_bytes(bytes), return_pc);
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(record_address + 12), &mut bytes)
        .unwrap();
    assert_eq!(u32::from_le_bytes(bytes), return_pc);
}
#[test]
fn kernel_and_ntdll_aliases_preserve_unique_canonical_gate_identity() {
    let mut engine = engine();
    engine.load_library_module("ntdll.dll", None).unwrap();
    assert!(engine.module_handle(Some("ntdll.dll")).unwrap().is_some());
    let mut ids = std::collections::BTreeMap::new();
    for gate in engine
        .loaded_windows_modules()
        .flat_map(|module| module.gates())
    {
        assert!(
            ids.insert(gate.id, gate.entry.0).is_none(),
            "duplicate gate ID {:#x}",
            gate.id
        );
    }
}

#[test]
fn vectored_continue_execution_stops_remaining_continue_handler_search() {
    let mut engine = engine();
    engine
        .exceptions
        .handlers
        .add(false, true, 0x401000)
        .unwrap();
    engine
        .exceptions
        .handlers
        .add(true, false, 0x401001)
        .unwrap();
    engine
        .exceptions
        .handlers
        .add(true, false, 0x401002)
        .unwrap();
    let original = engine.windows_context_state().unwrap();
    let dispatch = engine
        .begin_exception_delivery(
            ExceptionRecord32::new(0xe0001234, 0, original.state.eip, &[]).unwrap(),
            original,
            PendingOwner::Replacement(0),
            None,
            None,
            None,
        )
        .unwrap();
    returned(&mut engine, dispatch, u32::MAX, false);
    let continuing = engine
        .finish_windows_exception_handler(77, dispatch.id)
        .unwrap()
        .unwrap();
    assert_eq!(
        (continuing.phase, continuing.procedure),
        (ExceptionPhase32::Continue, 0x401001)
    );
    returned(&mut engine, continuing, u32::MAX, false);
    assert_eq!(
        engine
            .finish_windows_exception_handler(77, continuing.id)
            .unwrap(),
        None
    );
    assert!(engine.pending_windows_exception().is_none());
}
