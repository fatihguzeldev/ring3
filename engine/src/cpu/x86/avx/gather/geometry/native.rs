use super::*;

#[test]
fn all_closed_geometries_keep_distinct_data_index_and_vector_lengths() {
    for (id, data, index, lanes, active, index_register, length, early) in [
        (1, 4, 4, 4, 16, 16, VectorWidth::Xmm, false),
        (2, 4, 4, 8, 32, 32, VectorWidth::Ymm, false),
        (3, 8, 4, 2, 16, 16, VectorWidth::Xmm, true),
        (4, 8, 4, 4, 32, 16, VectorWidth::Ymm, true),
        (5, 4, 8, 2, 8, 16, VectorWidth::Xmm, true),
        (6, 4, 8, 4, 16, 32, VectorWidth::Ymm, true),
        (7, 8, 8, 2, 16, 16, VectorWidth::Xmm, false),
        (8, 8, 8, 4, 32, 32, VectorWidth::Ymm, false),
    ] {
        let geometry = GatherGeometry::from_id(id).unwrap();
        assert_eq!(geometry.id(), id);
        assert_eq!(geometry.data_bytes(), data);
        assert_eq!(geometry.index_bytes(), index);
        assert_eq!(geometry.lanes(), lanes);
        assert_eq!(geometry.active_bytes(), active);
        assert_eq!(geometry.index_register_bytes(), index_register);
        assert_eq!(geometry.vector_length(), length);
        assert_eq!(geometry.early_zero_unused_destination(), early);
    }
    for id in [0, 9, 0x41, 0x81, 255] {
        assert_eq!(GatherGeometry::from_id(id), None);
    }
    for power in 0..=255 {
        let scale = GatherScale::from_power(power);
        if power < 4 {
            let scale = scale.unwrap();
            assert_eq!(scale.power(), power);
            assert_eq!(scale.multiplier(), [1, 2, 4, 8][usize::from(power)]);
        } else {
            assert_eq!(scale, None);
        }
    }
}
