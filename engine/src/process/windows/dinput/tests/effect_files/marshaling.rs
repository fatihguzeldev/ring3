use super::super::{haptics::controller, support::DATA};
use crate::{
    input::dinput::{
        InterfaceKind32,
        haptics::{
            EffectKind32,
            files::{FileEffect32, encode},
        },
    },
    memory::{GuestAddress, PageRange, Permissions},
    process::{WindowsProviderOutcome32, windows::dinput::memory::words},
};
pub(super) fn constant(name: &[u8]) -> FileEffect32 {
    FileEffect32 {
        guid: EffectKind32::Constant.guid(),
        name: name.to_vec(),
        flags: 0x11,
        duration: 10000,
        sample_period: 0,
        gain: 10000,
        trigger: u32::MAX,
        trigger_repeat: u32::MAX,
        axes: vec![2, 0x102],
        direction: vec![1, 0],
        envelope: None,
        type_specific: 4000i32.to_le_bytes().to_vec(),
        opaque_chunks: Vec::new(),
        start_delay: 0,
    }
}
#[test]
fn file_rows_relocate_real_parameters_and_can_create_playing_effects() {
    let (mut runtime, mut memory, device, _) = controller();
    let input = encode(&[constant(b"first"), constant(b"second")], 4096).unwrap();
    let args = [device.0, 0, 0x9000, 0x55, 0];
    let WindowsProviderOutcome32::Callback(first) = runtime
        .enumerate_effect_file(
            &mut memory,
            device,
            InterfaceKind32::Device8A,
            &args,
            &input,
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(first.arguments[1], 0x55);
    let row = first.arguments[0];
    let effect = words::<1>(&memory, row + 20).unwrap()[0];
    assert_eq!(effect, row + 284);
    assert_eq!(words::<1>(&memory, effect + 32).unwrap()[0], row + 340);
    assert_eq!(words::<1>(&memory, effect + 36).unwrap()[0], row + 348);
    runtime
        .create_effect(
            &mut memory,
            device,
            &[device.0, row + 4, effect, DATA + 512, 0],
        )
        .unwrap();
    let pointer = GuestAddress(words::<1>(&memory, DATA + 512).unwrap()[0]);
    let native = runtime.force_device_mut(device).unwrap();
    let id = native.ids().next().unwrap();
    native.start(id, 1, 0, true, 0).unwrap();
    assert_eq!(native.evaluate(0, &[0; 32], &[0; 16], true)[0], 4000);
    let WindowsProviderOutcome32::Callback(next) = runtime
        .complete_enumeration(&mut memory, first.continuation, 1, None)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(next.arguments[0], row);
    let mut name = [0; 7];
    memory.read(GuestAddress(row + 24), &mut name).unwrap();
    assert_eq!(&name, b"second\0");
    assert_eq!(
        runtime
            .complete_enumeration(&mut memory, next.continuation, 0, None)
            .unwrap(),
        WindowsProviderOutcome32::Return(0)
    );
    assert!(memory.mapping_version_at(GuestAddress(row)).is_err());
    assert_eq!(runtime.com.references(pointer).unwrap(), 1);
}
#[test]
fn modify_if_needed_converts_polar_direction_and_preserves_unknown_effect_filtering() {
    let (mut runtime, mut memory, device, _) = controller();
    runtime.device_mut(device).unwrap().native.description.axes = 1;
    let mut known = constant(b"wheel");
    known.flags = 0x21;
    known.direction = vec![9000, 0];
    let mut unknown = constant(b"vendor");
    unknown.guid = [0x51; 16];
    let input = encode(&[known, unknown], 4096).unwrap();
    let args = [device.0, 0, 0x9000, 0, 0x10];
    let WindowsProviderOutcome32::Callback(request) = runtime
        .enumerate_effect_file(
            &mut memory,
            device,
            InterfaceKind32::Device8A,
            &args,
            &input,
        )
        .unwrap()
    else {
        panic!()
    };
    let row = request.arguments[0];
    let header = words::<14>(&memory, row + 284).unwrap();
    assert_eq!(header[1], 0x11);
    assert_eq!(header[7], 1);
    assert_eq!(words::<1>(&memory, header[9]).unwrap()[0], 10000);
    assert_eq!(
        runtime
            .complete_enumeration(&mut memory, request.continuation, 1, None)
            .unwrap(),
        WindowsProviderOutcome32::Return(0)
    );
    let args = [device.0, 0, 0x9000, 0, 1];
    let WindowsProviderOutcome32::Callback(first) = runtime
        .enumerate_effect_file(
            &mut memory,
            device,
            InterfaceKind32::Device8A,
            &args,
            &input,
        )
        .unwrap()
    else {
        panic!()
    };
    let WindowsProviderOutcome32::Callback(second) = runtime
        .complete_enumeration(&mut memory, first.continuation, 1, None)
        .unwrap()
    else {
        panic!()
    };
    let mut guid = [0; 16];
    memory
        .read(GuestAddress(second.arguments[0] + 4), &mut guid)
        .unwrap();
    assert_eq!(guid, [0x51; 16]);
    runtime
        .complete_enumeration(&mut memory, second.continuation, 0, None)
        .unwrap();
}
#[test]
fn multipage_opaque_vendor_file_rows_reject_a_replaced_middle_scratch_page() {
    let (mut runtime, mut memory, device, _) = controller();
    let mut custom = constant(b"opaque vendor");
    custom.guid = [0x51; 16];
    custom.type_specific = vec![0; 16000];
    let input = encode(&[custom], 32768).unwrap();
    let args = [device.0, 0, 0x9000, 0, 1];
    let WindowsProviderOutcome32::Callback(request) = runtime
        .enumerate_effect_file(
            &mut memory,
            device,
            InterfaceKind32::Device8A,
            &args,
            &input,
        )
        .unwrap()
    else {
        panic!()
    };
    let range = runtime.pending.last().unwrap().range;
    assert_eq!(range.count, 4);
    let page = GuestAddress(request.arguments[0] + 4096);
    let middle = PageRange::new(page, 1).unwrap();
    memory.unmap(middle).unwrap();
    memory.map_zeroed(middle, Permissions::READ_WRITE).unwrap();
    memory.write(page, b"replacement").unwrap();
    assert!(
        runtime
            .complete_enumeration(&mut memory, request.continuation, 0, None)
            .is_err()
    );
    assert!(runtime.check_active_thread_scratch(&memory).is_err());
    let mut bytes = [0; 11];
    memory.read(page, &mut bytes).unwrap();
    assert_eq!(&bytes, b"replacement");
}

#[test]
fn custom_file_enumeration_preserves_raw_guest_pointer_and_consumption_owns_checked_samples() {
    use crate::input::dinput::DirectInputResult32 as H;
    let (mut runtime, mut memory, device, _) = controller();
    let mut custom = constant(b"custom metadata");
    custom.guid = EffectKind32::Custom.guid();
    let samples = 0x6000_0000u32;
    custom.type_specific = [1u32, 1000, 4, samples].map(u32::to_le_bytes).concat();
    let input = encode(&[custom], 4096).unwrap();
    let WindowsProviderOutcome32::Callback(callback) = runtime
        .enumerate_effect_file(
            &mut memory,
            device,
            InterfaceKind32::Device8A,
            &[device.0, 0, 0x9000, 0, 0],
            &input,
        )
        .unwrap()
    else {
        panic!("custom metadata callback required")
    };
    let row = callback.arguments[0];
    let effect = words::<1>(&memory, row + 20).unwrap()[0];
    let metadata = words::<1>(&memory, effect + 48).unwrap()[0];
    assert_eq!(
        words::<4>(&memory, metadata).unwrap(),
        [1, 1000, 4, samples]
    );
    let before = runtime.com.references(device).unwrap();
    memory
        .write(GuestAddress(DATA + 512), &0xfeed_beefu32.to_le_bytes())
        .unwrap();
    assert_eq!(
        runtime.create_effect(
            &mut memory,
            device,
            &[device.0, row + 4, effect, DATA + 512, 0]
        ),
        Err(H::INVALID_PARAM)
    );
    assert_eq!(words::<1>(&memory, DATA + 512).unwrap(), [0xfeed_beef]);
    assert_eq!(runtime.com.references(device).unwrap(), before);
    assert_eq!(runtime.force_device(device).unwrap().len(), 0);
    memory
        .write(GuestAddress(metadata + 8), &u32::MAX.to_le_bytes())
        .unwrap();
    let version = memory.transaction_version();
    assert_eq!(
        runtime.create_effect(
            &mut memory,
            device,
            &[device.0, row + 4, effect, DATA + 512, 0]
        ),
        Err(H::INVALID_PARAM)
    );
    assert_eq!(memory.transaction_version(), version);
    assert_eq!(runtime.com.references(device).unwrap(), before);
    assert_eq!(runtime.force_device(device).unwrap().len(), 0);
    memory
        .write(GuestAddress(metadata + 8), &4u32.to_le_bytes())
        .unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(samples), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory
        .write(
            GuestAddress(samples),
            &[4000i32, -2000, 1000, 0].map(i32::to_le_bytes).concat(),
        )
        .unwrap();
    runtime
        .create_effect(
            &mut memory,
            device,
            &[device.0, row + 4, effect, DATA + 512, 0],
        )
        .unwrap();
    let id = runtime.force_device(device).unwrap().ids().next().unwrap();
    runtime
        .force_device_mut(device)
        .unwrap()
        .start(id, 1, 0, true, 0)
        .unwrap();
    memory
        .unmap(PageRange::new(GuestAddress(samples), 1).unwrap())
        .unwrap();
    assert_eq!(
        runtime
            .force_device_mut(device)
            .unwrap()
            .evaluate(0, &[0; 32], &[0; 16], true)[0],
        4000
    );
    runtime
        .complete_enumeration(&mut memory, callback.continuation, 0, None)
        .unwrap();
}
