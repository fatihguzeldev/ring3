use super::*;
use crate::process::windows::d3d8::lifetime::LifetimeChange32;

#[test]
fn resource_resurrection_rescues_device_before_parent_or_implicit_disposal_is_expanded() {
    let (mut runtime, mut memory, device) = runtime();
    runtime
        .device_resource_call(&mut memory, device, 23, &[device.0, 16, 0, 0, 2, DATA])
        .unwrap();
    let resource = GuestAddress(read(&memory, DATA));
    runtime
        .change_lifetime(
            &mut memory,
            LifetimeChange32 {
                release: vec![resource],
                hold: vec![resource],
                ..Default::default()
            },
            &[],
        )
        .unwrap();
    assert_eq!(runtime.com.references(resource), Ok(0));
    assert_eq!(runtime.com.references(device), Ok(1));
    let ObjectKind32::Device(owner) = &runtime.object(device).unwrap().kind else {
        unreachable!()
    };
    let parent = owner.parent;
    let parent_refs = runtime.com.references(parent).unwrap();
    memory.exhaust_versions_for_test();
    let prepared = runtime
        .prepare_lifetime(
            &memory,
            LifetimeChange32 {
                retain: vec![resource],
                release: vec![device],
                unhold: vec![resource],
                ..Default::default()
            },
            &[],
        )
        .unwrap();
    assert!(prepared.destroyed().is_empty());
    runtime.commit_lifetime(&mut memory, prepared);
    assert_eq!(runtime.com.references(resource), Ok(1));
    assert_eq!(runtime.com.references(device), Ok(1));
    assert_eq!(runtime.com.references(parent), Ok(parent_refs));
    assert!(runtime.retired_devices.is_empty());
}
