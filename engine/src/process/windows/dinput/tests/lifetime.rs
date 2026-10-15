use super::support::*;
use crate::{
    input::dinput::{DirectInputResult32, InterfaceKind32},
    memory::GuestAddress,
    process::windows::dinput::memory::words,
};
#[test]
fn query_interfaces_share_identity_and_device_retains_parent_until_final_release() {
    let (mut runtime, mut memory, pointer, _) = runtime(false);
    let parent = runtime.device(pointer).unwrap().parent;
    assert_eq!(runtime.com.references(parent).unwrap(), 2);
    memory
        .write(GuestAddress(DATA + 64), &InterfaceKind32::Device7W.iid().0)
        .unwrap();
    assert_eq!(
        runtime
            .query(&mut memory, pointer, DATA + 64, DATA + 80)
            .unwrap(),
        0
    );
    let [alias] = words(&memory, DATA + 80).unwrap();
    assert_ne!(alias, pointer.0);
    assert_eq!(
        runtime.com.lookup(GuestAddress(alias)).unwrap().id,
        runtime.com.lookup(pointer).unwrap().id
    );
    assert_eq!(runtime.release(&mut memory, None, parent).unwrap(), 1);
    assert_eq!(runtime.release(&mut memory, None, pointer).unwrap(), 1);
    assert_eq!(
        runtime
            .release(&mut memory, None, GuestAddress(alias))
            .unwrap(),
        0
    );
    assert!(runtime.com.lookup(parent).is_err());
    assert!(runtime.com.lookup(pointer).is_err());
}
#[test]
fn version8_refuses_legacy_query_without_reference_change() {
    let (mut runtime, mut memory, pointer, _) = runtime(true);
    memory
        .write(GuestAddress(DATA + 64), &InterfaceKind32::Device7A.iid().0)
        .unwrap();
    let before = runtime.com.references(pointer).unwrap();
    assert_eq!(
        runtime
            .query(&mut memory, pointer, DATA + 64, DATA + 80)
            .unwrap(),
        DirectInputResult32::NO_INTERFACE.0
    );
    assert_eq!(runtime.com.references(pointer).unwrap(), before);
    assert_eq!(words::<1>(&memory, DATA + 80).unwrap(), [0]);
}
