use super::*;
use crate::graphics::d3d8::{
    commands::CommandError,
    patch::{Basis, Degree, PatchInfo, RectPatchInfo, Segments, TriPatchInfo},
};

fn rectangle() -> PatchInfo {
    PatchInfo::Rectangle(RectPatchInfo {
        start_width: 0,
        start_height: 0,
        width: 4,
        height: 4,
        stride: 4,
        basis: Basis::Bezier,
        degree: Degree::Cubic,
    })
}

fn setup(device: &mut Device) -> DrawBindings {
    let resource = device
        .create_resource(buffer(16 * 12, 2, Pool::Managed))
        .unwrap()
        .value;
    device.bind_vertex_shader(2).unwrap();
    let mut bindings = DrawBindings::default();
    bindings.streams[0] = Some(StreamResource {
        resource: device.resource_version(resource).unwrap(),
        stride: 12,
    });
    device.begin_scene().unwrap();
    bindings
}

fn opcodes(device: &mut Device) -> Vec<u16> {
    let id = device.seal_batch().unwrap().unwrap();
    let (_, bytes) = device.queue().peek().unwrap();
    let (_, fragments) = crate::abi::graphics::d3d8::packet(bytes).unwrap();
    let codes = fragments.map(|fragment| fragment.opcode).collect();
    device.acknowledge_batch(id).unwrap();
    codes
}

#[test]
fn cached_patch_ignores_streams_pins_shader_and_has_ordered_disposal() {
    let mut device = device();
    let bindings = setup(&mut device);
    drain(&mut device);
    let shader = device
        .create_vertex_shader(&DECLARATION, Some(&VERTEX), 0)
        .unwrap()
        .value;
    device.bind_vertex_shader(shader).unwrap();
    drain(&mut device);
    device
        .draw_patch(7, false, Some(rectangle()), None, bindings)
        .unwrap();
    assert_eq!(opcodes(&mut device), [21, 6, 22, 23, 24]);
    device
        .draw_patch(7, false, None, None, DrawBindings::default())
        .unwrap();
    assert_eq!(opcodes(&mut device), [24]);
    device.delete_vertex_shader(shader).unwrap();
    device.flush_retired_shaders().unwrap();
    assert_eq!(device.retired_shaders.len(), 1);
    device.delete_patch(7).unwrap();
    device.flush_retired_shaders().unwrap();
    assert_eq!(opcodes(&mut device), [25, 5]);
    assert!(device.delete_patch(7).is_err());
}

#[test]
fn dynamic_patch_has_no_cache_and_replacement_uses_new_geometry_identity() {
    let mut device = device();
    let bindings = setup(&mut device);
    drain(&mut device);
    device
        .draw_patch(0, false, Some(rectangle()), None, bindings)
        .unwrap();
    assert_eq!(opcodes(&mut device), [21, 6, 23, 24]);
    assert!(device.patches.is_empty());
    assert!(device.draw_patch(0, false, None, None, bindings).is_err());
    device
        .draw_patch(9, false, Some(rectangle()), None, bindings)
        .unwrap();
    drain(&mut device);
    let triangle = PatchInfo::Triangle(TriPatchInfo {
        start: 0,
        vertices: 10,
        basis: Basis::Bezier,
        degree: Degree::Cubic,
    });
    device
        .draw_patch(9, true, Some(triangle), None, bindings)
        .unwrap();
    assert_eq!(opcodes(&mut device), [22, 23, 24, 25, 26]);
    assert!(device.draw_patch(9, false, None, None, bindings).is_err());
    device
        .draw_patch(
            9,
            true,
            None,
            Some(Segments::uniform(2.0f32.to_bits(), true).unwrap()),
            DrawBindings::default(),
        )
        .unwrap();
}

#[test]
fn refused_patch_submission_does_not_publish_cache_or_target_content() {
    let mut device = device();
    let bindings = setup(&mut device);
    let batch = device.seal_batch().unwrap().unwrap();
    let version = device
        .resources
        .version(device.color_target.resource)
        .unwrap();
    let error = device
        .draw_patch(1, false, Some(rectangle()), None, bindings)
        .unwrap_err();
    assert_eq!(error, DeviceError::Command(CommandError::Backpressure));
    assert!(device.patches.is_empty());
    assert_eq!(device.patch_sequence, 0);
    assert_eq!(
        device
            .resources
            .version(device.color_target.resource)
            .unwrap(),
        version
    );
    device.acknowledge_batch(batch).unwrap();
    device
        .draw_patch(1, false, Some(rectangle()), None, bindings)
        .unwrap();
    drain(&mut device);
    device.end_scene().unwrap();
    device.reset(description()).unwrap();
    drain(&mut device);
    assert!(device.patches.is_empty());
}

#[test]
fn null_segments_uses_current_render_state_and_domain_reuse_has_no_new_storage() {
    let mut device = device();
    let bindings = setup(&mut device);
    drain(&mut device);
    device
        .draw_patch(
            1,
            false,
            Some(rectangle()),
            Some(Segments::uniform(2.0f32.to_bits(), false).unwrap()),
            bindings,
        )
        .unwrap();
    drain(&mut device);
    let workspace = device.patch_workspace.as_ref().unwrap().vertices().as_ptr();
    device
        .state
        .set_render_state(164, 3.0f32.to_bits())
        .unwrap();
    device
        .draw_patch(1, false, None, None, DrawBindings::default())
        .unwrap();
    assert_eq!(
        device.patch_domain.unwrap().0.segments.words(),
        [3.0f32.to_bits(); 4]
    );
    assert_eq!(opcodes(&mut device), [6, 23, 24, 26]);
    device
        .draw_patch(1, false, None, None, DrawBindings::default())
        .unwrap();
    assert_eq!(opcodes(&mut device), [24]);
    assert_eq!(
        workspace,
        device.patch_workspace.as_ref().unwrap().vertices().as_ptr()
    );
}
