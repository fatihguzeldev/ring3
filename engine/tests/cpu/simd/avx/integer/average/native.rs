use super::support::{self, Form, Literals};

const FORMS: &[Form] = &[
    Form {
        map: 1,
        opcode: 0xe0,
        kind: 14,
        lane: 1,
        unary: false,
    },
    Form {
        map: 1,
        opcode: 0xe3,
        kind: 15,
        lane: 2,
        unary: false,
    },
];

fn oracle(form: Form, a: u64, b: u64) -> u64 {
    assert!(matches!(form.kind, 14 | 15));
    // the widened integer sum retains the carry before the upward rounding.
    (a + b).div_ceil(2)
}

fn literals(form: Form) -> Literals {
    let sign = 1_u64 << (form.lane * 8 - 1);
    let mask = sign * 2 - 1;
    (
        [mask, 0, 1, mask, sign, sign - 1, mask - 1, 2],
        [mask, 1, 0, 0, sign - 1, sign, 1, 3],
        [mask, 1, 1, sign, sign, sign, sign, 3],
    )
}

#[test]
fn ymm_encodings_preserve_roles_width_and_ignored_w() {
    support::register_encodings(FORMS, true);
}

#[test]
fn ymm_helpers_match_literals_and_every_source_alias() {
    support::helper_registers(FORMS, true, oracle);
    support::literal_values(FORMS, oracle, literals);
}

#[test]
fn m256_capture_and_late_fault_preserve_complete_state() {
    support::memory_capture_and_faults(FORMS, oracle);
}

#[test]
fn xmm_controls_preserve_legacy_results_and_zero_upper() {
    support::register_encodings(FORMS, false);
    support::helper_registers(FORMS, false, oracle);
}

#[test]
fn reserved_encodings_and_malformed_slots_are_neutral() {
    support::malformed_controls(FORMS);
}
