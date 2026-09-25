#[path = "support/format_width_cases.rs"]
mod format_width_cases;
#[path = "support/formatting_executable.rs"]
mod formatting_executable;
#[path = "support/imported_executable.rs"]
mod imported_executable;

#[test]
fn imported_width_runs_whole_or_stepwise() {
    format_width_cases::imported_width_runs_whole_or_stepwise();
}

#[test]
fn minimum_widths_preserve_values_and_wrapper_rules() {
    format_width_cases::minimum_widths_preserve_values_and_wrapper_rules();
}

#[test]
fn widths_obey_total_output_bounds() {
    format_width_cases::widths_obey_total_output_bounds();
}

#[test]
fn unsupported_widths_and_late_faults_are_atomic() {
    format_width_cases::unsupported_widths_and_late_faults_are_atomic();
}
