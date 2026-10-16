use super::*;

fn key(triangular: bool, values: &[f32], style: EdgeStyle) -> DomainKey {
    DomainKey {
        triangular,
        segments: Segments::new(&values.iter().map(|v| v.to_bits()).collect::<Vec<_>>()).unwrap(),
        style,
    }
}
fn area(mesh: &DomainMesh) -> f64 {
    let mut sum = 0.0;
    for triangle in mesh.indices().chunks_exact(3) {
        let a = mesh.vertices()[triangle[0] as usize].map(f64::from);
        let b = mesh.vertices()[triangle[1] as usize].map(f64::from);
        let c = mesh.vertices()[triangle[2] as usize].map(f64::from);
        let signed = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        assert!(signed >= -0.000001);
        sum += signed * 0.5;
    }
    sum
}
#[test]
fn asymmetric_domains_cover_exact_area_and_literal_direct3d_edges() {
    let mut mesh = DomainMesh::new().unwrap();
    mesh.rebuild(key(true, &[2.0, 3.0, 4.0], EdgeStyle::Discrete))
        .unwrap();
    assert_eq!(mesh.boundary_count(), 9);
    assert_eq!(mesh.rings(), 2);
    assert_eq!(mesh.vertices().len(), 19);
    assert_eq!(mesh.indices().len(), 81);
    assert_eq!(
        &mesh.vertices()[10..15],
        &[[0.0, 0.0], [0.25, 0.0], [0.5, 0.0], [0.75, 0.0], [1.0, 0.0]]
    );
    assert!((area(&mesh) - 0.5).abs() < 0.000001);
    mesh.rebuild(key(false, &[1.0, 2.0, 3.0, 4.0], EdgeStyle::Discrete))
        .unwrap();
    assert_eq!(mesh.boundary_count(), 10);
    assert!((area(&mesh) - 1.0).abs() < 0.000001);
}
#[test]
fn maximum_domains_respect_bounded_workspace_and_minimum_density() {
    let mut mesh = DomainMesh::new().unwrap();
    let vertex_allocation = mesh.vertices().as_ptr();
    let index_allocation = mesh.indices().as_ptr();
    for triangular in [false, true] {
        for style in [EdgeStyle::Discrete, EdgeStyle::Continuous] {
            let values = if triangular {
                &[64.0; 3][..]
            } else {
                &[64.0; 4][..]
            };
            mesh.rebuild(key(triangular, values, style)).unwrap();
            assert!(mesh.vertices().len() <= MAX_VERTICES);
            assert!(mesh.indices().len() <= MAX_INDICES);
            assert!(mesh.indices().len() / 3 >= if triangular { 4096 } else { 8192 });
            assert!((area(&mesh) - if triangular { 0.5 } else { 1.0 }).abs() < 0.00001);
        }
    }
    assert_eq!(vertex_allocation, mesh.vertices().as_ptr());
    assert_eq!(index_allocation, mesh.indices().as_ptr());
}
#[test]
fn fractional_partition_new_vertices_and_rings_are_continuous_at_integer_crossings() {
    for threshold in [2.0, 4.0, 16.0, 62.0] {
        let mut old = DomainMesh::new().unwrap();
        let mut new = DomainMesh::new().unwrap();
        old.rebuild(key(false, &[threshold; 4], EdgeStyle::Continuous))
            .unwrap();
        new.rebuild(key(false, &[threshold + 0.00001; 4], EdgeStyle::Continuous))
            .unwrap();
        for vertex in new.vertices() {
            let distance = old
                .vertices()
                .iter()
                .map(|v| (v[0] - vertex[0]).abs() + (v[1] - vertex[1]).abs())
                .fold(f32::INFINITY, f32::min);
            assert!(distance < 0.00002, "{threshold}: {vertex:?} {distance}");
        }
        assert!((area(&new) - 1.0).abs() < 0.00001);
    }
}
#[test]
fn opposing_edges_share_bit_exact_symmetric_knots_and_invalid_rebuild_is_neutral() {
    let mut mesh = DomainMesh::new().unwrap();
    mesh.rebuild(key(false, &[3.75; 4], EdgeStyle::Continuous))
        .unwrap();
    let outer = &mesh.vertices()[1 + (mesh.rings() - 1) * mesh.boundary_count()..];
    for i in 1..4 {
        assert_eq!(outer[i][0].to_bits(), outer[12 - i][0].to_bits());
    }
    let vertices = mesh.vertices().to_vec();
    let indices = mesh.indices().to_vec();
    let bad = DomainKey {
        triangular: true,
        ..key(false, &[3.75; 4], EdgeStyle::Continuous)
    };
    assert!(mesh.rebuild(bad).is_err());
    assert_eq!(mesh.vertices(), vertices);
    assert_eq!(mesh.indices(), indices);
}
