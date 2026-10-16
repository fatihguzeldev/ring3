use super::*;
use crate::{
    memory::{PageRange, Permissions},
    process::windows::exceptions::ExceptionPhase32,
};

fn guarded(engine: &mut EngineInstance, permissions: Permissions) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .map_zeroed(
            PageRange::new(GuestAddress(0x60000), 1).unwrap(),
            permissions.with_guard(),
        )
        .unwrap();
    engine
        .exceptions
        .handlers
        .add(false, true, 0x401000)
        .unwrap();
}

#[test]
fn data_guard_is_delivered_once_and_preserves_underlying_read_only_protection() {
    let mut engine = engine();
    guarded(&mut engine, Permissions::READ);
    let (unit, epoch) = permission_event(&mut engine, 0x60004, PagingAccess32::Write, 0x401000);
    let dispatch = engine
        .dispatch_resident_windows_exception(KEY, unit, epoch)
        .unwrap();
    assert_eq!(
        (dispatch.phase, dispatch.code),
        (ExceptionPhase32::Vectored, 0x80000001)
    );
    let mut bytes = [0; 28];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(dispatch.record), &mut bytes)
        .unwrap();
    assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 2);
    assert_eq!(u32::from_le_bytes(bytes[20..24].try_into().unwrap()), 1);
    assert_eq!(
        u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
        0x60004
    );
    assert_eq!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(0x60000)),
        Ok(Permissions::READ)
    );
    returned(&mut engine, dispatch, u32::MAX, false);
    assert!(
        engine
            .finish_windows_exception_handler(KEY, dispatch.id)
            .unwrap()
            .is_none()
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x60004), &mut [0])
            .is_ok()
    );
    assert!(
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0x60004), &[1])
            .is_err()
    );
}

#[test]
fn guard_scratch_failure_and_version_exhaustion_preserve_alarm_event_and_dispatch_state() {
    for exhausted in [false, true] {
        let mut engine = engine();
        guarded(&mut engine, Permissions::READ_WRITE);
        let (unit, epoch) = permission_event(&mut engine, 0x60004, PagingAccess32::Read, 0x401000);
        let guard = engine
            .memory()
            .unwrap()
            .guard_page(GuestAddress(0x60004))
            .unwrap();
        let stack = engine.threads.info(1).unwrap();
        let stack_range =
            PageRange::new(GuestAddress(stack.stack_base), stack.stack_pages).unwrap();
        let version = engine.memory().unwrap().transaction_version();
        if exhausted {
            engine.memory.as_mut().unwrap().exhaust_versions_for_test();
        } else {
            engine
                .memory
                .as_mut()
                .unwrap()
                .protect(stack_range, Permissions::READ)
                .unwrap();
        }
        let arena = engine.arena().to_vec();
        assert!(
            engine
                .dispatch_resident_windows_exception(KEY, unit, epoch)
                .is_err()
        );
        assert!(guard.is_current(engine.memory().unwrap()));
        assert_eq!(engine.arena(), arena);
        assert!(engine.cpu_event.is_some());
        assert!(engine.pending_windows_exception().is_none());
        if exhausted {
            engine
                .memory
                .as_mut()
                .unwrap()
                .restore_version_for_test(version);
        } else {
            engine
                .memory
                .as_mut()
                .unwrap()
                .protect(stack_range, Permissions::READ_WRITE)
                .unwrap();
        }
        assert_eq!(
            engine
                .dispatch_resident_windows_exception(KEY, unit, epoch)
                .unwrap()
                .code,
            0x80000001
        );
    }
}

#[test]
fn guarded_instruction_fetch_compiles_to_a_real_paging_stop_and_releases_execute_alarm() {
    let mut engine = engine();
    guarded(&mut engine, Permissions::ALL);
    let (unit, epoch) = permission_event(&mut engine, 0x60000, PagingAccess32::Execute, 0x60000);
    assert!(
        engine
            .resident
            .as_ref()
            .unwrap()
            .get_retained_raw(engine.memory().unwrap(), unit)
            .unwrap()
            .context_fault(0x60000)
            .is_some()
    );
    let dispatch = engine.dispatch_windows_exception(KEY, epoch).unwrap();
    assert_eq!(dispatch.code, 0x80000001);
    assert!(
        engine
            .memory()
            .unwrap()
            .fetch(GuestAddress(0x60000), &mut [0])
            .is_ok()
    );
}

#[test]
fn a_replaced_guard_mapping_cannot_clear_the_new_owner_from_an_old_cpu_stop() {
    let mut engine = engine();
    guarded(&mut engine, Permissions::READ_WRITE);
    let (_, epoch) = permission_event(&mut engine, 0x60004, PagingAccess32::Read, 0x401000);
    let range = PageRange::new(GuestAddress(0x60000), 1).unwrap();
    engine.memory.as_mut().unwrap().unmap(range).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .map_zeroed(range, Permissions::READ_WRITE.with_guard())
        .unwrap();
    assert!(matches!(
        engine.dispatch_windows_exception(KEY, epoch),
        Err(HostError::Exception(
            crate::windows::exceptions::ExceptionError32::Stale
        ))
    ));
    assert!(
        engine
            .memory()
            .unwrap()
            .guard_page(GuestAddress(0x60004))
            .is_some()
    );
}

#[test]
fn guarded_child_teb_can_supply_its_own_tib_under_the_exact_pending_clear() {
    let mut engine = super::stack::child();
    let teb = engine.windows_thread_info(2).unwrap().teb;
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(
            PageRange::new(GuestAddress(teb), 1).unwrap(),
            Permissions::READ_WRITE.with_guard(),
        )
        .unwrap();
    engine
        .exceptions
        .handlers
        .add(false, true, 0x401000)
        .unwrap();
    let (_, epoch) = permission_event(&mut engine, teb + 0x20, PagingAccess32::Read, 0x401000);
    let dispatch = engine.dispatch_windows_exception(KEY, epoch).unwrap();
    assert_eq!((dispatch.code, dispatch.procedure), (0x80000001, 0x401000));
    assert!(
        engine
            .memory()
            .unwrap()
            .guard_page(GuestAddress(teb))
            .is_none()
    );
    let mut identity = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(teb + 0x24), &mut identity)
        .unwrap();
    assert_eq!(u32::from_le_bytes(identity), 2);
}

#[test]
fn selected_handler_on_the_guarded_execute_page_is_preflighted_against_pending_rights() {
    let mut engine = engine();
    guarded(&mut engine, Permissions::ALL);
    engine
        .exceptions
        .handlers
        .add(false, true, 0x60000)
        .unwrap();
    let (_, epoch) = permission_event(&mut engine, 0x60000, PagingAccess32::Execute, 0x60000);
    let dispatch = engine.dispatch_windows_exception(KEY, epoch).unwrap();
    assert_eq!((dispatch.code, dispatch.procedure), (0x80000001, 0x60000));
    assert!(
        engine
            .memory()
            .unwrap()
            .fetch(GuestAddress(dispatch.procedure), &mut [0])
            .is_ok()
    );
}
