use super::{support as wire, *};

#[test]
fn versioned_draw_update_draw_and_destroy_reuse_keep_literal_payload_order() {
    let mut graphics = wire::context(1, 120, 1024);
    let mut source = wire::triangle();
    let resource = graphics.create_vertex_buffer(&source).unwrap();
    source[16..20].copy_from_slice(&0xffffffffu32.to_le_bytes());
    graphics.clear([0.25, 0.5, 0.75, 1.0]).unwrap();
    graphics.draw_triangles(resource, 0, 3).unwrap();
    let changed = wire::vertex([0.25, -0.25, 0.75, 1.0], 0x44332211);
    graphics
        .update_vertex_buffer(resource, 1, &changed)
        .unwrap();
    graphics.draw_triangles(resource, 0, 3).unwrap();
    graphics.destroy_vertex_buffer(resource).unwrap();
    assert_eq!(graphics.resource_bytes(), 0);
    assert_eq!(
        graphics.draw_triangles(resource, 0, 3),
        Err(GraphicsError::StaleResource)
    );
    let replacement = graphics.create_vertex_buffer(&source).unwrap();
    assert_eq!(
        (replacement.slot, replacement.generation),
        (resource.slot, resource.generation + 1)
    );
    graphics.present(91).unwrap();
    let batch = graphics.seal_batch().unwrap().unwrap();
    assert_eq!(
        batch,
        BatchId {
            epoch: 7,
            sequence: 1
        }
    );
    let (_, bytes) = graphics.peek_batch().unwrap();
    let records = wire::records(bytes, batch);
    assert_eq!(
        records.iter().map(|record| record.0).collect::<Vec<_>>(),
        [1, 5, 3, 2, 3, 4, 1, 6]
    );
    for index in [0, 2, 3, 4, 5] {
        assert_eq!(wire::id(records[index].1), resource);
    }
    assert_eq!(wire::u64(records[0].1, 16), 1);
    assert_eq!(
        (wire::u32(records[0].1, 24), wire::u32(records[0].1, 28)),
        (3, 1)
    );
    assert_eq!(&records[0].1[32..], wire::triangle());
    assert_eq!(
        records[1].1,
        [0x3e800000u32, 0x3f000000, 0x3f400000, 0x3f800000]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        (
            wire::u64(records[2].1, 16),
            wire::u32(records[2].1, 24),
            wire::u32(records[2].1, 28)
        ),
        (1, 0, 3)
    );
    assert_eq!(
        (
            wire::u64(records[3].1, 16),
            wire::u32(records[3].1, 24),
            wire::u32(records[3].1, 28)
        ),
        (2, 1, 1)
    );
    assert_eq!(&records[3].1[32..], changed);
    assert_eq!(wire::u64(records[4].1, 16), 2);
    assert_eq!(records[5].1.len(), 16);
    assert_eq!(wire::id(records[6].1), replacement);
    assert_eq!(&records[6].1[32..], source);
    assert_eq!(
        records[7].1,
        [91u32, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>()
    );
    graphics.acknowledge_batch(batch).unwrap();
    assert!(graphics.peek_batch().is_none());
    assert_eq!(graphics.resource_bytes(), 60);
    graphics.draw_triangles(replacement, 0, 3).unwrap();
    let second = graphics.seal_batch().unwrap().unwrap();
    assert_eq!(second.sequence, 2);
    let records = wire::records(graphics.peek_batch().unwrap().1, second);
    assert_eq!(wire::id(records[0].1), replacement);
    assert_eq!(wire::u64(records[0].1, 16), 1);
}
