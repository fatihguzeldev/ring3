use super::super::super::{commands::ReadbackKind, resources::ResourceError};
use super::*;

#[test]
fn gpu_shadow_readback_requires_matching_generation_version_and_full_payload() {
    let mut device = device();
    let surface = device.back_buffer(0).unwrap();
    device.clear(1, 0, 1f32.to_bits(), 0, &[]).unwrap();
    drain(&mut device);
    assert_eq!(
        device.resources().bytes(surface),
        Err(ResourceError::ReadbackRequired)
    );
    let request = device.request_lock_readback(surface).unwrap().value;
    assert_eq!(
        device.complete_readback(request, 2, &[1; 1024]),
        Err(DeviceError::StaleReadback)
    );
    assert!(device.complete_readback(request, 1, &[1; 4]).is_err());
    assert!(device.pending_readback(request).is_some());
    assert_eq!(
        device.complete_readback(request, 1, &[1; 1024]),
        Ok(ReadbackKind::Lock)
    );
    assert_eq!(device.resources().bytes(surface).unwrap(), &[1; 1024]);
}

#[test]
fn device_loss_cancels_pending_readbacks_and_rejects_late_host_bytes() {
    for status in [DeviceStatus::Lost, DeviceStatus::Removed] {
        let mut device = device();
        let surface = device.back_buffer(0).unwrap();
        device.clear(1, 0, 1f32.to_bits(), 0, &[]).unwrap();
        drain(&mut device);
        let request = device.request_lock_readback(surface).unwrap().value;
        drain(&mut device);
        let generation = device.generation();
        device.observe_status(generation, status).unwrap();
        assert!(device.pending_readback(request).is_none());
        assert_eq!(
            device.complete_readback(request, generation, &[1; 1024]),
            Err(DeviceError::StaleReadback)
        );
        assert_eq!(
            device.resources().bytes(surface),
            Err(ResourceError::ReadbackRequired)
        );
    }
}

#[test]
fn gpu_copy_to_system_memory_waits_for_host_bytes() {
    let mut device = device();
    let source = device.back_buffer(0).unwrap();
    let destination = SurfaceRef {
        resource: device
            .create_resource(image(Pool::SystemMemory))
            .unwrap()
            .value,
        subresource: 0,
    };
    device.clear(1, 0, 1f32.to_bits(), 0, &[]).unwrap();
    drain(&mut device);
    let request = device
        .copy_rects(source, destination, &[], None)
        .unwrap()
        .value
        .unwrap();
    assert_eq!(
        device.resources().bytes(destination),
        Err(ResourceError::ReadbackRequired)
    );
    assert_eq!(
        device.complete_readback(request, 1, &[7; 1024]),
        Ok(ReadbackKind::Copy)
    );
    assert_eq!(device.resources().bytes(destination).unwrap(), &[7; 1024]);
}

#[test]
fn process_vertices_is_a_semantic_host_job_with_versioned_readback() {
    let mut device = device();
    device.bind_vertex_shader(2).unwrap();
    let source = device
        .create_resource(buffer(36, 2, Pool::Managed))
        .unwrap()
        .value;
    let destination = device
        .create_resource(buffer(48, 4, Pool::Managed))
        .unwrap()
        .value;
    drain(&mut device);
    let mut bindings = DrawBindings::default();
    bindings.streams[0] = Some(StreamResource {
        resource: device.resource_version(source).unwrap(),
        stride: 12,
    });
    let request = device
        .process_vertices(0, 0, 3, destination, 0, bindings)
        .unwrap()
        .value
        .unwrap();
    assert_eq!(
        device.complete_readback(request, 1, &[9; 48]),
        Ok(ReadbackKind::ProcessVertices)
    );
    assert_eq!(
        device
            .resources()
            .bytes(SurfaceRef {
                resource: destination,
                subresource: 0
            })
            .unwrap(),
        &[9; 48]
    );
}
