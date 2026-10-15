use super::*;

#[test]
fn recorded_state_is_isolated_and_custom_apply_is_selective() {
    let mut device = DeviceState::new(640, 480, true).unwrap();
    let revision = device.revision();
    device.begin_state_block().unwrap();
    device.set_render_state(7, 0).unwrap();
    device
        .set_vertex_constants(95, &[[0x8000_0000, 0x7fc0_1234, 2, 3]])
        .unwrap();
    device.set_texture(2, 23).unwrap();
    assert_eq!(device.render_state(7), Ok(1));
    assert_eq!(device.snapshot().textures[2], 0);
    assert_eq!(device.revision(), revision);
    let mut block = device.end_state_block().unwrap();
    assert_eq!(block.resource_bindings().collect::<Vec<_>>(), [23]);
    device.set_render_state(22, 1).unwrap();
    device.apply_state_block(&block).unwrap();
    assert_eq!(device.render_state(7), Ok(0));
    assert_eq!(device.render_state(22), Ok(1));
    assert_eq!(
        device.snapshot().vertex_constants[95],
        [0x8000_0000, 0x7fc0_1234, 2, 3]
    );
    device.set_render_state(7, 2).unwrap();
    device.capture_state_block(&mut block).unwrap();
    device.set_render_state(7, 1).unwrap();
    device.set_render_state(22, 2).unwrap();
    device.apply_state_block(&block).unwrap();
    assert_eq!(device.render_state(7), Ok(2));
    assert_eq!(device.render_state(22), Ok(2));
}

#[test]
fn predefined_blocks_preserve_other_pipeline_groups() {
    let mut device = DeviceState::new(640, 480, false).unwrap();
    device.set_render_state(27, 1).unwrap();
    device.set_render_state(137, 0).unwrap();
    device.set_texture(0, 13).unwrap();
    let pixel = device.create_state_block(StateBlockKind::Pixel).unwrap();
    device.set_render_state(27, 0).unwrap();
    device.set_render_state(137, 1).unwrap();
    device.set_texture(0, 17).unwrap();
    device.apply_state_block(&pixel).unwrap();
    assert_eq!(device.render_state(27), Ok(1));
    assert_eq!(device.render_state(137), Ok(1));
    assert_eq!(device.snapshot().textures[0], 17);
    assert_eq!(device.snapshot().viewport.max_z, 0x3f80_0000);
    device.set_render_state(128, 3).unwrap();
    device.set_render_state(161, 0).unwrap();
    device.set_texture_stage_state(0, 11, 7).unwrap();
    let vertex = device.create_state_block(StateBlockKind::Vertex).unwrap();
    device.set_render_state(128, 0).unwrap();
    device.set_render_state(161, 1).unwrap();
    device.set_texture_stage_state(0, 11, 2).unwrap();
    device.apply_state_block(&vertex).unwrap();
    assert_eq!(device.render_state(128), Ok(0));
    assert_eq!(device.render_state(161), Ok(0));
    assert_eq!(device.texture_stage_state(0, 11), Ok(7));
}

#[test]
fn state_ranges_fail_without_mutation_and_enabled_lights_can_exceed_draw_caps() {
    let mut device = DeviceState::new(320, 200, true).unwrap();
    let revision = device.revision();
    assert_eq!(
        device.set_vertex_constants(96, &[[0; 4]]),
        Err(StateError::InvalidRange)
    );
    assert_eq!(
        device.set_render_state(138, 1),
        Err(StateError::InvalidState)
    );
    assert_eq!(
        device.set_texture_stage_state(8, 1, 1),
        Err(StateError::InvalidRange)
    );
    assert_eq!(
        device.set_transform(512, Matrix::IDENTITY),
        Err(StateError::InvalidState)
    );
    assert_eq!(device.revision(), revision);
    for index in 0..8 {
        device.enable_light(u32::MAX - index, true).unwrap();
    }
    assert_eq!(device.light(u32::MAX), Ok(Light::default()));
    let revision = device.revision();
    device.enable_light(99, true).unwrap();
    assert_eq!(device.light(99), Ok(Light::default()));
    assert!(device.revision() > revision);
}

#[test]
fn matrix_multiplication_uses_d3d_pre_multiply_order() {
    let mut device = DeviceState::new(320, 200, false).unwrap();
    let mut translate = Matrix::IDENTITY;
    translate.0[12] = 3.0f32.to_bits();
    let mut scale = Matrix::IDENTITY;
    scale.0[0] = 2.0f32.to_bits();
    device.set_transform(256, translate).unwrap();
    device.multiply_transform(256, scale).unwrap();
    let result = device.transform(256).unwrap();
    assert_eq!(result.0[0], 2.0f32.to_bits());
    assert_eq!(result.0[12], 3.0f32.to_bits());
    assert_eq!(device.transform(511), Ok(Matrix::IDENTITY));
}
