use super::*;
use crate::{
    graphics::d3d8::{api::HResult, resources::SurfaceRef},
    process::windows::d3d8::DispatchError32,
};

#[test]
fn balanced_nested_buffer_locks_share_live_bytes_until_the_last_unlock() {
    let (mut runtime, mut memory, device) = runtime();
    runtime
        .device_resource_call(&mut memory, device, 23, &[device.0, 16, 0, 0, 2, DATA])
        .unwrap();
    let buffer = GuestAddress(read(&memory, DATA));
    runtime
        .resource_call(
            &mut memory,
            buffer,
            InterfaceKind::VertexBuffer,
            11,
            &[buffer.0, 0, 8, DATA + 4, 0],
        )
        .unwrap();
    let first = read(&memory, DATA + 4);
    runtime
        .resource_call(
            &mut memory,
            buffer,
            InterfaceKind::VertexBuffer,
            11,
            &[buffer.0, 2, 4, DATA + 8, 0],
        )
        .unwrap();
    let second = read(&memory, DATA + 8);
    assert_eq!(second, first + 2);
    memory.write(GuestAddress(first + 1), &[7, 8, 9]).unwrap();
    assert_eq!(read(&memory, second) & 0xffff, 0x0908);
    runtime
        .resource_call(
            &mut memory,
            buffer,
            InterfaceKind::VertexBuffer,
            12,
            &[buffer.0],
        )
        .unwrap();
    assert!(memory.read(GuestAddress(first), &mut [0]).is_ok());
    runtime
        .resource_call(
            &mut memory,
            buffer,
            InterfaceKind::VertexBuffer,
            12,
            &[buffer.0],
        )
        .unwrap();
    assert!(memory.read(GuestAddress(first), &mut [0]).is_err());
    let surface = runtime.resource(buffer, device).unwrap();
    assert_eq!(
        &runtime
            .device(device)
            .unwrap()
            .resources()
            .bytes(surface)
            .unwrap()[..5],
        &[0, 7, 8, 9, 0]
    );
}

#[test]
fn last_unlock_preflights_unmap_and_failed_publication_preserves_the_live_lock() {
    use crate::graphics::d3d8::commands::{Command, DeviceEvent};

    let (mut runtime, mut memory, device) = runtime();
    runtime
        .device_resource_call(&mut memory, device, 23, &[device.0, 16, 0, 0, 2, DATA])
        .unwrap();
    let buffer = GuestAddress(read(&memory, DATA));
    let surface = runtime.resource(buffer, device).unwrap();
    for (offset, size, output) in [(0, 16, DATA + 4), (8, 4, DATA + 8)] {
        runtime
            .resource_call(
                &mut memory,
                buffer,
                InterfaceKind::VertexBuffer,
                11,
                &[buffer.0, offset, size, output, 0],
            )
            .unwrap();
    }
    let staging = GuestAddress(read(&memory, DATA + 4));
    memory.write(staging, &[9; 16]).unwrap();
    let memory_version = memory.transaction_version();
    memory.exhaust_versions_for_test();
    runtime
        .resource_call(
            &mut memory,
            buffer,
            InterfaceKind::VertexBuffer,
            12,
            &[buffer.0],
        )
        .unwrap();
    assert_eq!(memory.transaction_version(), u64::MAX);
    assert_eq!(read(&memory, staging.0), 0x0909_0909);
    let backend = runtime.device(device).unwrap();
    let version = backend.resources().version(surface.resource).unwrap();
    assert_eq!(version, 2);
    let original = backend.resources().bytes(surface).unwrap().to_vec();
    assert_eq!(&original[..8], &[0; 8]);
    assert_eq!(&original[8..12], &[9; 4]);
    let queue = backend
        .queue()
        .prepare(&[Command::DeviceEvent {
            generation: backend.generation(),
            event: DeviceEvent::Lost,
        }])
        .unwrap();
    assert!(matches!(
        runtime.resource_call(
            &mut memory,
            buffer,
            InterfaceKind::VertexBuffer,
            12,
            &[buffer.0],
        ),
        Err(DispatchError32::Api(HResult::INVALID_CALL))
    ));
    let backend = runtime.device(device).unwrap();
    assert!(backend.queue().can_commit(&queue).is_ok());
    assert_eq!(backend.resources().version(surface.resource), Ok(version));
    assert_eq!(
        backend.resources().bytes(surface).unwrap(),
        original.as_slice()
    );
    assert_eq!(backend.resources().is_locked(surface.resource), Ok(true));
    assert!(runtime.locks.contains_resource(surface.resource));
    assert_eq!(read(&memory, staging.0), 0x0909_0909);
    memory.restore_version_for_test(memory_version);
    runtime
        .resource_call(
            &mut memory,
            buffer,
            InterfaceKind::VertexBuffer,
            12,
            &[buffer.0],
        )
        .unwrap();
    assert_eq!(memory.transaction_version(), memory_version + 1);
    assert!(memory.read(staging, &mut [0]).is_err());
    let resources = runtime.device(device).unwrap().resources();
    assert_eq!(resources.version(surface.resource), Ok(version + 1));
    assert_eq!(resources.bytes(surface).unwrap(), &[9; 16]);
    assert_eq!(resources.is_locked(surface.resource), Ok(false));
    assert!(!runtime.locks.contains_resource(surface.resource));
}

#[test]
fn buffer_lock_publication_failure_is_neutral_and_unlock_reclaims_without_address_reuse() {
    let (mut runtime, mut memory, device) = runtime();
    runtime
        .device_resource_call(&mut memory, device, 23, &[device.0, 16, 0, 0, 2, DATA])
        .unwrap();
    let buffer = GuestAddress(read(&memory, DATA));
    let surface = runtime.resource(buffer, device).unwrap();
    assert!(matches!(
        runtime.resource_call(
            &mut memory,
            buffer,
            InterfaceKind::VertexBuffer,
            11,
            &[buffer.0, 1, 3, 0x8000, 0]
        ),
        Err(DispatchError32::Api(HResult::INVALID_CALL))
    ));
    assert!(
        !runtime
            .device(device)
            .unwrap()
            .resources()
            .is_locked(surface.resource)
            .unwrap()
    );
    runtime
        .resource_call(
            &mut memory,
            buffer,
            InterfaceKind::VertexBuffer,
            11,
            &[buffer.0, 1, 3, DATA + 4, 0],
        )
        .unwrap();
    let first = read(&memory, DATA + 4);
    memory.write(GuestAddress(first), &[7, 8, 9]).unwrap();
    runtime
        .resource_call(
            &mut memory,
            buffer,
            InterfaceKind::VertexBuffer,
            12,
            &[buffer.0],
        )
        .unwrap();
    assert_eq!(
        runtime
            .device(device)
            .unwrap()
            .resources()
            .bytes(surface)
            .unwrap()[..5],
        [0, 7, 8, 9, 0]
    );
    assert!(memory.read(GuestAddress(first), &mut [0]).is_err());
    runtime
        .resource_call(
            &mut memory,
            buffer,
            InterfaceKind::VertexBuffer,
            11,
            &[buffer.0, 0, 1, DATA + 4, 0],
        )
        .unwrap();
    assert!(read(&memory, DATA + 4) > first);
    let version = runtime
        .device(device)
        .unwrap()
        .resources()
        .version(surface.resource)
        .unwrap();
    let staging = GuestAddress(read(&memory, DATA + 4));
    memory.unmap(PageRange::new(staging, 1).unwrap()).unwrap();
    memory
        .map_zeroed(PageRange::new(staging, 1).unwrap(), Permissions::READ_WRITE)
        .unwrap();
    assert!(
        runtime
            .resource_call(
                &mut memory,
                buffer,
                InterfaceKind::VertexBuffer,
                12,
                &[buffer.0]
            )
            .is_err()
    );
    assert_eq!(
        runtime
            .device(device)
            .unwrap()
            .resources()
            .version(surface.resource)
            .unwrap(),
        version
    );
}

#[test]
fn gpu_written_lock_requests_real_readback_before_exposing_guest_memory() {
    let (mut runtime, mut memory, device) = runtime();
    runtime
        .device_resource_call(&mut memory, device, 25, &[device.0, 4, 4, 21, 0, 1, DATA])
        .unwrap();
    let pointer = GuestAddress(read(&memory, DATA));
    let surface: SurfaceRef = runtime.resource(pointer, device).unwrap();
    runtime
        .device_mut(device)
        .unwrap()
        .resources_mut()
        .unwrap()
        .mark_gpu_written(surface)
        .unwrap();
    memory.write(GuestAddress(DATA + 16), &[0xa5; 8]).unwrap();
    let Err(DispatchError32::LockReadback { id }) = runtime.resource_call(
        &mut memory,
        pointer,
        InterfaceKind::Surface,
        9,
        &[pointer.0, DATA + 16, 0, 0],
    ) else {
        panic!("expected an identified lock readback")
    };
    let backend = runtime.device(device).unwrap();
    assert_eq!(id.epoch, backend.queue().epoch());
    let (generation, kind, source, expected) = backend.pending_readback_details(id).unwrap();
    assert_eq!(generation, backend.generation());
    assert_eq!(kind, crate::graphics::d3d8::commands::ReadbackKind::Lock);
    assert_eq!(
        source,
        crate::graphics::d3d8::commands::SurfaceVersion {
            surface,
            version: 2
        }
    );
    assert_eq!(expected, source);
    let layout = backend.resources().info(expected.surface).unwrap();
    assert_eq!((layout.width, layout.height, layout.length), (4, 4, 64));
    assert_eq!(read(&memory, DATA + 16), 0xa5a5_a5a5);
    assert!(
        !runtime
            .device(device)
            .unwrap()
            .resources()
            .is_locked(surface.resource)
            .unwrap()
    );
}
