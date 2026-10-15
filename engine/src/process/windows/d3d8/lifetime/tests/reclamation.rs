use super::*;
use crate::{
    graphics::d3d8::{device::DeviceStatus, resources::ResourceError},
    process::windows::d3d8::garbage::Garbage32,
};

#[test]
fn final_release_reclaims_locked_guest_staging_atomically_and_device_waits_for_close_ack() {
    let (mut runtime, mut memory, device) = runtime();
    runtime
        .device_resource_call(&mut memory, device, 23, &[device.0, 16, 0, 0, 2, DATA])
        .unwrap();
    let buffer = GuestAddress(read(&memory, DATA));
    let surface = runtime.resource(buffer, device).unwrap();
    runtime
        .resource_call(
            &mut memory,
            buffer,
            InterfaceKind::VertexBuffer,
            11,
            &[buffer.0, 0, 16, DATA + 4, 0],
        )
        .unwrap();
    let staging = GuestAddress(read(&memory, DATA + 4));
    assert_eq!(runtime.release(&mut memory, device), Ok(1));
    let before = memory.transaction_version();
    assert_eq!(runtime.release(&mut memory, buffer), Ok(0));
    assert_eq!(memory.transaction_version(), before + 1);
    assert!(memory.read(staging, &mut [0]).is_err());
    assert!(memory.read(buffer, &mut [0]).is_err());
    assert!(memory.read(device, &mut [0]).is_err());
    assert!(!runtime.locks.contains_resource(surface.resource));
    assert_eq!(runtime.retired_devices.len(), 1);
    assert!(
        !runtime.retired_devices[0]
            .device
            .backend
            .resources()
            .is_locked(surface.resource)
            .unwrap()
    );
    runtime.collect_garbage().unwrap();
    let retired = &mut runtime.retired_devices[0];
    assert!(retired.closed);
    assert_eq!(retired.device.backend.status(), DeviceStatus::Removed);
    assert!(
        retired
            .device
            .backend
            .resources()
            .desc(surface.resource)
            .is_ok()
    );
    let packet = retired.device.backend.seal_batch().unwrap().unwrap();
    assert!(retired.device.backend.queue().is_sealed());
    retired.device.backend.acknowledge_batch(packet).unwrap();
    assert_eq!(runtime.retired_devices.len(), 1);
}

#[test]
fn private_namespace_collection_works_while_device_is_lost_and_ignores_reclaimed_backings() {
    let (mut runtime, mut memory, device) = runtime();
    runtime
        .device_resource_call(
            &mut memory,
            device,
            20,
            &[device.0, 4, 4, 1, 0, 21, 2, DATA],
        )
        .unwrap();
    let texture = GuestAddress(read(&memory, DATA));
    let id = runtime.resource(texture, device).unwrap().resource;
    runtime
        .device_mut(device)
        .unwrap()
        .metadata_mut()
        .set_private_data_for(id, 77, [1; 16], &[3])
        .unwrap();
    let generation = runtime.device(device).unwrap().generation();
    runtime
        .device_mut(device)
        .unwrap()
        .observe_status(generation, DeviceStatus::Lost)
        .unwrap();
    runtime.garbage.push(Garbage32::Namespace {
        device,
        resource: id,
        namespace: 77,
    });
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime
            .device(device)
            .unwrap()
            .resources()
            .private_data_for(id, 77, [1; 16]),
        Err(ResourceError::NotFound)
    );
    let stale = ResourceId {
        generation: id.generation + 1,
        ..id
    };
    runtime.garbage.push(Garbage32::Namespace {
        device,
        resource: stale,
        namespace: 77,
    });
    runtime.collect_garbage().unwrap();
    assert!(runtime.garbage.is_empty());
}
