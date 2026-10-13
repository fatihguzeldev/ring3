use super::support::{self, Form, Literals};

const FORMS: &[Form] = &[
    Form {
        map: 1,
        opcode: 0xd8,
        kind: 8,
        lane: 1,
        unary: false,
    },
    Form {
        map: 1,
        opcode: 0xd9,
        kind: 9,
        lane: 2,
        unary: false,
    },
    Form {
        map: 1,
        opcode: 0xdc,
        kind: 11,
        lane: 1,
        unary: false,
    },
    Form {
        map: 1,
        opcode: 0xdd,
        kind: 12,
        lane: 2,
        unary: false,
    },
    Form {
        map: 1,
        opcode: 0xe8,
        kind: 16,
        lane: 1,
        unary: false,
    },
    Form {
        map: 1,
        opcode: 0xe9,
        kind: 17,
        lane: 2,
        unary: false,
    },
    Form {
        map: 1,
        opcode: 0xec,
        kind: 19,
        lane: 1,
        unary: false,
    },
    Form {
        map: 1,
        opcode: 0xed,
        kind: 20,
        lane: 2,
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
    match form.kind {
        8 | 9 => a.saturating_sub(b),
        11 | 12 => (a + b).min((modulus - 1) as u64),
        16 | 17 => ((signed(a) - signed(b)).clamp(-half, half - 1) & (modulus - 1)) as u64,
        19 | 20 => ((signed(a) + signed(b)).clamp(-half, half - 1) & (modulus - 1)) as u64,
        _ => unreachable!(),
    }
}

fn literals(form: Form) -> Literals {
    let sign = 1_u64 << (form.lane * 8 - 1);
    let mask = sign * 2 - 1;
    match form.kind {
        8 | 9 => (
            [0, 1, mask, mask, sign, sign - 1, 2, mask],
            [1, mask, 0, mask, sign - 1, sign, 1, mask - 1],
            [0, 0, mask, 0, 1, 0, 1, 1],
        ),
        11 | 12 => (
            [mask, mask, 0, 1, mask - 1, sign, sign - 1, 2],
            [1, mask, 0, 1, 1, sign, sign, mask - 1],
            [mask, mask, 0, 2, mask, mask, mask, mask],
        ),
        16 | 17 => (
            [sign - 1, sign, sign - 1, sign, 0, mask, 1, mask],
            [mask, 1, 1, mask, 0, 1, mask, mask],
            [sign - 1, sign, sign - 2, sign + 1, 0, mask - 1, 2, 0],
        ),
        19 | 20 => (
            [sign - 1, sign, sign - 1, sign, 0, mask, 1, mask],
            [1, mask, mask, 1, 0, 1, 1, mask],
            [sign - 1, sign, sign - 2, sign + 1, 0, 0, 2, mask - 1],
        ),
        _ => unreachable!(),
    }
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
