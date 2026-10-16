use super::support::*;

#[test]
fn public_disposal_cannot_destroy_a_unit_retained_by_a_parked_module_owner() {
    use crate::{
        memory::{GuestAddress, PageRange, Permissions},
        process::{CallError, HostError, calls::call::PendingOwner},
        windows::WindowsApi32,
    };
    let (mut engine, atom) = engine_with_cache();
    let unit = engine.prepare_cache_entry(KEY, WNDPROC).unwrap();
    engine
        .install_cached_entry(KEY, unit.serial, unit.unit_id, unit.slot)
        .unwrap();
    begin_create(&mut engine, atom, &[WindowsApi32::LoadLibraryW.id()]);
    load(&mut engine);
    let saved = engine
        .modules
        .operations
        .last_mut()
        .unwrap()
        .execution
        .callback
        .as_mut()
        .unwrap();
    saved.outer.owner = PendingOwner::Resident(unit.unit_id);
    assert!(engine.callback.is_none() && engine.pending_call.is_none());
    let before = engine.cache_usage().unwrap();
    assert_eq!(
        engine.discard_unacknowledged_resident(KEY, unit.unit_id),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.discard_installed_resident(KEY, unit.unit_id, unit.slot),
        Err(HostError::Call(CallError::Busy))
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(
            PageRange::new(GuestAddress(EXE + 0x1000), 1).unwrap(),
            Permissions::from_bits(7).unwrap(),
        )
        .unwrap();
    assert_eq!(
        engine.retire_stale_resident(KEY, unit.unit_id),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.cache_usage().unwrap(), before);
    assert!(
        engine
            .resident
            .as_ref()
            .unwrap()
            .get_retained_raw(engine.memory.as_ref().unwrap(), unit.unit_id)
            .is_ok()
    );
}

#[test]
fn parked_module_context_restores_real_material_and_all_resident_callback_ancestor_pins() {
    use crate::{
        abi::resident_callback::ResidentCallbackRecord32,
        process::{
            calls::{call::PendingOwner, callback::SuspendedRecord},
            loading::modules::callback::ParkedModuleExecution32,
        },
        windows::CallingConvention32,
    };
    let (mut engine, _) = engine();
    material(
        &mut engine,
        &[crate::windows::WindowsApi32::LoadLibraryW.id()],
    );
    let generation = engine.generation;
    let state =
        crate::abi::x86::decode_state(&engine.arena()[..crate::abi::x86::STATE_SIZE]).unwrap();
    let frame = crate::windows::CallFrame32::capture(
        engine.memory().unwrap(),
        state,
        CallingConvention32::Cdecl,
        0,
    )
    .unwrap();
    let callback = |token, outer, home, body, active, parent| {
        crate::process::calls::callback::SuspendedCallback {
            record: SuspendedRecord::Resident {
                record: ResidentCallbackRecord32 {
                    token,
                    outer_token: token - 1,
                    phase: 1,
                    outcome: 0,
                    entry_pc: WNDPROC,
                    entry_esp: state.registers[4],
                    return_pc: RETURN_PC,
                    return_id: RETURN_ID,
                    stack_words: 0,
                    result: 0,
                    outer_unit_id: home,
                    callback_unit_id: body,
                },
                authorized: true,
                active_unit_id: active,
            },
            outer: crate::process::calls::call::PendingCall {
                token: token - 1,
                owner: PendingOwner::Resident(outer),
                frame,
                state: engine.arena()[..crate::abi::x86::STATE_SIZE]
                    .try_into()
                    .unwrap(),
                exit: engine.arena()[crate::abi::arena::EXIT_OFFSET
                    ..crate::abi::arena::EXIT_OFFSET + crate::abi::x86::EXIT_SIZE]
                    .try_into()
                    .unwrap(),
                extended: crate::process::cpu::context::ExtendedCpuState::capture(engine.arena())
                    .unwrap(),
            },
            parent,
        }
    };
    let parent = callback(2, 11, 12, 13, 14, None);
    let current = callback(4, 21, 22, 23, 24, Some(Box::new([parent])));
    let parked = ParkedModuleExecution32 {
        callback: Some(current),
        replies: Vec::new(),
        artifact: engine.artifact.take(),
        generation,
    };
    let mut pins = parked.execution_pins().collect::<Vec<_>>();
    pins.sort_unstable();
    assert_eq!(pins, [11, 12, 13, 14, 21, 22, 23, 24]);
    parked.restore(&mut engine);
    assert_eq!(engine.generation, generation);
    assert!(engine.artifact.is_some());
    assert_eq!(engine.callback.as_ref().unwrap().depth(), 2);
}
