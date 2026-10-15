use super::*;

#[test]
fn draws_validate_scene_and_user_memory_before_content_versions_change() {
    let mut device = device();
    device.bind_vertex_shader(2).unwrap();
    let target = device.back_buffer(0).unwrap();
    assert_eq!(
        device.draw(
            Primitive::TriangleList,
            1,
            DrawBindings::default(),
            DrawData::UserVertices {
                stride: 12,
                vertices: &[0; 36]
            }
        ),
        Err(DeviceError::InvalidCall)
    );
    device.begin_scene().unwrap();
    assert_eq!(
        device.draw(
            Primitive::TriangleList,
            1,
            DrawBindings::default(),
            DrawData::UserVertices {
                stride: 12,
                vertices: &[0; 35]
            }
        ),
        Err(DeviceError::InvalidCall)
    );
    assert_eq!(device.resources().version(target.resource).unwrap(), 1);
    device
        .draw(
            Primitive::TriangleList,
            1,
            DrawBindings::default(),
            DrawData::UserVertices {
                stride: 12,
                vertices: &[0; 36],
            },
        )
        .unwrap();
    assert_eq!(device.resources().version(target.resource).unwrap(), 2);
    assert_eq!(device.state().snapshot().streams[0].resource, 0);
    device.end_scene().unwrap();
}

#[test]
fn stream_draws_reuse_immutable_pipeline_and_reject_stale_resource_versions() {
    let mut device = device();
    device.bind_vertex_shader(2).unwrap();
    let id = device
        .create_resource(buffer(48, 2, Pool::Managed))
        .unwrap()
        .value;
    drain(&mut device);
    let mut bindings = DrawBindings::default();
    bindings.streams[0] = Some(StreamResource {
        resource: device.resource_version(id).unwrap(),
        stride: 12,
    });
    device.begin_scene().unwrap();
    device
        .draw(
            Primitive::TriangleList,
            1,
            bindings,
            DrawData::Vertices { first_vertex: 0 },
        )
        .unwrap();
    let pipeline = device.emitted_pipeline;
    device
        .draw(
            Primitive::PointList,
            1,
            bindings,
            DrawData::Vertices { first_vertex: 3 },
        )
        .unwrap();
    assert_eq!(device.emitted_pipeline, pipeline);
    bindings.streams[0].as_mut().unwrap().resource.version += 1;
    assert_eq!(
        device.draw(
            Primitive::PointList,
            1,
            bindings,
            DrawData::Vertices { first_vertex: 0 }
        ),
        Err(DeviceError::InvalidCall)
    );
}

#[test]
fn fixed_function_validation_reports_invalid_active_combiner_states() {
    let mut device = device();
    assert_eq!(device.validate_device(), Ok(1));
    device
        .state_mut()
        .unwrap()
        .set_texture_stage_state(0, 1, 999)
        .unwrap();
    assert_eq!(device.validate_device(), Err(DeviceError::InvalidCall));
}

#[test]
fn unchanged_user_draws_keep_pipeline_snapshot_cached() {
    let mut device = device();
    device.bind_vertex_shader(2).unwrap();
    device.begin_scene().unwrap();
    device
        .draw(
            Primitive::PointList,
            1,
            DrawBindings::default(),
            DrawData::UserVertices {
                stride: 12,
                vertices: &[0; 12],
            },
        )
        .unwrap();
    let pipeline = device.emitted_pipeline;
    let revision = device.state().revision();
    device
        .draw(
            Primitive::PointList,
            1,
            DrawBindings::default(),
            DrawData::UserVertices {
                stride: 12,
                vertices: &[0; 12],
            },
        )
        .unwrap();
    assert_eq!(device.emitted_pipeline, pipeline);
    assert_eq!(device.state().revision(), revision);
}

#[test]
fn zero_stride_stream_reuses_one_vertex_without_out_of_bounds_reads() {
    let mut device = device();
    device.bind_vertex_shader(2).unwrap();
    let id = device
        .create_resource(buffer(12, 2, Pool::Managed))
        .unwrap()
        .value;
    drain(&mut device);
    let mut bindings = DrawBindings::default();
    bindings.streams[0] = Some(StreamResource {
        resource: device.resource_version(id).unwrap(),
        stride: 0,
    });
    device.begin_scene().unwrap();
    device
        .draw(
            Primitive::PointList,
            3,
            bindings,
            DrawData::Vertices { first_vertex: 100 },
        )
        .unwrap();
}

#[test]
fn light_state_can_temporarily_exceed_draw_cap_without_rejecting_enable_calls() {
    let mut device = device();
    for index in 0..9 {
        device
            .state_mut()
            .unwrap()
            .enable_light(index, true)
            .unwrap();
    }
    assert_eq!(device.validate_device(), Err(DeviceError::NotAvailable));
    device.state_mut().unwrap().enable_light(0, false).unwrap();
    assert_eq!(device.validate_device(), Ok(1));
}

#[test]
fn accepted_fragmented_draw_commits_once_and_backpressure_preserves_content_version() {
    use super::super::super::commands::{CommandError, Progress};
    let mut limits = DeviceLimits::default();
    limits.commands.batch_bytes = 128;
    let mut device = Device::new(1, description(), limits).unwrap();
    drain(&mut device);
    device.bind_vertex_shader(2).unwrap();
    device.begin_scene().unwrap();
    let target = device.back_buffer(0).unwrap();
    let outcome = device
        .draw(
            Primitive::PointList,
            1,
            DrawBindings::default(),
            DrawData::UserVertices {
                stride: 12,
                vertices: &[0; 12],
            },
        )
        .unwrap();
    assert_eq!(outcome.progress, Progress::Draining);
    assert_eq!(device.resources().version(target.resource), Ok(2));
    assert_eq!(
        device.draw(
            Primitive::PointList,
            1,
            DrawBindings::default(),
            DrawData::UserVertices {
                stride: 12,
                vertices: &[0; 12]
            }
        ),
        Err(DeviceError::Command(CommandError::Backpressure))
    );
    assert_eq!(device.resources().version(target.resource), Ok(2));
    drain(&mut device);
    device
        .draw(
            Primitive::PointList,
            1,
            DrawBindings::default(),
            DrawData::UserVertices {
                stride: 12,
                vertices: &[0; 12],
            },
        )
        .unwrap();
    assert_eq!(device.resources().version(target.resource), Ok(3));
}
