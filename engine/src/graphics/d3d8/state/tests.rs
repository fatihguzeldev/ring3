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

#[test]
fn binding_revisions_preserve_wire_state_and_transaction_exhaustion() {
    let mut state = DeviceState::new(16, 16, false).unwrap();
    let wire = state.pipeline_revision();
    state.set_texture(7, 12).unwrap();
    state
        .set_stream(
            0,
            StreamBinding {
                resource: 13,
                stride: 16,
            },
        )
        .unwrap();
    state.set_indices(14, 3).unwrap();
    let revision = state.revision();
    assert_eq!(state.pipeline_revision(), wire);
    state.finish_user_draw(true).unwrap();
    assert!(state.revision() > revision);
    assert_eq!(state.pipeline_revision(), wire);
    assert_eq!(state.snapshot().streams[0], StreamBinding::default());
    assert_eq!(state.snapshot().indices, 0);
    assert_eq!(state.snapshot().base_vertex, 0);
    assert_eq!(state.try_clone().unwrap().pipeline_revision(), wire);
    state.revision = u64::MAX;
    assert_eq!(state.set_texture(7, 99), Err(StateError::IdentityExhausted));
    assert_eq!(state.snapshot().textures[7], 12);
    assert_eq!(state.pipeline_revision(), wire);
    assert_eq!(
        state.set_render_state(22, 1),
        Err(StateError::IdentityExhausted)
    );
    assert_eq!(state.pipeline_revision(), wire);
}

#[test]
fn failed_state_block_application_keeps_both_revisions_and_live_state() {
    let large = DeviceState::new(64, 64, false).unwrap();
    let block = large.create_state_block(StateBlockKind::All).unwrap();
    let mut small = DeviceState::new(16, 16, false).unwrap();
    let revision = small.revision();
    let wire = small.pipeline_revision();
    let before = small.snapshot().try_clone().unwrap();
    assert_eq!(
        small.apply_state_block(&block),
        Err(StateError::InvalidRange)
    );
    assert_eq!(small.snapshot(), &before);
    assert_eq!(small.revision(), revision);
    assert_eq!(small.pipeline_revision(), wire);
}
