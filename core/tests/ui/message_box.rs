use super::message_box_cases;

#[test]
fn imported_message_box_waits_for_acknowledgement() {
    message_box_cases::imported_message_box_waits_for_acknowledgement();
}

#[test]
fn message_snapshots_preserve_bytes_owner_and_error_state() {
    message_box_cases::message_snapshots_preserve_bytes_owner_and_error_state();
}

#[test]
fn malformed_messages_do_not_publish_or_mutate() {
    message_box_cases::malformed_messages_do_not_publish_or_mutate();
}

#[test]
fn changed_continuations_cannot_consume_acknowledgements() {
    message_box_cases::changed_continuations_cannot_consume_acknowledgements();
}

#[test]
fn pending_message_pauses_ready_threads_and_pins_completion() {
    message_box_cases::pending_message_pauses_ready_threads_and_pins_completion();
}
