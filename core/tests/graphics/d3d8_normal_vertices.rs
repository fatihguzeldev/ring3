use super::normal_vertex_cases;

#[test]
fn vertex_storage_accepts_all_texture_coordinate_counts() {
    normal_vertex_cases::vertex_storage_accepts_texture_coordinate_counts();
}

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

#[test]
fn depth_comparisons_apply_to_indexed_and_up_triangles() {
    normal_vertex_cases::depth_comparison_functions_match_d16_values();
}

#[test]
fn depth_writes_tests_and_clears_are_independent() {
    normal_vertex_cases::depth_writes_are_independent_of_tests_and_clear();
}

#[test]
fn invalid_depth_states_preserve_the_last_policy() {
    normal_vertex_cases::invalid_depth_policy_preserves_the_previous_state();
}
