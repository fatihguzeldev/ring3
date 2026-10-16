use super::support::*;
use crate::{
    memory::{GuestAddress, Permissions},
    process::{CallError, HostError},
    windows::{
        activation::ActivationApi32, files::FileApi32, ole32::Ole32Api32, registry::Advapi32Api32,
    },
};

#[test]
fn captured_activation_creation_guard_failure_keeps_invalid_handle_and_exact_token_authority() {
    for api in [
        ActivationApi32::CreateActCtxA,
        ActivationApi32::CreateActCtxW,
    ] {
        let mut engine = engine();
        engine.configure_activation(Default::default()).unwrap();
        put(&mut engine, DATA, &32u32.to_le_bytes());
        put(&mut engine, DATA + 0x3000, &0xfeed_faceu32.to_le_bytes());
        guard(&mut engine, DATA, 2);
        let teb = engine.module_teb_address().unwrap();
        guard(&mut engine, teb, 1);
        let (generation, token) = capture(&mut engine, api.id(), &[DATA]);
        let before = engine.arena().to_vec();
        let version = engine.memory().unwrap().transaction_version();
        assert_eq!(
            engine.complete_windows_call(KEY, generation, token + 1),
            Err(HostError::Call(CallError::InvalidToken))
        );
        assert_eq!(engine.arena(), before.as_slice());
        assert_eq!(engine.memory().unwrap().transaction_version(), version);
        assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
        assert_eq!(word(&engine, DATA + 0x3000), 0xfeed_face);

        // the guarded TEB fails admission before Activation publishes a result.
        // this exercises the captured call's cold failure convention, unlike
        // an ACTCTX read failure handled directly by the activation provider.
        engine
            .complete_windows_call(KEY, generation, token)
            .unwrap();
        assert_eq!(result(&engine), u32::MAX);
        assert_eq!(engine.current_windows_thread().unwrap().last_error(), 998);
        assert!(engine.pending_call.is_none());
        assert_eq!(engine.memory().unwrap().guard_page(GuestAddress(teb)), None);
        assert!(
            engine
                .memory()
                .unwrap()
                .guard_page(GuestAddress(DATA))
                .is_some()
        );
        assert_eq!(word(&engine, DATA + 0x3000), 0xfeed_face);

        // repair the input to a readable, invalid structure. Reusing the old
        // token must retain the accepted 998 result instead of executing it.
        engine
            .memory
            .as_mut()
            .unwrap()
            .protect(range(DATA, 2), Permissions::READ_WRITE)
            .unwrap();
        put(&mut engine, DATA, &0u32.to_le_bytes());
        let before = engine.arena().to_vec();
        let version = engine.memory().unwrap().transaction_version();
        assert_eq!(
            engine.complete_windows_call(KEY, generation, token),
            Err(HostError::Call(CallError::InvalidToken))
        );
        assert_eq!(engine.arena(), before.as_slice());
        assert_eq!(engine.memory().unwrap().transaction_version(), version);
        assert_eq!(engine.current_windows_thread().unwrap().last_error(), 998);
        assert_eq!(word(&engine, DATA + 0x3000), 0xfeed_face);
        assert_eq!(call(&mut engine, api.id(), &[DATA]), u32::MAX);
        assert_eq!(engine.current_windows_thread().unwrap().last_error(), 87);

        // controls distinguish the ACTCTX input fault from the TEB admission
        // fault, and prove a completed service retains no fault for a new call.
        guard(&mut engine, DATA, 1);
        assert_eq!(call(&mut engine, api.id(), &[DATA]), u32::MAX);
        assert_eq!(engine.current_windows_thread().unwrap().last_error(), 998);
        assert_eq!(
            engine.memory().unwrap().guard_page(GuestAddress(DATA)),
            None
        );
        assert_eq!(call(&mut engine, api.id(), &[DATA]), u32::MAX);
        assert_eq!(engine.current_windows_thread().unwrap().last_error(), 87);
        assert_eq!(call(&mut engine, api.id(), &[DATA + 0x4000]), u32::MAX);
        assert_eq!(engine.current_windows_thread().unwrap().last_error(), 998);
        assert_eq!(word(&engine, DATA + 0x3000), 0xfeed_face);
    }
}

#[test]
fn guarded_filename_returns_invalid_handle_and_retry_does_not_retain_the_fault() {
    let mut engine = engine();
    put(&mut engine, DATA, b"C:\\Temp\\guard.bin\0");
    guard(&mut engine, DATA, 2);
    let args = [DATA, 0xc000_0000, 7, 0, 2, 0x80, 0];
    assert_eq!(
        call(&mut engine, FileApi32::CreateFileA.id(), &args),
        u32::MAX
    );
    assert_eq!(engine.current_windows_thread().unwrap().last_error(), 998);
    assert_eq!(
        engine.memory().unwrap().guard_page(GuestAddress(DATA)),
        None
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .guard_page(GuestAddress(DATA + 4096))
            .is_some()
    );
    let handle = call(&mut engine, FileApi32::CreateFileA.id(), &args);
    assert_ne!(handle, u32::MAX);
    assert_ne!(handle, 0);
}

#[test]
fn guard_failure_preserves_registry_lstatus_and_com_hresult_conventions() {
    let mut engine = engine();
    put(&mut engine, DATA, &[0]);
    put(&mut engine, DATA + 0x2000, &0xdead_beefu32.to_le_bytes());
    guard(&mut engine, DATA, 1);
    let args = [0x8000_0001, DATA, 0, 0x20019, DATA + 0x2000];
    assert_eq!(
        call(&mut engine, Advapi32Api32::RegOpenKeyExA.id(), &args),
        998
    );
    assert_eq!(
        engine.current_windows_thread().unwrap().last_error(),
        0x1234_5678
    );
    assert_eq!(word(&engine, DATA + 0x2000), 0xdead_beef);
    assert_eq!(
        call(&mut engine, Advapi32Api32::RegOpenKeyExA.id(), &args),
        0
    );
    assert_ne!(word(&engine, DATA + 0x2000), 0xdead_beef);

    guard(&mut engine, DATA, 1);
    put(&mut engine, DATA + 0x2000, &0xfeed_faceu32.to_le_bytes());
    assert_eq!(
        call(
            &mut engine,
            Ole32Api32::CoGetClassObject.id(),
            &[DATA, 1, 0, DATA + 0x1000, DATA + 0x2000]
        ),
        0x8000_4003
    );
    assert_eq!(
        engine.current_windows_thread().unwrap().last_error(),
        0x1234_5678
    );
    assert_eq!(word(&engine, DATA + 0x2000), 0xfeed_face);
    assert_eq!(
        engine.memory().unwrap().guard_page(GuestAddress(DATA)),
        None
    );
}
