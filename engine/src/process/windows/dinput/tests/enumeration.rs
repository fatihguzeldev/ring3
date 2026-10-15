use super::support::*;
use crate::{
    input::dinput::{DeviceKind32, InterfaceKind32},
    memory::GuestAddress,
    process::WindowsProviderOutcome32,
};
#[test]
fn enumeration_callbacks_stop_and_release_owned_scratch() {
    let (mut runtime, mut memory, pointer, source) = runtime(true);
    let outcome = runtime
        .enum_devices(
            &mut memory,
            &source,
            InterfaceKind32::Input8A,
            3,
            0x9000,
            0x55,
            0,
        )
        .unwrap();
    let WindowsProviderOutcome32::Callback(request) = outcome else {
        panic!("expected callback")
    };
    assert_eq!(request.count, 2);
    assert_eq!(request.arguments[1], 0x55);
    let address = GuestAddress(request.arguments[0]);
    assert!(memory.mapping_version_at(address).is_ok());
    assert_eq!(
        runtime
            .complete_enumeration(&mut memory, request.continuation, 0, None)
            .unwrap(),
        WindowsProviderOutcome32::Return(0)
    );
    assert!(memory.mapping_version_at(address).is_err());
    assert_eq!(
        runtime.device(pointer).unwrap().kind,
        DeviceKind32::Keyboard
    );
}

#[test]
fn semantic_enumeration_retains_its_device_and_retries_next_creation_without_skipping() {
    let (mut runtime, mut memory, pointer, source) = runtime(true);
    let parent = runtime.device(pointer).unwrap().parent;
    runtime.config.max_devices = 2;
    let mut header = [0; 328];
    for (at, value) in [
        (0, 328),
        (4, 40),
        (8, 8),
        (12, 2),
        (16, DATA + 1536),
        (40, 4),
    ] {
        crate::process::windows::dinput::memory::put(&mut header, at, value);
    }
    memory.write(GuestAddress(DATA + 1024), &header).unwrap();
    let mut actions = [0; 80];
    for (at, value) in [(4, 0x8100_041e), (44, 0x8200_040c)] {
        crate::process::windows::dinput::memory::put(&mut actions, at, value);
    }
    memory.write(GuestAddress(DATA + 1536), &actions).unwrap();
    let outcome = runtime
        .enum_semantics(
            &mut memory,
            &source,
            parent,
            InterfaceKind32::Input8A,
            &[parent.0, 0, DATA + 1024, 0x9000, 0x55, 0],
        )
        .unwrap();
    let WindowsProviderOutcome32::Callback(first) = outcome else {
        panic!("expected callback")
    };
    assert_eq!(first.count, 5);
    assert_eq!(first.arguments[3], 1);
    let first_device = GuestAddress(first.arguments[1]);
    assert_eq!(
        runtime.device(first_device).unwrap().kind,
        DeviceKind32::Keyboard
    );
    runtime.com.add_ref(&memory, first_device).unwrap();
    assert!(
        runtime
            .complete_enumeration(&mut memory, first.continuation, 1, None)
            .is_err()
    );
    assert_eq!(runtime.com.references(first_device).unwrap(), 1);
    assert_eq!(runtime.release(&mut memory, None, first_device).unwrap(), 0);
    let WindowsProviderOutcome32::Callback(second) = runtime
        .complete_enumeration(&mut memory, first.continuation, 1, None)
        .unwrap()
    else {
        panic!("expected second callback")
    };
    assert_eq!(second.arguments[3], 0);
    let mouse = GuestAddress(second.arguments[1]);
    assert_eq!(runtime.device(mouse).unwrap().kind, DeviceKind32::Mouse);
    assert_eq!(
        runtime
            .complete_enumeration(&mut memory, second.continuation, 0, None)
            .unwrap(),
        WindowsProviderOutcome32::Return(0)
    );
    assert!(runtime.com.lookup(mouse).is_err());
    assert_eq!(runtime.com.references(parent).unwrap(), 2);
}

#[test]
fn semantic_flags_reject_undefined_bits_and_filter_unavailable_hardware_honestly() {
    let (mut runtime, mut memory, pointer, source) = runtime(true);
    let parent = runtime.device(pointer).unwrap().parent;
    let mut header = [0; 328];
    for (at, value) in [(0, 328), (4, 40), (8, 4), (12, 1), (16, DATA + 1536)] {
        crate::process::windows::dinput::memory::put(&mut header, at, value);
    }
    memory.write(GuestAddress(DATA + 1024), &header).unwrap();
    let mut action = [0; 40];
    crate::process::windows::dinput::memory::put(&mut action, 4, 0x8100_041e);
    memory.write(GuestAddress(DATA + 1536), &action).unwrap();
    let mut args = [parent.0, 0, DATA + 1024, 0x9000, 0, 0];
    for flags in [1, 2, 4, 8, 0x20, 0x80, 0x8000] {
        args[5] = flags;
        assert_eq!(
            runtime.enum_semantics(
                &mut memory,
                &source,
                parent,
                InterfaceKind32::Input8A,
                &args
            ),
            Err(crate::input::dinput::DirectInputResult32::INVALID_PARAM)
        );
    }
    for flags in [0x100, 0x2000, 0x4000] {
        args[5] = flags;
        assert_eq!(
            runtime
                .enum_semantics(
                    &mut memory,
                    &source,
                    parent,
                    InterfaceKind32::Input8A,
                    &args
                )
                .unwrap(),
            WindowsProviderOutcome32::Return(0)
        );
    }
    for flags in [0, 0x10, 0x1000] {
        args[5] = flags;
        let WindowsProviderOutcome32::Callback(request) = runtime
            .enum_semantics(
                &mut memory,
                &source,
                parent,
                InterfaceKind32::Input8A,
                &args,
            )
            .unwrap()
        else {
            panic!("keyboard should enumerate")
        };
        assert_eq!(request.arguments[2], 0x21);
        runtime
            .complete_enumeration(&mut memory, request.continuation, 0, None)
            .unwrap();
    }
}
