use super::x87_reverse_divide_cases;

#[test]
fn register_reverse_divide_preserves_sources_and_uses_source_over_top() {
    x87_reverse_divide_cases::register_reverse_divide();
}

#[test]
fn register_reverse_divide_rejects_bad_profiles_atomically() {
    x87_reverse_divide_cases::register_reverse_divide_rejects_atomically();
}
