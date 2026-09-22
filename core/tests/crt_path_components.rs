#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/path_component_cases.rs"]
mod path_component_cases;

#[test]
fn imported_path_splitting_preserves_bytes_and_cdecl_void_state() {
    path_component_cases::verify();
}
