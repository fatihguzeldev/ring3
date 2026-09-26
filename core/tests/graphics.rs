#[path = "support/d3d8_executable.rs"]
mod d3d8_executable;
#[path = "support/discard_resource_cases.rs"]
mod discard_resource_cases;
#[path = "support/executable.rs"]
mod executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/light_cases.rs"]
mod light_cases;
#[path = "support/material_cases.rs"]
mod material_cases;
#[path = "support/normal_vertex_cases.rs"]
mod normal_vertex_cases;
#[path = "support/readonly_texture_cases.rs"]
mod readonly_texture_cases;
#[path = "support/window_creation_executable.rs"]
mod window_creation_executable;

#[path = "graphics/d3d8_frame.rs"]
mod d3d8_frame;
#[path = "graphics/d3d8_normal_vertices.rs"]
mod d3d8_normal_vertices;

#[test]
fn material_state_accepts_a_full_readable_structure() {
    material_cases::material_state_accepts_a_full_readable_structure();
}

#[test]
fn material_read_fault_does_not_complete_the_call() {
    material_cases::material_read_fault_does_not_complete_the_call();
}

#[test]
fn light_state_accepts_a_full_readable_structure() {
    light_cases::light_state_accepts_a_full_readable_structure();
}

#[test]
fn light_read_fault_does_not_complete_the_call() {
    light_cases::light_read_fault_does_not_complete_the_call();
}
