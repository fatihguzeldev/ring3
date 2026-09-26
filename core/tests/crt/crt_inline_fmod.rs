use super::crt_inline_fmod_cases;

#[test]
fn inline_fmod_consumes_x87_operands_and_preserves_lower_stack() {
    crt_inline_fmod_cases::finite_results_across_budgets();
}

#[test]
fn invalid_operands_and_unmasked_control_refuse_without_changing_cpu() {
    crt_inline_fmod_cases::invalid_operands_are_atomic_and_retryable();
}

#[test]
fn missing_second_x87_operand_refuses_atomically() {
    crt_inline_fmod_cases::missing_second_operand_is_atomic();
}
