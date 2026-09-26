use super::mmx_packed_cases;

#[test]
fn independent_integer_vectors() {
    mmx_packed_cases::packed_register_and_memory_results_match_independent_oracles();
}

#[test]
fn immediate_counts() {
    mmx_packed_cases::immediate_shifts_match_verified_full_width_count_forms();
}

#[test]
fn operand_boundaries() {
    mmx_packed_cases::narrow_unpacks_and_faults_respect_the_complete_operand_width();
}
