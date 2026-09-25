use super::accelerator_miss_cases;

#[test]
fn imported_accelerator_translation_recognizes_distinct_keys() {
    accelerator_miss_cases::distinct_keys_are_definite_misses();
}

#[test]
fn accelerator_candidates_and_other_message_classes_remain_unsupported() {
    accelerator_miss_cases::candidates_and_unsupported_messages_remain_unhandled();
}

#[test]
fn accelerator_misses_preserve_owned_tables_messages_and_error_cells() {
    accelerator_miss_cases::misses_preserve_owned_data_and_queued_messages();
}

#[test]
fn accelerator_miss_identity_and_memory_failures_are_atomic() {
    accelerator_miss_cases::invalid_targets_and_memory_are_atomic();
}
