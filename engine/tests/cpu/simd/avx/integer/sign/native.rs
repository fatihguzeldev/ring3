use super::support::{self, Form, Literals};

const FORMS: &[Form] = &[
    Form {
        map: 2,
        opcode: 0x08,
        kind: 29,
        lane: 1,
        unary: false,
    },
    Form {
        map: 2,
        opcode: 0x09,
        kind: 30,
        lane: 2,
        unary: false,
    },
    Form {
        map: 2,
        opcode: 0x0a,
        kind: 31,
        lane: 4,
        unary: false,
    },
];

fn oracle(form: Form, data: u64, control: u64) -> u64 {
    let modulus = 1_u64 << (form.lane * 8);
    if control == 0 {
        0
    } else if control < modulus / 2 {
        data
    } else {
        (modulus - data) & (modulus - 1)
    }
}

fn literals(form: Form) -> Literals {
    let sign = 1_u64 << (form.lane * 8 - 1);
    let mask = sign * 2 - 1;
    // negation wraps; a negative sign control never requests saturation.
    (
        [sign, sign, mask, mask, 1, sign - 1, sign + 1, 2],
        [mask, 0, 1, mask, sign, 1, mask, 0],
        [sign, 0, mask, 1, mask, sign - 1, sign - 1, 0],
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
