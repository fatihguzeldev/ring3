use super::mutex_wait_cases;

#[test]
fn mutex_release_transfers_recursive_ownership() {
    mutex_wait_cases::mutex_release_transfers_recursive_ownership();
}

#[test]
fn mutex_wait_timeout_does_not_acquire_ownership() {
    mutex_wait_cases::mutex_wait_timeout_does_not_acquire_ownership();
}

#[test]
fn suspended_mutex_waiters_and_return_faults_preserve_ownership() {
    mutex_wait_cases::suspended_mutex_waiters_and_return_faults_preserve_ownership();
}
