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
