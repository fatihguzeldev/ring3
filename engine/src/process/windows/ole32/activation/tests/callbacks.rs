use super::*;
use crate::windows::ole32::HResult32;
#[test]
fn class_factory_creation_uses_captured_iid_and_balances_factory_release_before_failure() {
    let (mut engine, scratch) = pending(Ole32Api32::CoCreateInstance);
    engine.write32(scratch + 48, 0x10100).unwrap();
    engine.write32(0x10100, 0x10200).unwrap();
    engine.write32(0x10208, 0x20002).unwrap();
    engine.write32(0x1020c, 0x20004).unwrap();
    let Ole32CallbackOutcome32::Callback(create) = engine.resume_ole32_callback(1, 0).unwrap()
    else {
        panic!("real factory callback")
    };
    assert_eq!(create.procedure, GuestAddress(0x20004));
    assert_eq!(create.count, 4);
    assert_eq!(
        &create.arguments[..4],
        &[0x10100, 0xabcd, scratch + 32, 0x10300]
    );
    let mut requested = [0u8; 16];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(scratch + 32), &mut requested)
        .unwrap();
    assert_eq!(requested, [0x55; 16]);
    engine.write32(0x10300, 0xfeed).unwrap();
    let Ole32CallbackOutcome32::Callback(release) = engine
        .resume_ole32_callback(1, HResult32::NO_INTERFACE.0)
        .unwrap()
    else {
        panic!("balanced release")
    };
    assert_eq!(release.procedure, GuestAddress(0x20002));
    assert_eq!(release.count, 1);
    assert_eq!(release.arguments[0], 0x10100);
    let Ole32CallbackOutcome32::Complete(result) =
        engine.resume_ole32_callback(1, 0xdeadbeef).unwrap()
    else {
        panic!("original HRESULT")
    };
    assert_eq!(result, HResult32::NO_INTERFACE.0);
    assert_eq!(engine.ole32_read_word(0x10300).unwrap(), 0);
    assert!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(scratch))
            .is_err()
    );
    assert!(engine.resume_ole32_callback(1, 0).is_err());
}
#[test]
fn requested_class_factory_interface_is_returned_without_runtime_releasing_it() {
    let (mut engine, scratch) = pending(Ole32Api32::CoGetClassObject);
    engine.write32(0x10300, 0x10200).unwrap();
    let Ole32CallbackOutcome32::Complete(result) = engine.resume_ole32_callback(1, 0).unwrap()
    else {
        panic!("factory returned")
    };
    assert_eq!(result, 0);
    assert_eq!(engine.ole32_read_word(0x10300).unwrap(), 0x10200);
    assert!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(scratch))
            .is_err()
    );
}
#[test]
fn forced_thread_retirement_prepares_only_its_exact_live_activation_scratch() {
    let (mut engine, scratch) = pending(Ole32Api32::CoCreateInstance);
    assert_eq!(
        engine.prepare_com_thread_exit(1).unwrap().ranges(),
        [PageRange::new(GuestAddress(scratch), 1).unwrap()]
    );
    assert!(
        engine
            .prepare_com_thread_exit(2)
            .unwrap()
            .ranges()
            .is_empty()
    );
    engine.unmap(scratch, 1).unwrap();
    engine.map(scratch, 1, 3).unwrap();
    assert!(engine.prepare_com_thread_exit(1).is_err());
}
