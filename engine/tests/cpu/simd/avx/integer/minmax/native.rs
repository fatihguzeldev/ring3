use super::support::{self, Form, Literals};

const FORMS: &[Form] = &[
    Form {
        map: 1,
        opcode: 0xda,
        kind: 10,
        lane: 1,
        unary: false,
    },
    Form {
        map: 1,
        opcode: 0xde,
        kind: 13,
        lane: 1,
        unary: false,
    },
    Form {
        map: 1,
        opcode: 0xea,
        kind: 18,
        lane: 2,
        unary: false,
    },
    Form {
        map: 1,
        opcode: 0xee,
        kind: 21,
        lane: 2,
        unary: false,
    },
    Form {
        map: 2,
        opcode: 0x38,
        kind: 37,
        lane: 1,
        unary: false,
    },
    Form {
        map: 2,
        opcode: 0x39,
        kind: 38,
        lane: 4,
        unary: false,
    },
    Form {
        map: 2,
        opcode: 0x3a,
        kind: 39,
        lane: 2,
        unary: false,
    },
    Form {
        map: 2,
        opcode: 0x3b,
        kind: 40,
        lane: 4,
        unary: false,
    },
    Form {
        map: 2,
        opcode: 0x3c,
        kind: 41,
        lane: 1,
        unary: false,
    },
    Form {
        map: 2,
        opcode: 0x3d,
        kind: 42,
        lane: 4,
        unary: false,
    },
    Form {
        map: 2,
        opcode: 0x3e,
        kind: 43,
        lane: 2,
        unary: false,
    },
    Form {
        map: 2,
        opcode: 0x3f,
        kind: 44,
        lane: 4,
        unary: false,
    },
];

fn oracle(form: Form, a: u64, b: u64) -> u64 {
    let modulus = 1_i128 << (form.lane * 8);
    let half = modulus / 2;
    let signed = |value: u64| {
        if i128::from(value) >= half {
            i128::from(value) - modulus
        } else {
            i128::from(value)
        }
    };
    let choose_first = match form.kind {
        10 | 39 | 40 => a < b,
        13 | 43 | 44 => a > b,
        18 | 37 | 38 => signed(a) < signed(b),
        21 | 41 | 42 => signed(a) > signed(b),
        _ => unreachable!(),
    };
    if choose_first { a } else { b }
}

fn literals(form: Form) -> Literals {
    let sign = 1_u64 << (form.lane * 8 - 1);
    let mask = sign * 2 - 1;
    let first = [mask, sign, sign - 1, 0, 0, sign, mask, 1];
    let second = [0, sign - 1, sign, mask, 0, sign, mask, mask];
    let result = match form.kind {
        18 | 37 | 38 | 13 | 43 | 44 => [mask, sign, sign, mask, 0, sign, mask, mask],
        10 | 39 | 40 | 21 | 41 | 42 => [0, sign - 1, sign - 1, 0, 0, sign, mask, 1],
        _ => unreachable!(),
    };
    (first, second, result)
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
