use super::*;

#[test]
fn prepared_creations_are_neutral_until_commit_and_stale_receipts_fail() {
    let mut device = device();
    let first = device
        .prepare_resource(buffer(64, 2, Pool::Managed))
        .unwrap();
    let same = device
        .prepare_resource(buffer(64, 2, Pool::Managed))
        .unwrap();
    let id = first.id();
    assert_eq!(id, same.id());
    assert!(device.resources().desc(id).is_err());
    assert!(device.queue().peek().is_none());
    device.commit_resource(first).unwrap();
    assert!(device.can_commit_resource(&same).is_err());
    assert_eq!(
        device.resources().desc(id).unwrap(),
        buffer(64, 2, Pool::Managed)
    );
}

#[test]
fn reset_replaces_implicit_resources_but_preserves_managed_resources_and_shader_handles() {
    let mut device = device();
    let managed = device
        .create_resource(buffer(64, 2, Pool::Managed))
        .unwrap()
        .value;
    let shader = device
        .create_vertex_shader(&DECLARATION, Some(&VERTEX), 0)
        .unwrap()
        .value;
    drain(&mut device);
    let old = device.back_buffer(0).unwrap();
    let mut next = description();
    next.width = 8;
    let reset = device.prepare_reset(next).unwrap();
    let replacement = reset.back_buffers()[0];
    assert_eq!(device.back_buffer(0).unwrap(), old);
    assert_ne!(old, replacement);
    device.commit_reset(reset).unwrap();
    assert_eq!(device.generation(), 2);
    assert_eq!(device.back_buffer(0).unwrap(), replacement);
    assert!(device.resources().desc(old.resource).is_err());
    assert!(device.resources().desc(managed).is_ok());
    assert!(device.shaders().vertex(shader).is_ok());
    assert_eq!(device.state().snapshot().vertex_shader, 0);
}

#[test]
fn reset_rejects_explicit_default_resources_without_mutating_device() {
    let mut device = device();
    let id = device
        .create_resource(buffer(64, 2, Pool::Default))
        .unwrap()
        .value;
    drain(&mut device);
    assert!(device.prepare_reset(description()).is_err());
    assert_eq!(device.generation(), 1);
    device.destroy_resource(id).unwrap();
    drain(&mut device);
    device.reset(description()).unwrap();
}

#[test]
fn destruction_and_close_are_prepared_before_resource_lifetime_changes() {
    let mut device = device();
    let id = device
        .create_resource(buffer(64, 2, Pool::Managed))
        .unwrap()
        .value;
    drain(&mut device);
    let destruction = device.prepare_destroy_resources(&[id]).unwrap();
    assert!(device.resources().desc(id).is_ok());
    device.commit_destruction(destruction).unwrap();
    assert!(device.resources().desc(id).is_err());
    let close = device.prepare_close().unwrap();
    assert_eq!(device.status(), DeviceStatus::Operational);
    device.commit_close(close).unwrap();
    assert_eq!(device.status(), DeviceStatus::Removed);
    assert!(device.seal_batch().unwrap().is_some());
}

#[test]
fn reset_invalidates_prepared_bindings_even_when_state_revision_repeats() {
    let mut device = device();
    let prepared = device
        .prepare_binding(Binding::Texture {
            stage: 0,
            handle: 12,
        })
        .unwrap();
    device.reset(description()).unwrap();
    assert_eq!(
        device.commit_binding(prepared),
        Err(DeviceError::InvalidCall)
    );
    assert_eq!(device.state().snapshot().textures[0], 0);
}

#[test]
fn d3d8_defaults_provision_full_frames_without_eager_budget_allocation() {
    use super::super::super::resources::ResourceError;
    let small = device();
    assert_eq!(small.resources().resource_bytes(), 1024);
    let mut frame = description();
    frame.width = 1024;
    frame.height = 768;
    frame.depth = Some(Format::D24S8);
    let device = Device::new(2, frame, DeviceLimits::default()).unwrap();
    assert_eq!(device.resources().resource_bytes(), 1024 * 768 * 8);
    let mut limited = DeviceLimits::default();
    limited.graphics.resource_bytes = 1024;
    assert!(matches!(
        Device::new(3, frame, limited),
        Err(DeviceError::Resource(ResourceError::ByteCapacity))
    ));
}
