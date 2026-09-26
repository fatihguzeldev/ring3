use super::crt_inline_math_cases;

#[test]
fn imported_inline_math_preserves_x87_and_caller_state() {
    crt_inline_math_cases::finite_values_across_budgets();
}

#[test]
fn invalid_inline_math_domains_are_atomic() {
    crt_inline_math_cases::invalid_domains_are_atomic();
}

#[test]
fn missing_operands_and_unmasked_control_can_retry() {
    crt_inline_math_cases::missing_operand_and_unmasked_control_are_retryable();
}
