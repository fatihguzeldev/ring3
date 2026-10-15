use super::support::*;
use crate::{
    input::dinput::DirectInputResult32, memory::GuestAddress, process::windows::dinput::memory::put,
};
#[test]
fn invalid_custom_format_does_not_replace_prior_bindings() {
    let (mut runtime, mut memory, pointer, _) = runtime(true);
    keyboard_format(&mut runtime, &mut memory, pointer);
    let mut malformed = [0; 40];
    for (at, value) in [
        (0, 24),
        (4, 16),
        (8, 1),
        (12, 8),
        (16, 1),
        (20, DATA + 256 + 24),
        (28, 8),
        (32, 0x1e04),
    ] {
        put(&mut malformed, at, value);
    }
    memory.write(GuestAddress(DATA + 256), &malformed).unwrap();
    assert_eq!(
        runtime.set_format(&memory, pointer, DATA + 256),
        Err(DirectInputResult32::INVALID_PARAM)
    );
    assert_eq!(
        runtime
            .device(pointer)
            .unwrap()
            .native
            .format()
            .unwrap()
            .size,
        8
    );
    assert_eq!(
        runtime
            .device(pointer)
            .unwrap()
            .native
            .format()
            .unwrap()
            .bindings()[0]
            .offset,
        3
    );
}

#[test]
fn action_mapping_builds_custom_offsets_and_appdata_without_consuming_physical_state() {
    let (mut runtime, mut memory, pointer, mut source) = runtime(true);
    let mut header = [0; 328];
    for (at, value) in [
        (0, 328),
        (4, 40),
        (8, 8),
        (12, 2),
        (16, DATA + 1536),
        (40, 4),
    ] {
        put(&mut header, at, value);
    }
    memory.write(GuestAddress(DATA + 1024), &header).unwrap();
    let mut actions = [0; 80];
    for (at, value) in [(0, 0xabc), (4, 0x8100_041e), (40, 0xdef), (44, 0x8100_041f)] {
        put(&mut actions, at, value);
    }
    memory.write(GuestAddress(DATA + 1536), &actions).unwrap();
    let args = [pointer.0, DATA + 1024, 0, 0];
    runtime
        .action_map(
            &mut memory,
            pointer,
            &args,
            crate::input::dinput::InterfaceKind32::Device8A,
            false,
        )
        .unwrap();
    let mapped =
        crate::process::windows::dinput::memory::words::<10>(&memory, DATA + 1536).unwrap();
    assert_eq!(mapped[8], 0x1e04);
    assert_eq!(mapped[9], 0x20);
    assert_eq!(mapped[0], 0xabc);
    runtime
        .action_map(
            &mut memory,
            pointer,
            &args,
            crate::input::dinput::InterfaceKind32::Device8A,
            true,
        )
        .unwrap();
    let native = &mut runtime.device_mut(pointer).unwrap().native;
    native
        .set_cooperative(
            crate::input::dinput::device::CooperativeLevel32::new(
                0,
                10,
                crate::input::dinput::DeviceKind32::Keyboard,
            )
            .unwrap(),
        )
        .unwrap();
    native.acquire(&source, 0).unwrap();
    source
        .publish_physical(
            [crate::input::TimedInputEvent {
                timestamp_ns: 2_000_000,
                kind: crate::input::InputKind::Key {
                    code: 30,
                    pressed: true,
                },
            }]
            .into_iter(),
        )
        .unwrap();
    native.synchronize(&source, 0, 0);
    let mut state = [0; 8];
    native.read_state(&mut state).unwrap();
    assert_eq!(&state[..4], &0x80u32.to_le_bytes());
    let mut events = [crate::input::dinput::device::DeviceEvent32::default(); 4];
    assert_eq!(native.read_buffer(&mut events).unwrap().0, 1);
    assert_eq!(events[0].app_data, 0xabc);
    assert_eq!(events[0].offset, 0);
}

#[test]
fn action_record_at_address_space_end_is_rejected_without_wrapping_or_mutation() {
    let (mut runtime, mut memory, pointer, _) = runtime(true);
    let mut header = [0; 328];
    for (at, value) in [(0, 328), (4, 40), (8, 4), (12, 1), (16, 0xffff_fff0)] {
        put(&mut header, at, value);
    }
    memory.write(GuestAddress(DATA + 1024), &header).unwrap();
    memory
        .map_zeroed(
            crate::memory::PageRange::new(GuestAddress(0xffff_f000), 1).unwrap(),
            crate::memory::Permissions::READ_WRITE,
        )
        .unwrap();
    let version = memory.transaction_version();
    assert!(
        runtime
            .action_map(
                &mut memory,
                pointer,
                &[pointer.0, DATA + 1024, 0, 0],
                crate::input::dinput::InterfaceKind32::Device8A,
                false
            )
            .is_err()
    );
    assert_eq!(memory.transaction_version(), version);
    assert!(runtime.device(pointer).unwrap().native.format().is_none());
}

#[test]
fn mouse_axis_action_map_build_preserve_and_set_use_the_same_object_type() {
    let (mut runtime, mut memory, keyboard, _) = runtime(true);
    let parent = runtime.device(keyboard).unwrap().parent;
    memory
        .write(
            GuestAddress(DATA + 64),
            &crate::input::dinput::api::SYSTEM_MOUSE,
        )
        .unwrap();
    runtime
        .create_device(
            &mut memory,
            parent,
            crate::input::dinput::InterfaceKind32::Device8A,
            DATA + 64,
            DATA + 80,
            0,
        )
        .unwrap();
    let [mouse] = crate::process::windows::dinput::memory::words(&memory, DATA + 80).unwrap();
    let mouse = GuestAddress(mouse);
    let mut header = [0; 328];
    for (at, value) in [
        (0, 328),
        (4, 40),
        (8, 4),
        (12, 1),
        (16, DATA + 1536),
        (40, 4),
    ] {
        put(&mut header, at, value);
    }
    memory.write(GuestAddress(DATA + 1024), &header).unwrap();
    let mut action = [0; 40];
    put(&mut action, 4, 0x8200_0300);
    memory.write(GuestAddress(DATA + 1536), &action).unwrap();
    let mut args = [mouse.0, DATA + 1024, 0, 0];
    runtime
        .action_map(
            &mut memory,
            mouse,
            &args,
            crate::input::dinput::InterfaceKind32::Device8A,
            false,
        )
        .unwrap();
    let [object] =
        crate::process::windows::dinput::memory::words(&memory, DATA + 1536 + 32).unwrap();
    assert_eq!(object, 1);
    args[3] = 1;
    runtime
        .action_map(
            &mut memory,
            mouse,
            &args,
            crate::input::dinput::InterfaceKind32::Device8A,
            false,
        )
        .unwrap();
    args[3] = 0;
    runtime
        .action_map(
            &mut memory,
            mouse,
            &args,
            crate::input::dinput::InterfaceKind32::Device8A,
            true,
        )
        .unwrap();
    assert!(runtime.device(mouse).unwrap().native.relative());
    assert_eq!(
        runtime
            .device(mouse)
            .unwrap()
            .native
            .format()
            .unwrap()
            .bindings()[0]
            .object,
        Some(crate::input::dinput::format::DeviceObject32::Axis {
            index: 0,
            aspect: 0
        })
    );
}
