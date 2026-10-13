use super::support::{self, Form, Literals};

const FORMS: &[Form] = &[
    Form {
        map: 2,
        opcode: 0x1c,
        kind: 32,
        lane: 1,
        unary: true,
    },
    Form {
        map: 2,
        opcode: 0x1d,
        kind: 33,
        lane: 2,
        unary: true,
    },
    Form {
        map: 2,
        opcode: 0x1e,
        kind: 34,
        lane: 4,
        unary: true,
    },
];

fn oracle(form: Form, _first: u64, source: u64) -> u64 {
    let modulus = 1_u64 << (form.lane * 8);
    if source < modulus / 2 {
        source
    } else {
        modulus - source
    }
}

fn literals(form: Form) -> Literals {
    let sign = 1_u64 << (form.lane * 8 - 1);
    let mask = sign * 2 - 1;
    // the most-negative signed value has a representable unsigned magnitude.
    (
        [mask; 8],
        [sign, sign + 1, mask, 0, 1, sign - 1, mask - 1, 2],
        [sign, sign - 1, 1, 0, 1, sign - 1, 2, 2],
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
