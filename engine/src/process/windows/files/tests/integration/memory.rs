use super::*;
use crate::{
    process::{CallError, HostError},
    windows::{WindowsApi32 as Api, files::FileApi32 as File},
};

#[test]
fn reserve_commit_rounds_guest_address_and_virtual_protect_cannot_escalate_view_access() {
    let mut engine = engine();
    let section = complete(
        &mut engine,
        File::CreateFileMappingW.id(),
        &[u32::MAX, 0, 0x0400_0004, 0, 4096, 0],
    );
    let view = complete(
        &mut engine,
        File::MapViewOfFile.id(),
        &[section, 4, 0, 0, 0],
    );
    assert_eq!(
        complete(
            &mut engine,
            Api::VirtualAlloc.id(),
            &[view + 3, 1, 0x1000, 2]
        ),
        view
    );
    assert_eq!(
        complete(&mut engine, Api::VirtualProtect.id(), &[view, 1, 4, 0x1200]),
        0
    );
    assert_eq!(engine.windows_thread.last_error(), 87);
    assert_eq!(
        complete(&mut engine, Api::VirtualProtect.id(), &[view, 1, 1, 0x1200]),
        1
    );
    assert_eq!(
        super::super::support::dword(engine.memory().unwrap(), 0x1200),
        2
    );
}

#[test]
fn raw_virtual_alloc_gate_with_wrong_stack_metadata_is_rejected_before_argument_access() {
    for words in [0, 2] {
        let mut engine = engine();
        let token = capture(&mut engine, Api::VirtualAlloc.id(), &[], Some(words));
        assert_eq!(
            engine.complete_windows_call(KEY, engine.generation, token),
            Err(HostError::Call(CallError::InvalidRequest))
        );
        assert!(engine.pending_call.is_some());
        engine.abandon_call(KEY, token).unwrap();
    }
}

#[test]
fn mapping_virtual_alloc_falls_back_when_unowned_and_preserves_owned_token_priority() {
    use crate::process::calls::call::PendingOwner;
    let mut engine = engine();
    let token = capture(&mut engine, Api::VirtualAlloc.id(), &[], Some(0));
    let frame = engine.pending_call.as_ref().unwrap().frame;
    let owner = PendingOwner::Replacement(engine.generation);
    assert!(
        !engine
            .try_complete_file_mapping_allocation(owner, 0, frame)
            .unwrap()
    );
    let files = engine.files.take();
    assert!(
        !engine
            .try_complete_file_mapping_allocation(owner, 0, frame)
            .unwrap()
    );
    engine.files = files;
    engine.abandon_call(KEY, token).unwrap();
    let section = complete(
        &mut engine,
        File::CreateFileMappingW.id(),
        &[u32::MAX, 0, 0x0400_0004, 0, 4096, 0],
    );
    let view = complete(
        &mut engine,
        File::MapViewOfFile.id(),
        &[section, 4, 0, 0, 0],
    );
    let token = capture(&mut engine, Api::VirtualAlloc.id(), &[view, 1], Some(2));
    let frame = engine.pending_call.as_ref().unwrap().frame;
    let owner = PendingOwner::Replacement(engine.generation);
    assert_eq!(
        engine.try_complete_file_mapping_allocation(owner, token + 1, frame),
        Err(HostError::Call(CallError::InvalidToken))
    );
    assert_eq!(
        engine.try_complete_file_mapping_allocation(owner, token, frame),
        Err(HostError::Call(CallError::InvalidRequest))
    );
    engine.abandon_call(KEY, token).unwrap();
}
