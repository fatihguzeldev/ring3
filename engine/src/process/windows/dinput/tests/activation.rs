use super::support::*;
use crate::{
    input::dinput::{DirectInputResult32 as H, InterfaceKind32, MethodGate},
    memory::GuestAddress,
    process::WindowsProviderOutcome32,
    windows::com::IID_IUNKNOWN,
};
#[test]
fn activated_input_initialization_is_version_checked_and_failure_neutral() {
    let (mut runtime, mut memory, _, source) = runtime(true);
    runtime
        .activate_class(&mut memory, &[], true, IID_IUNKNOWN, DATA + 800)
        .unwrap();
    let pointer = GuestAddress(super::super::memory::words::<1>(&memory, DATA + 800).unwrap()[0]);
    let status = MethodGate {
        interface: InterfaceKind32::Input8A,
        slot: 5,
    };
    assert_eq!(
        runtime.method(
            &mut memory,
            None,
            None,
            &source,
            status,
            &[pointer.0, DATA + 32]
        ),
        Err(H::NOT_INITIALIZED)
    );
    let initialize = MethodGate {
        interface: InterfaceKind32::Input8A,
        slot: 7,
    };
    assert_eq!(
        runtime.method(
            &mut memory,
            None,
            None,
            &source,
            initialize,
            &[pointer.0, 0x400000, 0x700]
        ),
        Err(H(0x8007_047e))
    );
    assert_eq!(
        runtime.method(
            &mut memory,
            None,
            None,
            &source,
            status,
            &[pointer.0, DATA + 32]
        ),
        Err(H::NOT_INITIALIZED)
    );
    assert_eq!(
        runtime.method(
            &mut memory,
            None,
            None,
            &source,
            initialize,
            &[pointer.0, 0x400000, 0x800]
        ),
        Ok(WindowsProviderOutcome32::Return(0))
    );
    assert_eq!(
        runtime.method(
            &mut memory,
            None,
            None,
            &source,
            status,
            &[pointer.0, DATA + 32]
        ),
        Ok(WindowsProviderOutcome32::Return(0))
    );
    assert_eq!(
        runtime.method(
            &mut memory,
            None,
            None,
            &source,
            initialize,
            &[pointer.0, 0x400000, 0x800]
        ),
        Ok(WindowsProviderOutcome32::Return(0))
    );
}

#[test]
fn cooperative_window_may_belong_to_another_live_thread_in_the_same_process() {
    let (mut runtime, mut memory, pointer, source) = runtime(true);
    let mut windows = crate::windows::window::WindowContext32::new(2, 8).unwrap();
    let window = windows
        .create(crate::windows::window::WindowDescription32 {
            thread_id: 2,
            procedure: GuestAddress(0x9000),
            extent: crate::windows::window::WindowExtent32 {
                width: 64,
                height: 64,
            },
            visible: true,
            enabled: true,
        })
        .unwrap();
    let gate = MethodGate {
        interface: InterfaceKind32::Device8A,
        slot: 13,
    };
    assert_eq!(
        runtime
            .method(
                &mut memory,
                Some(&windows),
                None,
                &source,
                gate,
                &[pointer.0, window.get(), 6]
            )
            .unwrap(),
        WindowsProviderOutcome32::Return(0)
    );
    assert_eq!(
        runtime
            .device(pointer)
            .unwrap()
            .native
            .cooperative()
            .unwrap()
            .window,
        window.get()
    );
    assert_eq!(
        runtime.method(
            &mut memory,
            Some(&windows),
            None,
            &source,
            gate,
            &[pointer.0, 0xdead, 6]
        ),
        Err(H::HANDLE)
    );
    assert_eq!(
        runtime
            .device(pointer)
            .unwrap()
            .native
            .cooperative()
            .unwrap()
            .window,
        window.get()
    );
}

#[test]
fn automatically_initialized_device_checks_version_guid_and_acquisition_without_fake_success() {
    let (mut runtime, mut memory, pointer, source) = runtime(true);
    let gate = MethodGate {
        interface: InterfaceKind32::Device8A,
        slot: 17,
    };
    memory
        .write(
            GuestAddress(DATA + 64),
            &crate::input::dinput::api::SYSTEM_KEYBOARD,
        )
        .unwrap();
    let call = |runtime: &mut super::super::DirectInputRuntime32,
                memory: &mut crate::memory::AddressSpace,
                version,
                guid| {
        runtime.method(
            memory,
            None,
            None,
            &source,
            gate,
            &[pointer.0, 0x400000, version, guid],
        )
    };
    assert_eq!(
        call(&mut runtime, &mut memory, 0x800, DATA + 64),
        Ok(WindowsProviderOutcome32::Return(1))
    );
    assert_eq!(
        call(&mut runtime, &mut memory, 0x700, DATA + 64),
        Err(H(0x8007047e))
    );
    memory.write(GuestAddress(DATA + 80), &[0xee; 16]).unwrap();
    assert_eq!(
        call(&mut runtime, &mut memory, 0x800, DATA + 80),
        Err(H::DEVICE_NOT_REGISTERED)
    );
    keyboard_format(&mut runtime, &mut memory, pointer);
    runtime
        .device_mut(pointer)
        .unwrap()
        .native
        .acquire(&source, 0)
        .unwrap();
    assert_eq!(
        call(&mut runtime, &mut memory, 0x800, DATA + 64),
        Err(H::ACQUIRED)
    );
    assert_eq!(
        runtime.device(pointer).unwrap().kind,
        crate::input::dinput::DeviceKind32::Keyboard
    );
}
