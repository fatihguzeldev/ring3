use ring3_engine::abi::AbiError;
use ring3_engine::abi::call_frame::{
    CALL_FRAME_SIZE, CallRecord32, decode_call_frame, encode_call_frame,
};

const GOLDEN: [u8; 112] = [
    0x52, 0x33, 0x43, 0x46, 0x01, 0x00, 0x01, 0x00, 0x70, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x44, 0x33, 0x22, 0x11, 0x88, 0x77, 0x66, 0x55, 0x03, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00,
    0xcc, 0xbb, 0xaa, 0x99, 0x01, 0xff, 0xee, 0xdd, 0x78, 0x56, 0x34, 0x12, 0x21, 0x43, 0x65, 0x87,
    0xef, 0xbe, 0xad, 0xde, 0xc0, 0xd0, 0xe0, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

fn golden_record() -> CallRecord32 {
    let mut arguments = [0; 16];
    arguments[0] = 0xdead_beef;
    arguments[1] = 0xf0e0_d0c0;
    CallRecord32 {
        token: 0x1122_3344,
        id: 0x5566_7788,
        convention: 3,
        stack_words: 2,
        gate_pc: 0x99aa_bbcc,
        entry_esp: 0xddee_ff01,
        return_pc: 0x1234_5678,
        this_pointer: 0x8765_4321,
        arguments,
    }
}

fn set_word(bytes: &mut [u8; 112], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn literal(record: &CallRecord32) -> [u8; 112] {
    let mut bytes = [0; 112];
    bytes[..16].copy_from_slice(&[0x52, 0x33, 0x43, 0x46, 1, 0, 1, 0, 112, 0, 0, 0, 0, 0, 0, 0]);
    for (index, value) in [
        record.token,
        record.id,
        record.convention,
        record.stack_words,
        record.gate_pc,
        record.entry_esp,
        record.return_pc,
        record.this_pointer,
    ]
    .into_iter()
    .enumerate()
    {
        set_word(&mut bytes, 16 + index * 4, value);
    }
    for (index, value) in record.arguments.iter().enumerate() {
        set_word(&mut bytes, 48 + index * 4, *value);
    }
    bytes
}

fn assert_atomic_rejection(record: &CallRecord32) {
    let mut output = [0xa5; 112];
    assert_eq!(
        encode_call_frame(record, &mut output),
        Err(AbiError::CallFrame)
    );
    assert_eq!(output, [0xa5; 112]);
    assert_eq!(
        decode_call_frame(&literal(record)),
        Err(AbiError::CallFrame)
    );
}

#[test]
fn integer_call_record_matches_an_independent_full_wire_literal() {
    assert_eq!(CALL_FRAME_SIZE, 112);
    let expected = golden_record();
    let mut output = [0xa5; 112];
    encode_call_frame(&expected, &mut output).unwrap();
    assert_eq!(output, GOLDEN);
    assert_eq!(decode_call_frame(&GOLDEN), Ok(expected));
}

#[test]
fn all_conventions_zero_one_and_sixteen_words_have_canonical_fields() {
    for convention in 1..=3 {
        for count in [0, 1, 16] {
            let mut record = golden_record();
            record.token = u32::MAX;
            record.id = u32::MAX;
            record.convention = convention;
            record.stack_words = count;
            record.gate_pc = 0;
            record.entry_esp = u32::MAX;
            record.return_pc = 0;
            record.this_pointer = if convention == 3 { 0xfedc_ba98 } else { 0 };
            record.arguments.fill(0);
            for index in 0..count as usize {
                record.arguments[index] = 0x1234_5600 + index as u32;
            }
            let mut output = [0xa5; 112];
            encode_call_frame(&record, &mut output).unwrap();
            assert_eq!(output, literal(&record));
            assert_eq!(decode_call_frame(&literal(&record)), Ok(record));
        }
    }
    let mut thiscall = golden_record();
    thiscall.this_pointer = 0;
    let mut output = [0xa5; 112];
    encode_call_frame(&thiscall, &mut output).unwrap();
    assert_eq!(decode_call_frame(&literal(&thiscall)), Ok(thiscall));
}

#[test]
fn zero_token_zero_id_unknown_convention_and_oversized_count_are_atomic_errors() {
    for (field, values) in [
        (16, &[0][..]),
        (20, &[0][..]),
        (24, &[0, 4, u32::MAX][..]),
        (28, &[17, u32::MAX][..]),
    ] {
        for value in values {
            let mut record = golden_record();
            match field {
                16 => record.token = *value,
                20 => record.id = *value,
                24 => record.convention = *value,
                28 => record.stack_words = *value,
                _ => unreachable!(),
            }
            assert_atomic_rejection(&record);
        }
    }
}

#[test]
fn this_pointer_must_be_zero_outside_thiscall() {
    for convention in [1, 2] {
        let mut record = golden_record();
        record.convention = convention;
        for this_pointer in [1, u32::MAX] {
            record.this_pointer = this_pointer;
            assert_atomic_rejection(&record);
        }
    }
}

#[test]
fn every_unused_argument_word_is_rejected_without_normalizing_it() {
    for count in [0, 2, 15] {
        for index in count as usize..16 {
            let mut record = golden_record();
            record.stack_words = count;
            record.arguments.fill(0);
            record.arguments[index] = 0xdead_beef;
            assert_atomic_rejection(&record);
        }
    }
}

#[test]
fn malformed_header_bytes_keep_the_shared_field_error_precedence() {
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
        assert_eq!(decode_call_frame(&malformed), Err(expected));
    }
    for version in [0, 2, 3, 255] {
        let mut malformed = GOLDEN;
        malformed[4] = version;
        assert_eq!(decode_call_frame(&malformed), Err(AbiError::Version));
    }
}

#[test]
fn exact_length_is_required_before_invalid_field_or_header_validation() {
    let valid = golden_record();
    let mut invalid = valid;
    invalid.token = 0;
    for length in (0..112).chain([113, 224]) {
        for record in [&valid, &invalid] {
            let mut output = vec![0xa5; length];
            assert_eq!(
                encode_call_frame(record, &mut output),
                Err(AbiError::Length)
            );
            assert_eq!(output, vec![0xa5; length]);
        }
        let mut input = GOLDEN.to_vec();
        input[0] = 0;
        input.resize(length, 0);
        assert_eq!(decode_call_frame(&input), Err(AbiError::Length));
    }
}

#[test]
fn canonical_encoding_overwrites_the_complete_output_and_retains_raw_scalar_addresses() {
    let mut record = golden_record();
    record.gate_pc = u32::MAX;
    record.entry_esp = 0;
    record.return_pc = u32::MAX;
    record.arguments[0] = 0;
    record.arguments[1] = u32::MAX;
    let mut output = [0xff; 112];
    encode_call_frame(&record, &mut output).unwrap();
    assert_eq!(output, literal(&record));
    assert_eq!(&output[56..], &[0; 56]);
    assert_eq!(decode_call_frame(&output), Ok(record));
}
