use crate::{
    input::{
        InputKind, TimedInputEvent,
        dinput::{FactoryGate, InterfaceKind32, api::SYSTEM_KEYBOARD},
        source::InputSource32,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{
        WindowsProviderOutcome32,
        windows::dinput::{
            DirectInputConfig32, DirectInputRuntime32,
            memory::{put, words},
        },
    },
};
pub(super) const DATA: u32 = 0x2000;
pub(super) fn runtime(
    version8: bool,
) -> (
    DirectInputRuntime32,
    AddressSpace,
    GuestAddress,
    InputSource32,
) {
    let config = DirectInputConfig32 {
        objects: PageRange::new(GuestAddress(0x1000_0000), 256).unwrap(),
        scratch: PageRange::new(GuestAddress(0x2000_0000), 32).unwrap(),
        max_objects: 32,
        max_devices: 8,
        max_buffer_events: 32,
    };
    let mut runtime = DirectInputRuntime32::new(config).unwrap();
    for interface in InterfaceKind32::ALL {
        runtime.methods[interface as usize - 1] =
            vec![GuestAddress(0x9000); interface.words().len()];
    }
    let mut memory = AddressSpace::new(512).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(DATA), 2).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x9000), 1).unwrap(),
            Permissions::READ_EXECUTE,
        )
        .unwrap();
    let interface = if version8 {
        InterfaceKind32::Input8A
    } else {
        InterfaceKind32::Input1A
    };
    memory
        .write(GuestAddress(DATA + 16), &interface.iid().0)
        .unwrap();
    let outcome = if version8 {
        runtime.factory(
            &mut memory,
            FactoryGate::Create8,
            &[0x400000, 0x800, DATA + 16, DATA, 0],
        )
    } else {
        runtime.factory(
            &mut memory,
            FactoryGate::CreateA,
            &[0x400000, 0x700, DATA, 0],
        )
    }
    .unwrap();
    assert_eq!(outcome, WindowsProviderOutcome32::Return(0));
    let [parent] = words(&memory, DATA).unwrap();
    memory
        .write(GuestAddress(DATA + 32), &SYSTEM_KEYBOARD)
        .unwrap();
    runtime
        .create_device(
            &mut memory,
            GuestAddress(parent),
            interface.device_interface(),
            DATA + 32,
            DATA + 4,
            0,
        )
        .unwrap();
    let [pointer] = words(&memory, DATA + 4).unwrap();
    let mut source = InputSource32::new(1, 0, 512).unwrap();
    source
        .publish_physical(
            [TimedInputEvent {
                timestamp_ns: 1_000_000,
                kind: InputKind::Focus { focused: true },
            }]
            .into_iter(),
        )
        .unwrap();
    (runtime, memory, GuestAddress(pointer), source)
}
pub(super) fn keyboard_format(
    runtime: &mut DirectInputRuntime32,
    memory: &mut AddressSpace,
    pointer: GuestAddress,
) {
    let mut bytes = [0; 56];
    for (at, value) in [
        (0, 24),
        (4, 16),
        (8, 2),
        (12, 8),
        (16, 2),
        (20, DATA + 128 + 24),
        (24, 0),
        (28, 3),
        (32, 0x1e04),
        (36, 0),
        (40, 0),
        (44, 6),
        (48, 0x1f04),
        (52, 0),
    ] {
        put(&mut bytes, at, value);
    }
    memory.write(GuestAddress(DATA + 128), &bytes).unwrap();
    runtime.set_format(memory, pointer, DATA + 128).unwrap();
    let native = &mut runtime.device_mut(pointer).unwrap().native;
    native.set_buffer_size(4).unwrap();
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
}
