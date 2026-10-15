use super::super::super::state::StateBlockKind;
use super::*;

#[test]
fn prepared_bindings_track_live_recorded_and_captured_holds_independently() {
    let mut device = device();
    let binding = device
        .prepare_binding(Binding::Texture {
            stage: 0,
            handle: 17,
        })
        .unwrap();
    assert_eq!(
        (binding.released_resource(), binding.retained_resource()),
        (None, Some(17))
    );
    device.commit_binding(binding).unwrap();
    device.begin_state_block().unwrap();
    let binding = device
        .prepare_binding(Binding::Texture {
            stage: 0,
            handle: 23,
        })
        .unwrap();
    assert_eq!(binding.released_resource(), None);
    device.commit_binding(binding).unwrap();
    assert_eq!(device.state().snapshot().textures[0], 17);
    assert_eq!(
        device.retained_resource_handles().collect::<Vec<_>>(),
        [17, 23]
    );
    let block = device.end_state_block().unwrap();
    let apply = device.prepare_apply_state_block(block).unwrap();
    assert_eq!(apply.released_resources(), [17]);
    assert_eq!(apply.retained_resources(), [23]);
    device.commit_block_change(apply).unwrap();
    assert_eq!(
        device.retained_resource_handles().collect::<Vec<_>>(),
        [23, 23]
    );
}

#[test]
fn deleted_shader_remains_available_to_captured_state_until_ordered_retirement() {
    let mut device = device();
    let shader = device
        .create_vertex_shader(&DECLARATION, Some(&VERTEX), 0)
        .unwrap()
        .value;
    drain(&mut device);
    device.bind_vertex_shader(shader).unwrap();
    let block = device.create_state_block(StateBlockKind::Vertex).unwrap();
    device.delete_vertex_shader(shader).unwrap();
    assert_eq!(device.state().snapshot().vertex_shader, 0);
    assert!(device.shaders().vertex(shader).is_err());
    assert_eq!(device.retired_shaders.len(), 1);
    assert_eq!(device.seal_batch().unwrap(), None);
    device.flush_retired_shaders().unwrap();
    assert_eq!(device.retired_shaders.len(), 1);
    device.apply_state_block(block).unwrap();
    assert_eq!(device.state().snapshot().vertex_shader, shader);
    device.delete_state_block(block).unwrap();
    device.bind_vertex_shader(2).unwrap();
    device.flush_retired_shaders().unwrap();
    assert!(device.retired_shaders.is_empty());
}

#[test]
fn host_sealing_retires_deleted_shaders_without_draw_or_present() {
    use crate::graphics::d3d8::shader::registry::{RegistryError, ShaderLimits};
    let limits = DeviceLimits {
        shaders: ShaderLimits {
            objects: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut device = Device::new(1, description(), limits).unwrap();
    drain(&mut device);
    for _ in 0..3 {
        let prepared = device
            .prepare_vertex_shader(&DECLARATION, Some(&VERTEX), 0)
            .unwrap();
        let shader = prepared.handle();
        device.commit_shader(prepared).unwrap();
        drain(&mut device);
        device.delete_vertex_shader(shader).unwrap();
        assert!(matches!(
            device.prepare_vertex_shader(&DECLARATION, Some(&VERTEX), 0),
            Err(DeviceError::Shader(RegistryError::ObjectCapacity))
        ));
        let batch = device.seal_batch().unwrap().unwrap();
        let (_, bytes) = device.queue().peek().unwrap();
        let (_, fragments) = crate::abi::graphics::d3d8::packet(bytes).unwrap();
        let fragments: Vec<_> = fragments.collect();
        assert_eq!(fragments.len(), 1);
        assert_eq!(fragments[0].opcode, 5);
        assert_eq!(fragments[0].bytes, shader.to_le_bytes());
        device.acknowledge_batch(batch).unwrap();
        assert!(device.retired_shaders.is_empty());
    }
    let pixel = [0xffff_0101, 1, 0x800f_0000, 0x90e4_0000, 0xffff];
    let prepared = device.prepare_pixel_shader(&pixel).unwrap();
    let shader = prepared.handle();
    device.commit_shader(prepared).unwrap();
    drain(&mut device);
    device.delete_pixel_shader(shader).unwrap();
    drain(&mut device);
    assert!(device.prepare_pixel_shader(&pixel).is_ok());
}

#[test]
fn recording_binding_receipt_cannot_commit_after_end_recording() {
    let mut device = device();
    device.begin_state_block().unwrap();
    let prepared = device
        .prepare_binding(Binding::Texture {
            stage: 0,
            handle: 3,
        })
        .unwrap();
    device.end_state_block().unwrap();
    assert_eq!(
        device.commit_binding(prepared),
        Err(DeviceError::InvalidCall)
    );
    assert_eq!(device.state().snapshot().textures[0], 0);
}
