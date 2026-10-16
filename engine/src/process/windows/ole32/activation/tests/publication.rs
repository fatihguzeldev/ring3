#[path = "../../../service/tests/support.rs"]
mod support;

use super::*;
use crate::{
    loader::SystemModule32,
    memory::MemoryError,
    process::{HostError, ModuleOperation32},
    windows::ole32::{HResult32, Ole32Api32},
};

fn arguments(engine: &mut EngineInstance) -> [u32; 5] {
    support::put(engine, support::DATA, &[0x11; 16]);
    support::put(engine, support::DATA + 16, &[0x22; 16]);
    support::put(engine, support::DATA + 128, &0xfeed_beefu32.to_le_bytes());
    [support::DATA, 1, 0, support::DATA + 16, support::DATA + 128]
}

#[test]
fn early_com_failure_cannot_publish_an_hresult_when_null_output_write_is_exhausted() {
    let mut engine = support::engine();
    let args = arguments(&mut engine);
    let (generation, token) =
        support::capture(&mut engine, Ole32Api32::CoGetClassObject.id(), &args);
    let version = engine.memory().unwrap().transaction_version();
    engine.memory.as_mut().unwrap().exhaust_versions_for_test();
    let cpu = engine.arena()[..crate::abi::arena::TRANSFER_OFFSET].to_vec();
    assert_eq!(
        engine.complete_windows_call(support::KEY, generation, token),
        Err(HostError::Memory(MemoryError::VersionExhausted))
    );
    assert_eq!(
        &engine.arena()[..crate::abi::arena::TRANSFER_OFFSET],
        cpu.as_slice()
    );
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    assert_eq!(support::word(&engine, args[4]), 0xfeed_beef);
    assert!(engine.ole32.as_ref().unwrap().activation.pending.is_empty());
    engine
        .memory
        .as_mut()
        .unwrap()
        .restore_version_for_test(version);
    engine
        .complete_windows_call(support::KEY, generation, token)
        .unwrap();
    assert_eq!(support::result(&engine), HResult32::NOT_INITIALIZED.0);
    assert_eq!(support::word(&engine, args[4]), 0);
}

#[test]
fn early_com_guard_failure_keeps_hresult_shape_and_retry_nulls_the_valid_output() {
    let mut engine = support::engine();
    let mut args = arguments(&mut engine);
    args[4] = support::DATA + 4096;
    support::put(&mut engine, args[4], &0xfeed_beefu32.to_le_bytes());
    support::guard(&mut engine, args[4], 1);
    assert_eq!(
        support::call(&mut engine, Ole32Api32::CoGetClassObject.id(), &args),
        HResult32::POINTER.0
    );
    assert_eq!(support::word(&engine, args[4]), 0xfeed_beef);
    assert_eq!(
        support::call(&mut engine, Ole32Api32::CoGetClassObject.id(), &args),
        HResult32::NOT_INITIALIZED.0
    );
    assert_eq!(support::word(&engine, args[4]), 0);
}

#[test]
fn two_same_handle_server_owners_release_both_actual_loader_references_without_initializers() {
    let mut engine = support::engine();
    engine
        .register_system_module(SystemModule32 {
            name: "lifetime.dll".into(),
            preferred_base: 0x500000,
            exports: Vec::new(),
        })
        .unwrap();
    for name in ["lifetime", "lifetime.dll"] {
        assert_eq!(
            engine.load_library_module(name, None).unwrap(),
            ModuleOperation32::Loaded { handle: 0x500000 }
        );
    }
    assert_eq!(
        support::call(&mut engine, Ole32Api32::CoInitializeEx.id(), &[0, 2]),
        0
    );
    let runtime = &mut engine.ole32.as_mut().unwrap().activation;
    for id in [1, 2] {
        runtime.servers.push(Server32 {
            id,
            path: if id == 1 { "lifetime" } else { "lifetime.dll" }.into(),
            handle: 0x500000,
            threading: ServerThreadingModel32::Both,
            threads: vec![1],
            mta_lease: false,
            neutral_lease: false,
            unused_deadline: Some(0),
            activity: 1,
        });
    }
    runtime.next = 2;
    assert_eq!(
        support::call(
            &mut engine,
            Ole32Api32::CoFreeUnusedLibrariesEx.id(),
            &[0, 0]
        ),
        0
    );
    assert!(engine.ole32.as_ref().unwrap().activation.servers.is_empty());
    assert_eq!(engine.module_handle(Some("lifetime.dll")).unwrap(), None);
    assert!(engine.module_windows_reply.is_none());
    assert!(engine.module_initializer().unwrap().is_none());
}

#[test]
fn cached_initializer_to_com_failure_retries_publication_without_repeating_the_loader() {
    let mut engine = support::engine();
    assert_eq!(
        support::call(&mut engine, Ole32Api32::CoInitializeEx.id(), &[0, 2]),
        0
    );
    let args = arguments(&mut engine);
    let (_, token) = support::capture(&mut engine, Ole32Api32::CoGetClassObject.id(), &args);
    let captured = engine.pending_call.as_ref().unwrap();
    let (owner, frame) = (captured.owner, captured.frame);
    let scratch = GuestCallbackScratch32::allocate(
        engine.memory.as_mut().unwrap(),
        ActivationRuntime32::scratch_reservation(),
        &[0; 64],
    )
    .unwrap();
    engine
        .ole32
        .as_mut()
        .unwrap()
        .activation
        .pending
        .push(PendingActivation32 {
            continuation: 1,
            thread: 1,
            owner,
            token,
            frame,
            api: Ole32Api32::CoGetClassObject,
            out: args[4],
            outer: 0,
            path: "failed.dll".into(),
            scratch,
            phase: Phase32::Finish {
                result: HResult32::ERROR_IN_DLL.0,
            },
            threading: ServerThreadingModel32::Both,
            server_threads: vec![1],
            server: None,
        });
    let version = engine.memory().unwrap().transaction_version();
    engine.memory.as_mut().unwrap().exhaust_versions_for_test();
    assert_eq!(
        engine.complete_module_windows_initializer(ModuleOperation32::InitializationFailed, 77),
        Err(HostError::Memory(MemoryError::VersionExhausted))
    );
    assert_eq!(engine.module_windows_reply.as_ref().unwrap().owner(), owner);
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    assert_eq!(support::word(&engine, args[4]), 0xfeed_beef);
    scratch.check(engine.memory().unwrap()).unwrap();
    assert!(engine.module_initializer().unwrap().is_none());
    engine
        .memory
        .as_mut()
        .unwrap()
        .restore_version_for_test(version);
    assert_eq!(
        engine.finish_module_initializer(77).unwrap(),
        ModuleOperation32::InitializationFailed
    );
    assert_eq!(support::result(&engine), HResult32::ERROR_IN_DLL.0);
    assert_eq!(support::word(&engine, args[4]), 0);
    assert!(engine.pending_call.is_none());
    assert!(engine.module_windows_reply.is_none());
    assert!(engine.ole32.as_ref().unwrap().activation.pending.is_empty());
    assert!(engine.finish_module_initializer(77).is_err());
}
