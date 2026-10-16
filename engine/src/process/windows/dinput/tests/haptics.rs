use super::support::{DATA, runtime};
use crate::{
    input::{
        dinput::{
            ControllerDescriptor32, ControllerKind32, DeviceKind32, DirectInputResult32 as H,
            InterfaceKind32,
            device::CooperativeLevel32,
            haptics::{EffectKind32, HapticCapabilities32},
        },
        source::{ControllerState32, InputSource32},
    },
    memory::{AddressSpace, GuestAddress},
    process::{
        WindowsProviderOutcome32,
        windows::dinput::{
            DirectInputRuntime32,
            memory::{put, words},
        },
    },
};

const EFFECT: u32 = DATA + 1024;
const AXES: u32 = EFFECT + 64;
const DIRECTION: u32 = EFFECT + 80;
const PAYLOAD: u32 = EFFECT + 96;
const GUID: u32 = EFFECT + 128;
const OUT: u32 = EFFECT + 148;

pub(super) fn controller() -> (
    DirectInputRuntime32,
    AddressSpace,
    GuestAddress,
    InputSource32,
) {
    let (mut runtime, mut memory, keyboard, mut source) = runtime(true);
    let parent = runtime.device(keyboard).unwrap().parent;
    let instance = [0x41; 16];
    runtime.controllers[0] = Some(ControllerDescriptor32 {
        instance,
        product: [0x42; 16],
        name: "force controller".encode_utf16().collect(),
        axes: 2,
        povs: 0,
        buttons: 2,
        kind: ControllerKind32::Joystick,
    });
    runtime.haptic_capabilities[0] = Some(HapticCapabilities32 {
        axes: 2,
        max_effects: 2,
    });
    memory.write(GuestAddress(DATA + 32), &instance).unwrap();
    runtime
        .create_device(
            &mut memory,
            parent,
            InterfaceKind32::Device8A,
            DATA + 32,
            DATA + 4,
            0,
        )
        .unwrap();
    let pointer = GuestAddress(words::<1>(&memory, DATA + 4).unwrap()[0]);
    let mut format = [0; 56];
    for (at, value) in [
        (0, 24),
        (4, 16),
        (8, 1),
        (12, 8),
        (16, 2),
        (20, DATA + 128 + 24),
        (28, 0),
        (32, 2),
        (44, 4),
        (48, 0x102),
    ] {
        put(&mut format, at, value);
    }
    memory.write(GuestAddress(DATA + 128), &format).unwrap();
    runtime.set_format(&memory, pointer, DATA + 128).unwrap();
    source
        .publish_controller(
            1,
            0,
            2_000_000,
            ControllerState32 {
                connected: true,
                ..Default::default()
            },
        )
        .unwrap();
    let device = runtime.device_mut(pointer).unwrap();
    device
        .native
        .set_cooperative(CooperativeLevel32::new(1, 9, DeviceKind32::Controller(0)).unwrap())
        .unwrap();
    device.native.acquire(&source, 0).unwrap();
    device.haptics.as_mut().unwrap().set_autocenter(false);
    (runtime, memory, pointer, source)
}

fn definition(memory: &mut AddressSpace) {
    let mut header = [0; 56];
    for (at, value) in [
        (0, 56),
        (4, 0x11),
        (8, 10_000),
        (16, 10_000),
        (20, u32::MAX),
        (28, 2),
        (32, AXES),
        (36, DIRECTION),
        (44, 4),
        (48, PAYLOAD),
    ] {
        put(&mut header, at, value);
    }
    memory.write(GuestAddress(EFFECT), &header).unwrap();
    memory
        .write_words32(&[
            crate::process::windows::dinput::memory::word(AXES, 2),
            crate::process::windows::dinput::memory::word(AXES + 4, 0x102),
            crate::process::windows::dinput::memory::word(DIRECTION, 1),
            crate::process::windows::dinput::memory::word(DIRECTION + 4, 0),
            crate::process::windows::dinput::memory::word(PAYLOAD, 4000),
        ])
        .unwrap();
    memory
        .write(GuestAddress(GUID), &EffectKind32::Constant.guid())
        .unwrap();
}

fn effect(
    runtime: &mut DirectInputRuntime32,
    memory: &mut AddressSpace,
    device: GuestAddress,
) -> GuestAddress {
    definition(memory);
    assert_eq!(
        runtime
            .create_effect(memory, device, &[device.0, GUID, EFFECT, OUT, 0])
            .unwrap(),
        0
    );
    GuestAddress(words::<1>(memory, OUT).unwrap()[0])
}

#[test]
fn force_effect_com_parameters_playback_and_parent_lifetime_are_real() {
    let (mut runtime, mut memory, device, _) = controller();
    let effect = effect(&mut runtime, &mut memory, device);
    assert_eq!(
        runtime.object(effect).unwrap().interface,
        InterfaceKind32::Effect8
    );
    assert_eq!(runtime.com.references(device).unwrap(), 2);
    assert_eq!(runtime.force_device(device).unwrap().load(), 50);
    runtime
        .effect_call(&mut memory, effect, 7, &[effect.0, 1, 0])
        .unwrap();
    assert_eq!(
        runtime
            .force_device_mut(device)
            .unwrap()
            .evaluate(1000, &[0; 32], &[0; 16], true)[0],
        4000
    );
    memory
        .write(GuestAddress(PAYLOAD), &7000u32.to_le_bytes())
        .unwrap();
    runtime
        .effect_call(&mut memory, effect, 6, &[effect.0, EFFECT, 0x80000100])
        .unwrap();
    assert_eq!(
        runtime
            .force_device_mut(device)
            .unwrap()
            .evaluate(1000, &[0; 32], &[0; 16], true)[0],
        4000
    );
    assert_eq!(
        runtime
            .effect_call(&mut memory, effect, 10, &[effect.0])
            .unwrap(),
        1
    );
    assert_eq!(
        runtime
            .force_device_mut(device)
            .unwrap()
            .evaluate(1000, &[0; 32], &[0; 16], true)[0],
        7000
    );
    memory
        .write(GuestAddress(PAYLOAD), &0u32.to_le_bytes())
        .unwrap();
    runtime
        .effect_call(&mut memory, effect, 5, &[effect.0, EFFECT, 0x100])
        .unwrap();
    assert_eq!(words::<1>(&memory, PAYLOAD).unwrap(), [7000]);
    assert_eq!(runtime.release(&mut memory, None, device).unwrap(), 1);
    assert_eq!(runtime.release(&mut memory, None, effect).unwrap(), 0);
    assert!(runtime.com.lookup(device).is_err());
    assert!(runtime.com.lookup(effect).is_err());
}

#[test]
fn invalid_effect_publication_and_scatter_output_leave_owned_state_unchanged() {
    let (mut runtime, mut memory, device, _) = controller();
    definition(&mut memory);
    let before = runtime.com.references(device).unwrap();
    assert!(
        runtime
            .create_effect(
                &mut memory,
                device,
                &[device.0, GUID, EFFECT, 0xdead0000, 0]
            )
            .is_err()
    );
    assert_eq!(runtime.force_device(device).unwrap().len(), 0);
    assert_eq!(runtime.com.references(device).unwrap(), before);
    let effect = effect(&mut runtime, &mut memory, device);
    let mut header = [0; 56];
    memory.read(GuestAddress(EFFECT), &mut header).unwrap();
    put(&mut header, 48, 0xdead0000);
    memory.write(GuestAddress(EFFECT), &header).unwrap();
    memory.write(GuestAddress(AXES), &[0xcc; 8]).unwrap();
    memory.write(GuestAddress(DIRECTION), &[0xdd; 8]).unwrap();
    assert!(
        runtime
            .effect_call(&mut memory, effect, 5, &[effect.0, EFFECT, 0x165])
            .is_err()
    );
    assert_eq!(words::<2>(&memory, AXES).unwrap(), [0xcccccccc; 2]);
    assert_eq!(words::<2>(&memory, DIRECTION).unwrap(), [0xdddddddd; 2]);
    let mut after = [0; 56];
    memory.read(GuestAddress(EFFECT), &mut after).unwrap();
    assert_eq!(after, header);
}

#[test]
fn created_effect_enumeration_retains_real_identity_until_callback_completion() {
    let (mut runtime, mut memory, device, _) = controller();
    let effect = effect(&mut runtime, &mut memory, device);
    let outcome = runtime
        .enumerate_effects(
            &mut memory,
            device,
            InterfaceKind32::Device8A,
            &[device.0, 0x9000, 0x55, 0],
            true,
        )
        .unwrap();
    let WindowsProviderOutcome32::Callback(callback) = outcome else {
        panic!("expected effect callback")
    };
    assert_eq!(&callback.arguments[..2], &[effect.0, 0x55]);
    assert_eq!(runtime.com.references(effect).unwrap(), 2);
    assert_eq!(runtime.release(&mut memory, None, effect).unwrap(), 1);
    assert_eq!(
        runtime
            .complete_enumeration(&mut memory, callback.continuation, 0, None)
            .unwrap(),
        WindowsProviderOutcome32::Return(0)
    );
    assert!(runtime.com.lookup(effect).is_err());
    assert_eq!(runtime.force_device(device).unwrap().len(), 0);
    assert_eq!(runtime.com.references(device).unwrap(), 1);
}

#[test]
fn ordinary_keyboard_and_legacy_effect_catalog_keep_distinct_capabilities() {
    let (runtime, _, keyboard, _) = runtime(true);
    assert_eq!(runtime.force_device(keyboard).err(), Some(H::UNSUPPORTED));
    assert_eq!(
        InterfaceKind32::EffectLegacy.iid(),
        InterfaceKind32::Effect8.iid()
    );
    assert!(!InterfaceKind32::Effect8.compatible(InterfaceKind32::EffectLegacy));
    assert!(!InterfaceKind32::Effect8.device());
}

#[test]
fn force_feedback_device_info_uses_a_complete_driver_guid_for_ansi_and_unicode() {
    let (runtime, mut memory, pointer, _) = controller();
    let expected = [
        0x52, 0x33, 0x46, 0x46, 0x08, 0, 0x40, 0x40, 0x80, 0, 0, 0, 0, 0, 0, 1,
    ];
    for (interface, size, minimum) in [
        (InterfaceKind32::Device8A, 580u32, 560usize),
        (InterfaceKind32::Device8W, 1100u32, 1080usize),
    ] {
        memory
            .write(GuestAddress(DATA), &size.to_le_bytes())
            .unwrap();
        assert_eq!(
            runtime.device_info(&mut memory, pointer, DATA, interface),
            Ok(0)
        );
        let mut guid = [0; 16];
        memory
            .read(GuestAddress(DATA + minimum as u32), &mut guid)
            .unwrap();
        assert_eq!(guid, expected);
        assert_eq!(
            words::<1>(&memory, DATA + minimum as u32 + 16).unwrap()[0],
            0x0004_0001
        );
    }
    assert_eq!(
        runtime
            .instance_bytes(DeviceKind32::Keyboard, false, true, 580)
            .unwrap()[560..576],
        [0; 16]
    );
}
