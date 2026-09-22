#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/window_creation_executable.rs"]
mod window_creation_executable;
#[path = "support/window_message_cases.rs"]
mod window_message_cases;

#[test]
fn sends_execute_the_current_procedure_with_real_arguments_and_results() {
    window_message_cases::verify();
}
