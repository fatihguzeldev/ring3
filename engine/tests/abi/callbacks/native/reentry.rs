use ring3_engine::abi::AbiError;
use ring3_engine::abi::callback::{
    CALLBACK_RECORD_SIZE, CallbackRecord32, decode_callback, encode_callback,
};

const GOLDEN: [u8; 64] = [
    0x52, 0x33, 0x43, 0x42, 0x01, 0x00, 0x01, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0xcc, 0xbb, 0xaa, 0x99, 0x01, 0xff, 0xee, 0xdd, 0x78, 0x56, 0x34, 0x12, 0x21, 0x43, 0x65, 0x87,
    0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xef, 0xbe, 0xad, 0xde, 0x00, 0x00, 0x00, 0x00,
];

fn golden_record() -> CallbackRecord32 {
    CallbackRecord32 {
        token: 0x5566_7788,
        outer_token: 0x1122_3344,
        phase: 1,
        outcome: 1,
        entry_pc: 0x99aa_bbcc,
        entry_esp: 0xddee_ff01,
        return_pc: 0x1234_5678,
        return_id: 0x8765_4321,
        stack_words: 16,
        result: 0,
        generation: 0xdead_beef,
    }
}

fn set_word(bytes: &mut [u8; 64], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn literal(record: &CallbackRecord32) -> [u8; 64] {
    let mut bytes = [0; 64];
    bytes[..16].copy_from_slice(&[0x52, 0x33, 0x43, 0x42, 1, 0, 1, 0, 64, 0, 0, 0, 0, 0, 0, 0]);
    for (index, value) in [
        record.token,
        record.outer_token,
        record.phase,
        record.outcome,
        record.entry_pc,
        record.entry_esp,
        record.return_pc,
        record.return_id,
        record.stack_words,
        record.result,
        record.generation,
    ]
    .into_iter()
    .enumerate()
    {
        set_word(&mut bytes, 16 + index * 4, value);
    }
    bytes
}

fn assert_atomic_rejection(record: &CallbackRecord32) {
    let mut output = [0xa5; 64];
    assert_eq!(
        encode_callback(record, &mut output),
        Err(AbiError::CallbackFrame)
    );
    assert_eq!(output, [0xa5; 64]);
    assert_eq!(
        decode_callback(&literal(record)),
        Err(AbiError::CallbackFrame)
    );
}

#[test]
fn callback_record_matches_an_independent_full_sixty_four_byte_literal() {
    assert_eq!(CALLBACK_RECORD_SIZE, 64);
    let expected = golden_record();
    let mut output = [0xa5; 64];
    encode_callback(&expected, &mut output).unwrap();
    assert_eq!(output, GOLDEN);
    assert_eq!(decode_callback(&GOLDEN), Ok(expected));
}

#[test]
fn started_and_returned_records_have_closed_phase_outcome_result_policies() {
    for (phase, outcome, result) in [(1, 0, 0), (1, 1, 0), (2, 0, 0), (2, 0, u32::MAX)] {
        for count in [0, 1, 16] {
            let mut record = golden_record();
            record.token = u32::MAX;
            record.outer_token = u32::MAX - 1;
            record.phase = phase;
            record.outcome = outcome;
            record.stack_words = count;
            record.result = result;
            record.generation = 1;
            record.entry_pc = 0;
            record.entry_esp = u32::MAX;
            record.return_pc = u32::MAX;
            let mut output = [0xa5; 64];
            encode_callback(&record, &mut output).unwrap();
            assert_eq!(output, literal(&record));
            assert_eq!(decode_callback(&literal(&record)), Ok(record));
        }
    }
}

#[test]
fn tokens_must_be_nonzero_and_callback_token_must_follow_the_outer_token() {
    for (token, outer_token) in [(0, 1), (1, 0), (0, 0), (1, 1), (1, 2), (u32::MAX, u32::MAX)] {
        let mut record = golden_record();
        record.token = token;
        record.outer_token = outer_token;
        assert_atomic_rejection(&record);
    }
}

#[test]
fn generation_return_id_count_and_distinct_program_counters_are_validated_atomically() {
    for field in [44, 56] {
        let mut record = golden_record();
        if field == 44 {
            record.return_id = 0;
        } else {
            record.generation = 0;
        }
        assert_atomic_rejection(&record);
    }
    for count in [17, u32::MAX] {
        let mut record = golden_record();
        record.stack_words = count;
        assert_atomic_rejection(&record);
    }
    let mut record = golden_record();
    record.entry_pc = record.return_pc;
    assert_atomic_rejection(&record);
}

#[test]
fn invalid_phase_outcome_or_result_combination_is_rejected_without_output_changes() {
    for (phase, outcome, result) in [
        (0, 0, 0),
        (3, 0, 0),
        (u32::MAX, 0, 0),
        (1, 2, 0),
        (1, u32::MAX, 0),
        (1, 0, 1),
        (1, 1, u32::MAX),
        (2, 1, 0),
        (2, 1, u32::MAX),
        (2, 2, 0),
    ] {
        let mut record = golden_record();
        record.phase = phase;
        record.outcome = outcome;
        record.result = result;
        assert_atomic_rejection(&record);
    }
}

#[test]
fn every_malformed_header_byte_keeps_its_field_specific_error() {
    for offset in 0..16 {
        let mut malformed = GOLDEN;
        malformed[offset] ^= 0x80;
        let expected = match offset {
            0..=3 => AbiError::Magic,
            4..=5 => AbiError::Version,
            6..=7 => AbiError::Profile,
            8..=11 => AbiError::Length,
            12..=15 => AbiError::Reserved,
            _ => unreachable!(),
        };
        assert_eq!(decode_callback(&malformed), Err(expected));
    }
    for version in [0, 2, 3, 255] {
        let mut malformed = GOLDEN;
        malformed[4] = version;
        assert_eq!(decode_callback(&malformed), Err(AbiError::Version));
    }
}

#[test]
fn every_nonzero_byte_of_the_final_reserved_word_is_rejected() {
    for offset in 60..64 {
        let mut malformed = GOLDEN;
        malformed[offset] = 1;
        assert_eq!(decode_callback(&malformed), Err(AbiError::Reserved));
    }
    let mut output = [0xff; 64];
    encode_callback(&golden_record(), &mut output).unwrap();
    assert_eq!(&output[60..64], &[0; 4]);
}

#[test]
fn exact_length_takes_priority_over_invalid_header_and_record_fields() {
    let valid = golden_record();
    let mut invalid = valid;
    invalid.token = 0;
    for length in (0..64).chain([65, 128]) {
        for record in [&valid, &invalid] {
            let mut output = vec![0xa5; length];
            assert_eq!(encode_callback(record, &mut output), Err(AbiError::Length));
            assert_eq!(output, vec![0xa5; length]);
        }
        let mut input = GOLDEN.to_vec();
        input[0] = 0;
        input.resize(length, 0);
        assert_eq!(decode_callback(&input), Err(AbiError::Length));
    }
}
