use super::*;
use crate::{
    process::windows::ole32::activation::Phase32,
    windows::{com::ServerThreadingModel32, ole32::Ole32Api32},
};

fn local(phase: Phase32) -> (EngineInstance, u32) {
    let mut engine = engine();
    let address = super::lifetime::pending(
        &mut engine,
        ExecutionApartment32 {
            apartment: ApartmentId32::Mta,
            thread: 2,
        },
    );
    let runtime = engine.ole32.as_mut().unwrap();
    runtime.apartments.uninitialize(1);
    runtime.apartments.initialize(1, 0, 0).unwrap();
    runtime.apartments.initialize(2, 0, 0).unwrap();
    let pending = &mut runtime.activation.pending[0];
    pending.thread = 2;
    pending.api = Ole32Api32::CoCreateInstance;
    pending.threading = ServerThreadingModel32::Both;
    pending.phase = phase;
    runtime.marshaling.activations.clear();
    (engine, address)
}

#[test]
fn local_factory_is_escrowed_when_its_mta_caller_dies_before_create_instance() {
    let (mut engine, address) = local(Phase32::Create {
        factory: 0x10100,
        release: 0x20000,
    });
    let before = engine.memory().unwrap().transaction_version();
    let prepared = engine.prepare_com_thread_exit(2).unwrap();
    assert!(prepared.ranges().is_empty());
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    assert!(
        engine
            .ole32
            .as_ref()
            .unwrap()
            .marshaling
            .activations
            .is_empty()
    );
    engine.commit_com_thread_exit(prepared);
    let runtime = engine.ole32.as_ref().unwrap();
    assert_eq!(runtime.activation.pending.len(), 1);
    let remote = &runtime.marshaling.activations[0];
    assert!(remote.abandoned);
    assert_eq!(remote.execution.apartment, ApartmentId32::Mta);
    let cleanup = remote.cleanup.unwrap();
    assert_eq!(
        (cleanup.pointers, cleanup.count, cleanup.next),
        ([0x10100, 0], 1, 0)
    );
    assert!(engine.com_abandoned_work_ready(1));
    assert!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(address))
            .is_ok()
    );
}

#[test]
fn local_successful_create_cleanup_uses_the_callers_output_and_keeps_each_reference() {
    let (mut engine, address) = local(Phase32::Release { result: 0 });
    engine.write32(address + 48, 0x10100).unwrap();
    engine.write32(address + 52, 0xdeadbeef).unwrap();
    engine.write32(0x10300, 0x10100).unwrap();
    let prepared = engine.prepare_com_thread_exit(2).unwrap();
    assert!(prepared.ranges().is_empty());
    engine.commit_com_thread_exit(prepared);
    let cleanup = engine.ole32.as_ref().unwrap().marshaling.activations[0]
        .cleanup
        .unwrap();
    assert_eq!((cleanup.pointers, cleanup.count), ([0x10100, 0x10100], 2));
    assert_eq!(engine.ole32_read_word(address + 52).unwrap(), 0xdeadbeef);
}

#[test]
fn sealed_local_create_result_keeps_output_outside_the_activation_scratch() {
    let (mut engine, address) = local(Phase32::Create {
        factory: 0x10100,
        release: 0x20000,
    });
    engine.write32(address + 52, 0xdeadbeef).unwrap();
    engine.write32(0x10300, 0x10120).unwrap();
    let cleanup = engine.activation_cleanup(0, Some(0)).unwrap();
    assert_eq!((cleanup.pointers, cleanup.count), ([0x10100, 0x10120], 2));
    let failed = engine.activation_cleanup(0, Some(0x80004005)).unwrap();
    assert_eq!((failed.pointers, failed.count), ([0x10100, 0], 1));
}
