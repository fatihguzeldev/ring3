use super::x87_roundup_cases;

#[test]
fn inexact_sum_round_up_status_tracks_magnitude_for_both_signs() {
    x87_roundup_cases::sums();
}

#[test]
fn products_and_quotients_report_the_same_round_up_bit_for_both_signs() {
    x87_roundup_cases::products_and_quotients();
}
