use super::super::tests::support::{DATA, read, runtime};
use super::decoding::*;
use crate::{
    graphics::d3d8::api::HResult,
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use crate::{
    graphics::d3d8::{
        device::{DeviceError, DeviceStatus},
        resources::ResourceError,
    },
    process::windows::d3d8::DispatchError32,
    windows::window::{WindowContext32, WindowDescription32, WindowExtent32},
};

fn windows() -> WindowContext32 {
    let mut windows = WindowContext32::new(4, 16).unwrap();
    windows
        .create(WindowDescription32 {
            thread_id: 1,
            procedure: GuestAddress(0x9000),
            extent: WindowExtent32 {
                width: 16,
                height: 16,
            },
            visible: true,
            enabled: true,
        })
        .unwrap();
    windows
}

fn parameters(memory: &mut AddressSpace) {
    let words = [16u32, 16, 21, 1, 0, 3, 1, 1, 0, 0, 1, 0, 0];
    let bytes: Vec<_> = words.into_iter().flat_map(u32::to_le_bytes).collect();
    memory.write(GuestAddress(DATA + 128), &bytes).unwrap();
}

#[test]
fn dirty_region_preserves_explicit_empty_and_validates_header_bounds() {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(DATA), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let words = [32u32, 1, 0, 0, 0, 0, 0, 0];
    let bytes: Vec<_> = words.into_iter().flat_map(u32::to_le_bytes).collect();
    memory.write(GuestAddress(DATA), &bytes).unwrap();
    assert!(dirty_region(&memory, DATA, 1024).unwrap().is_empty());
    assert!(dirty_region(&memory, 0, 1024).unwrap().is_empty());
    memory
        .write(GuestAddress(DATA), &31u32.to_le_bytes())
        .unwrap();
    assert_eq!(
        dirty_region(&memory, DATA, 1024),
        Err(HResult::INVALID_CALL)
    );
}

#[test]
fn implicit_backbuffer_identity_is_stable_and_owns_no_external_device_cycle() {
    let (mut runtime, mut memory, device) = runtime();
    let surface = runtime.device(device).unwrap().back_buffer(0).unwrap();
    let before = runtime.com.references(device).unwrap();
    runtime
        .acquire_surface(&mut memory, device, device, surface, DATA)
        .unwrap();
    let first = GuestAddress(read(&memory, DATA));
    assert_eq!(runtime.com.references(device).unwrap(), before + 1);
    assert_eq!(runtime.release(&mut memory, first).unwrap(), 0);
    assert_eq!(runtime.com.references(device).unwrap(), before);
    assert_eq!(runtime.com.state(first).unwrap().holds, 1);
    runtime
        .acquire_surface(&mut memory, device, device, surface, DATA + 4)
        .unwrap();
    assert_eq!(read(&memory, DATA + 4), first.0);
    assert_eq!(runtime.com.references(device).unwrap(), before + 1);
}

#[test]
fn reset_discards_default_binding_holds_after_public_release_and_clears_scene() {
    let (mut runtime, mut memory, device) = runtime();
    let windows = windows();
    parameters(&mut memory);
    runtime
        .device_resource_call(&mut memory, device, 23, &[device.0, 64, 0, 2, 0, DATA])
        .unwrap();
    let buffer = GuestAddress(read(&memory, DATA));
    let id = runtime.resource(buffer, device).unwrap().resource;
    runtime
        .binding_call(&mut memory, device, 83, &[device.0, 0, buffer.0, 12])
        .unwrap();
    assert_eq!(runtime.release(&mut memory, buffer).unwrap(), 0);
    runtime.device_mut(device).unwrap().begin_scene().unwrap();
    runtime
        .reset_device(&mut memory, &windows, device, DATA + 128)
        .unwrap();
    assert_eq!(
        runtime.device(device).unwrap().status(),
        DeviceStatus::Operational
    );
    assert_eq!(
        runtime.device(device).unwrap().state().snapshot().streams[0].resource,
        0
    );
    assert!(matches!(
        runtime.device(device).unwrap().resources().desc(id),
        Err(ResourceError::StaleResource)
    ));
    assert_eq!(
        runtime.device_mut(device).unwrap().end_scene(),
        Err(DeviceError::InvalidCall)
    );
}

#[test]
fn reset_with_external_default_resource_reports_lost_and_ready_to_reset_then_recovers() {
    let (mut runtime, mut memory, device) = runtime();
    let windows = windows();
    parameters(&mut memory);
    runtime
        .device_resource_call(&mut memory, device, 23, &[device.0, 64, 0, 2, 0, DATA])
        .unwrap();
    let buffer = GuestAddress(read(&memory, DATA));
    assert!(matches!(
        runtime.reset_device(&mut memory, &windows, device, DATA + 128),
        Err(DispatchError32::Device(DeviceError::Lost))
    ));
    assert_eq!(
        runtime.device(device).unwrap().status(),
        DeviceStatus::Resettable
    );
    runtime.release(&mut memory, buffer).unwrap();
    runtime
        .reset_device(&mut memory, &windows, device, DATA + 128)
        .unwrap();
    assert_eq!(
        runtime.device(device).unwrap().status(),
        DeviceStatus::Operational
    );
}
