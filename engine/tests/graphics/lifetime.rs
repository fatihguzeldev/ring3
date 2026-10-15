use super::{support as wire, *};

#[test]
fn reset_preflight_is_neutral_and_new_epoch_rejects_old_resources_and_acknowledgements() {
    let mut graphics = wire::context(1, 60, 512);
    let resource = graphics.create_vertex_buffer(&wire::triangle()).unwrap();
    let batch = graphics.seal_batch().unwrap().unwrap();
    let expected = graphics.peek_batch().unwrap().1.to_vec();
    for epoch in [0, 6, 7] {
        assert_eq!(graphics.can_reset(epoch), Err(GraphicsError::InvalidEpoch));
        assert_eq!(graphics.reset(epoch), Err(GraphicsError::InvalidEpoch));
        assert_eq!(graphics.peek_batch().unwrap().1, expected);
        assert_eq!(graphics.resource_bytes(), 60);
    }
    graphics.can_reset(8).unwrap();
    assert_eq!(graphics.peek_batch().unwrap().1, expected);
    graphics.reset(8).unwrap();
    assert_eq!(graphics.resource_bytes(), 0);
    assert!(graphics.peek_batch().is_none());
    assert_eq!(
        graphics.acknowledge_batch(batch),
        Err(GraphicsError::StaleBatch)
    );
    assert_eq!(
        graphics.draw_triangles(resource, 0, 3),
        Err(GraphicsError::StaleResource)
    );
    let replacement = graphics.create_vertex_buffer(&wire::triangle()).unwrap();
    assert_eq!(
        replacement,
        ResourceId {
            epoch: 8,
            slot: 0,
            generation: 1
        }
    );
    assert_ne!(replacement, resource);
    let next = graphics.seal_batch().unwrap().unwrap();
    assert_eq!(
        next,
        BatchId {
            epoch: 8,
            sequence: 1
        }
    );
    wire::records(graphics.peek_batch().unwrap().1, next);
}

#[test]
fn empty_queue_and_configured_byte_bounds_are_explicit() {
    let mut graphics = wire::context(1, 60, 512);
    assert_eq!(graphics.seal_batch(), Ok(None));
    let mut output = [0xa5; 128];
    assert_eq!(
        graphics.copy_batch(&mut output),
        Err(GraphicsError::EmptyBatch)
    );
    assert_eq!(output, [0xa5; 128]);
    assert!(GraphicsContext::new(0, GraphicsLimits::default()).is_err());
    for limits in [
        GraphicsLimits {
            resources: 0,
            ..GraphicsLimits::default()
        },
        GraphicsLimits {
            resource_bytes: 19,
            ..GraphicsLimits::default()
        },
        GraphicsLimits {
            batch_bytes: 99,
            ..GraphicsLimits::default()
        },
    ] {
        assert!(matches!(
            GraphicsContext::new(1, limits),
            Err(GraphicsError::InvalidLimits)
        ));
    }
    assert_eq!(graphics.limits().batch_bytes, 512);
}

#[test]
fn provisioning_ceilings_reject_oversized_requests_before_eager_allocation() {
    assert_eq!(GraphicsLimits::MAX_RESOURCES, 4096);
    assert_eq!(GraphicsLimits::MAX_RESOURCE_BYTES, 268_435_456);
    assert_eq!(GraphicsLimits::MAX_BATCH_BYTES, 16_777_216);
    for limits in [
        GraphicsLimits {
            resources: 4097,
            ..GraphicsLimits::default()
        },
        GraphicsLimits {
            resource_bytes: 268_435_457,
            ..GraphicsLimits::default()
        },
        GraphicsLimits {
            batch_bytes: 16_777_217,
            ..GraphicsLimits::default()
        },
        GraphicsLimits {
            resources: u32::MAX,
            ..GraphicsLimits::default()
        },
        GraphicsLimits {
            resource_bytes: u32::MAX,
            ..GraphicsLimits::default()
        },
        GraphicsLimits {
            batch_bytes: u32::MAX,
            ..GraphicsLimits::default()
        },
    ] {
        assert!(matches!(
            GraphicsContext::new(1, limits),
            Err(GraphicsError::InvalidLimits)
        ));
    }
}
