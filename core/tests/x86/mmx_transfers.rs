use super::mmx_transfer_cases;

#[test]
fn bit_preserving_transfers() {
    mmx_transfer_cases::transfers_preserve_bits_in_every_register_and_memory();
    mmx_transfer_cases::dword_moves_zero_extend_and_truncate();
}

#[test]
fn shared_x87_state() {
    mmx_transfer_cases::emms_preserves_physical_bits_status_and_top();
    mmx_transfer_cases::physical_significands_survive_pops_and_pending_faults_stop_mmx();
}

#[test]
fn atomic_failures() {
    mmx_transfer_cases::faults_and_unsupported_numeric_transitions_are_atomic();
}
