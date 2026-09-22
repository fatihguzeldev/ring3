#[path = "support/imported_executable.rs"]
mod imported_executable;
#[path = "support/window_creation_executable.rs"]
mod window_creation_executable;
#[path = "support/hook_chain_cases.rs"]
mod hook_chain_cases;

#[test]
fn real_forwarding_preserves_arguments_results_and_creation_effects_across_budgets() {
    hook_chain_cases::verify();
}
