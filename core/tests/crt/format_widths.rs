use super::format_width_cases;

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

#[test]
fn zero_padding_preserves_signs_and_wrapper_rules() {
    format_width_cases::zero_padding_preserves_signs_and_wrapper_rules();
}
