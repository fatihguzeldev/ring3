use ring3_engine::abi::{
    AbiError,
    callback::{CallbackRecord32, decode_callback, encode_callback},
    resident_callback::{
        RESIDENT_CALLBACK_RESULT_SIZE, ResidentCallbackRecord32, ResidentCallbackResult32,
        decode_resident_callback, decode_resident_callback_result, encode_resident_callback,
        encode_resident_callback_result,
    },
};

fn words(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn result_record(result: u32) -> ResidentCallbackResult32 {
    ResidentCallbackResult32 {
        token: 0xffff_ffff,
        outer_token: 0x8000_0001,
        result,
        outer_unit_id: 0x1020_3040_ffff_ffff,
        callback_unit_id: 0xaabb_ccdd_8000_0000,
    }
}

fn result_literal(result: u32) -> Vec<u8> {
    words(&[
        u32::from_le_bytes(*b"R3RR"),
        0x10001,
        48,
        0,
        0xffff_ffff,
        0x8000_0001,
        result,
        0,
        0xffff_ffff,
        0x1020_3040,
        0x8000_0000,
        0xaabb_ccdd,
    ])
}

#[test]
fn resident_result_has_an_independent_full_width_literal_and_preserves_old_record_rules() {
    assert_eq!(RESIDENT_CALLBACK_RESULT_SIZE, 48);
    for result in [0, 0xffff_ffff, 0x8000_0001] {
        let record = result_record(result);
        let literal = result_literal(result);
        let mut encoded = [0xa5; 48];
        encode_resident_callback_result(&record, &mut encoded).unwrap();
        assert_eq!(encoded.as_slice(), literal);
        assert_eq!(decode_resident_callback_result(&literal), Ok(record));
    }
    for callback_unit_id in [0xaabb_ccdd_ffff_ffff_u64, 0xaabb_ccdd_0000_0000] {
        let record = ResidentCallbackResult32 {
            callback_unit_id,
            ..result_record(42)
        };
        let mut literal = result_literal(42);
        literal[40..44].copy_from_slice(&(callback_unit_id as u32).to_le_bytes());
        literal[44..48].copy_from_slice(&0xaabb_ccdd_u32.to_le_bytes());
        let mut encoded = [0xa5; 48];
        encode_resident_callback_result(&record, &mut encoded).unwrap();
        assert_eq!(encoded.as_slice(), literal);
        assert_eq!(decode_resident_callback_result(&literal), Ok(record));
    }

    let admitted = ResidentCallbackRecord32 {
        token: 2,
        outer_token: 1,
        phase: 1,
        outcome: 3,
        entry_pc: 0x2000,
        entry_esp: 0x8ffc,
        return_pc: 0x2100,
        return_id: 18,
        stack_words: 0,
        result: 0,
        outer_unit_id: 0x1020_3040_ffff_ffff,
        callback_unit_id: 0xaabb_ccdd_8000_0000,
    };
    let admission_literal = words(&[
        u32::from_le_bytes(*b"R3RC"),
        0x10001,
        72,
        0,
        2,
        1,
        1,
        3,
        0x2000,
        0x8ffc,
        0x2100,
        18,
        0,
        0,
        0xffff_ffff,
        0x1020_3040,
        0x8000_0000,
        0xaabb_ccdd,
    ]);
    let mut encoded_admission = [0xa5; 72];
    encode_resident_callback(&admitted, &mut encoded_admission).unwrap();
    assert_eq!(encoded_admission.as_slice(), admission_literal);
    assert_eq!(decode_resident_callback(&admission_literal), Ok(admitted));
    for (offset, value) in [(24, 2u32), (52, 1)] {
        let mut invalid = admission_literal.clone();
        invalid[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert_eq!(
            decode_resident_callback(&invalid),
            Err(AbiError::CallbackFrame)
        );
        let mut record = admitted;
        if offset == 24 {
            record.phase = value;
        } else {
            record.result = value;
        }
        let mut output = [0xa5; 72];
        assert_eq!(
            encode_resident_callback(&record, &mut output),
            Err(AbiError::CallbackFrame)
        );
        assert_eq!(output, [0xa5; 72]);
    }

    let legacy = CallbackRecord32 {
        token: 2,
        outer_token: 1,
        phase: 2,
        outcome: 0,
        entry_pc: 0x2000,
        entry_esp: 0x8ffc,
        return_pc: 0x2100,
        return_id: 18,
        stack_words: 0,
        result: 0xffff_ffff,
        generation: 7,
    };
    let legacy_literal = words(&[
        u32::from_le_bytes(*b"R3CB"),
        0x10001,
        64,
        0,
        2,
        1,
        2,
        0,
        0x2000,
        0x8ffc,
        0x2100,
        18,
        0,
        0xffff_ffff,
        7,
        0,
    ]);
    let mut encoded_legacy = [0xa5; 64];
    encode_callback(&legacy, &mut encoded_legacy).unwrap();
    assert_eq!(encoded_legacy.as_slice(), legacy_literal);
    assert_eq!(decode_callback(&legacy_literal), Ok(legacy));
    assert_eq!(
        decode_resident_callback_result(&admission_literal),
        Err(AbiError::Length)
    );
    assert_eq!(
        decode_resident_callback(&result_literal(42)),
        Err(AbiError::Length)
    );
    assert_eq!(decode_callback(&result_literal(42)), Err(AbiError::Length));
}

#[test]
fn resident_result_rejects_malformed_headers_reserved_fields_and_identity_without_writes() {
    let valid = result_literal(42);
    for length in [47, 49] {
        let mut input = valid.clone();
        input.resize(length, 0);
        assert_eq!(
            decode_resident_callback_result(&input),
            Err(AbiError::Length)
        );
    }
    for (offset, value, error) in [
        (0, 0u32, AbiError::Magic),
        (4, 0x10002, AbiError::Version),
        (4, 0x20001, AbiError::Profile),
        (8, 47, AbiError::Length),
        (12, 1, AbiError::Reserved),
        (28, 1, AbiError::Reserved),
    ] {
        let mut input = valid.clone();
        input[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        input[16..20].fill(0);
        assert_eq!(decode_resident_callback_result(&input), Err(error));
    }

    let good = result_record(42);
    let invalid_records = [
        ResidentCallbackResult32 { token: 0, ..good },
        ResidentCallbackResult32 {
            outer_token: 0,
            ..good
        },
        ResidentCallbackResult32 {
            token: good.outer_token,
            ..good
        },
        ResidentCallbackResult32 {
            token: good.outer_token - 1,
            ..good
        },
        ResidentCallbackResult32 {
            outer_unit_id: 0,
            ..good
        },
        ResidentCallbackResult32 {
            callback_unit_id: 0,
            ..good
        },
        ResidentCallbackResult32 {
            callback_unit_id: good.outer_unit_id,
            ..good
        },
    ];
    for record in invalid_records {
        let mut output = [0x5a; 48];
        assert_eq!(
            encode_resident_callback_result(&record, &mut output),
            Err(AbiError::CallbackFrame)
        );
        assert_eq!(output, [0x5a; 48]);
        let mut input = valid.clone();
        for (offset, value) in [
            (16, record.token),
            (20, record.outer_token),
            (32, record.outer_unit_id as u32),
            (36, (record.outer_unit_id >> 32) as u32),
            (40, record.callback_unit_id as u32),
            (44, (record.callback_unit_id >> 32) as u32),
        ] {
            input[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            decode_resident_callback_result(&input),
            Err(AbiError::CallbackFrame)
        );
        let mut short = [0x5a; 47];
        assert_eq!(
            encode_resident_callback_result(&record, &mut short),
            Err(AbiError::Length)
        );
        assert_eq!(short, [0x5a; 47]);
    }
    for length in [47, 49] {
        let mut output = vec![0x5a; length];
        assert_eq!(
            encode_resident_callback_result(&good, &mut output),
            Err(AbiError::Length)
        );
        assert_eq!(output, vec![0x5a; length]);
    }
}
