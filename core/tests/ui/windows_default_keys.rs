use super::default_key_cases;

#[test]
fn ordinary_keys_preserve_state() {
    default_key_cases::ordinary_keys_preserve_state();
}

#[test]
fn special_keys_and_faults_remain_atomic() {
    default_key_cases::special_keys_and_faults_remain_atomic();
}

#[test]
fn default_key_callbacks_resume_whole_or_stepwise() {
    default_key_cases::default_key_callbacks_resume_whole_or_stepwise();
}
