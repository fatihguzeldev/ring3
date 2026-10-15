use super::{support as wire, *};

#[test]
fn full_and_sealed_batches_are_atomic_retryable_and_chunk_copy_never_consumes() {
    let mut graphics = wire::context(1, 120, 140);
    let resource = graphics.create_vertex_buffer(&wire::triangle()).unwrap();
    assert_eq!(
        graphics.draw_triangles(resource, 0, 3),
        Err(GraphicsError::BatchCapacity)
    );
    assert_eq!(
        graphics.update_vertex_buffer(resource, 0, &wire::vertex([0.0, 0.0, 0.5, 1.0], 0)),
        Err(GraphicsError::BatchCapacity)
    );
    let batch = graphics.seal_batch().unwrap().unwrap();
    let expected = graphics.peek_batch().unwrap().1.to_vec();
    assert_eq!(expected.len(), 140);
    assert_eq!(graphics.seal_batch(), Ok(Some(batch)));
    let mut short = [0xa5; 139];
    assert_eq!(
        graphics.copy_batch(&mut short),
        Err(GraphicsError::OutputTooSmall)
    );
    assert_eq!(short, [0xa5; 139]);
    let mut copied = [0; 140];
    graphics.copy_batch_range(0, &mut copied[..97]).unwrap();
    graphics.copy_batch_range(97, &mut copied[97..]).unwrap();
    assert_eq!(copied.as_slice(), expected);
    let mut tail = [0xa5; 2];
    assert_eq!(
        graphics.copy_batch_range(139, &mut tail),
        Err(GraphicsError::InvalidRange)
    );
    assert_eq!(tail, [0xa5; 2]);
    for result in [
        graphics.clear([0.0; 4]),
        graphics.present(1),
        graphics.destroy_vertex_buffer(resource),
        graphics.draw_triangles(resource, 0, 3),
    ] {
        assert_eq!(result, Err(GraphicsError::Busy));
    }
    assert_eq!(
        graphics.create_vertex_buffer(&wire::triangle()),
        Err(GraphicsError::Busy)
    );
    assert_eq!(
        graphics.update_vertex_buffer(resource, 0, &wire::triangle()),
        Err(GraphicsError::Busy)
    );
    assert_eq!(
        graphics.acknowledge_batch(BatchId {
            epoch: 7,
            sequence: 2
        }),
        Err(GraphicsError::StaleBatch)
    );
    assert_eq!(graphics.peek_batch().unwrap().1, expected);
    graphics.acknowledge_batch(batch).unwrap();
    graphics.draw_triangles(resource, 0, 3).unwrap();
    let second = graphics.seal_batch().unwrap().unwrap();
    let records = wire::records(graphics.peek_batch().unwrap().1, second);
    assert_eq!(
        wire::u64(records[0].1, 16),
        1,
        "denied updates do not advance content version"
    );
}

#[test]
fn byte_and_slot_limits_and_invalid_vertex_ranges_do_not_publish_commands() {
    let mut graphics = wire::context(1, 120, 1024);
    let resource = graphics.create_vertex_buffer(&wire::triangle()).unwrap();
    assert_eq!(
        graphics.create_vertex_buffer(&wire::vertex([0.0, 0.0, 0.5, 1.0], 0)),
        Err(GraphicsError::ResourceCapacity)
    );
    for (first, count) in [(0, 0), (0, 2), (1, 3), (u32::MAX, 3), (u32::MAX, u32::MAX)] {
        assert_eq!(
            graphics.draw_triangles(resource, first, count),
            Err(GraphicsError::InvalidRange)
        );
    }
    assert_eq!(
        graphics.update_vertex_buffer(resource, u32::MAX, &wire::triangle()),
        Err(GraphicsError::InvalidRange)
    );
    assert_eq!(graphics.present(0), Err(GraphicsError::InvalidSurface));
    assert_eq!(
        graphics.clear([f32::NAN, 0.0, 0.0, 1.0]),
        Err(GraphicsError::InvalidColor)
    );
    for bytes in [
        Vec::new(),
        vec![0; 19],
        wire::vertex([f32::INFINITY, 0.0, 0.0, 1.0], 0),
        wire::vertex([0.0, 0.0, 0.0, -0.0], 0),
    ] {
        assert_eq!(
            graphics.create_vertex_buffer(&bytes),
            Err(GraphicsError::InvalidVertexData)
        );
    }
    assert_eq!(graphics.resource_bytes(), 60);
    let batch = graphics.seal_batch().unwrap().unwrap();
    assert_eq!(
        wire::records(graphics.peek_batch().unwrap().1, batch).len(),
        1
    );
    let mut bounded = wire::context(2, 60, 1024);
    bounded.create_vertex_buffer(&wire::triangle()).unwrap();
    assert_eq!(
        bounded.create_vertex_buffer(&wire::vertex([0.0, 0.0, 0.0, 1.0], 0)),
        Err(GraphicsError::ByteCapacity)
    );
}
