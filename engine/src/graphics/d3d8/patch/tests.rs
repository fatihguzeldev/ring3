use super::*;

#[test]
fn rectangular_extent_preserves_vertex_stride_and_rejects_overflow() {
    let mut info = RectPatchInfo {
        start_width: 1,
        start_height: 2,
        width: 4,
        height: 4,
        stride: 5,
        basis: Basis::Bezier,
        degree: Degree::Cubic,
    };
    assert_eq!(PatchInfo::Rectangle(info).vertex_extent(), Ok((11, 19)));
    info.start_width = 2;
    assert_eq!(PatchInfo::Rectangle(info).vertex_extent(), Err(PatchError));
    info.start_width = 0;
    info.start_height = u32::MAX;
    assert_eq!(PatchInfo::Rectangle(info).vertex_extent(), Err(PatchError));
}

#[test]
fn triangular_basis_degree_and_edge_values_are_typed() {
    for (degree, vertices) in [
        (Degree::Linear, 3),
        (Degree::Cubic, 10),
        (Degree::Quintic, 21),
    ] {
        let mut info = TriPatchInfo {
            start: 7,
            vertices,
            basis: Basis::Bezier,
            degree,
        };
        assert_eq!(PatchInfo::Triangle(info).vertex_extent(), Ok((7, vertices)));
        info.basis = Basis::BSpline;
        assert_eq!(PatchInfo::Triangle(info).vertex_extent(), Err(PatchError));
    }
    for value in [0.0f32, -1.0, f32::NAN, f32::INFINITY, 65.0] {
        assert!(Segments::uniform(value.to_bits(), false).is_err());
    }
    let segments = Segments::new(&[1.0f32.to_bits(), 2.5f32.to_bits(), 64.0f32.to_bits()]).unwrap();
    assert_eq!(segments.words()[3], 0);
    assert!(segments.validate(true).is_ok());
    assert!(segments.validate(false).is_err());
    assert!(Degree::try_from(2).is_err());
}
