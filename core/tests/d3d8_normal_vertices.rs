#[path = "support/d3d8_executable.rs"]
mod d3d8_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/normal_vertex_cases.rs"]
mod normal_vertex_cases;

#[test]
fn normal_layout_matches_diffuse_and_textured_pixels() {
    normal_vertex_cases::equivalent_diffuse_and_textured_pixels();
}

#[test]
fn invalid_normal_records_preserve_color_and_depth() {
    normal_vertex_cases::rejects_short_records_and_nonfinite_inputs_atomically();
}

#[test]
fn unsupported_format_selection_preserves_normal_layout() {
    normal_vertex_cases::rejected_selection_preserves_the_normal_layout();
}

#[test]
fn world_transform_moves_both_indexed_layouts() {
    normal_vertex_cases::world_transform_controls_positions();
}

#[test]
fn texture_transform_is_not_a_world_transform() {
    normal_vertex_cases::texture_matrix_does_not_substitute_for_world();
}
