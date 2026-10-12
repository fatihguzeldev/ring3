use ring3_engine::abi::{
    AbiError,
    callback::{CallbackRecord32, decode_callback, encode_callback},
    resident_callback::{
        RESIDENT_CALLBACK_RECORD_SIZE, ResidentCallbackRecord32, decode_resident_callback,
        encode_resident_callback,
    },
};

fn record() -> ResidentCallbackRecord32 {
    ResidentCallbackRecord32 {
        token: 0x0102_0305,
        outer_token: 0x0102_0304,
        phase: 1,
        outcome: 0,
        entry_pc: 0x1122_3344,
        entry_esp: 0xffff_fff0,
        return_pc: 0x5566_7788,
        return_id: 0x8000_0011,
        stack_words: 16,
        result: 0,
        outer_unit_id: 0x1020_3040_5060_7080,
        callback_unit_id: 0x90a0_b0c0_d0e0_f001,
    }
}

fn words(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn literal(outcome: u32) -> Vec<u8> {
    words(&[
        u32::from_le_bytes(*b"R3RC"),
        0x10001,
        72,
        0,
        0x0102_0305,
        0x0102_0304,
        1,
        outcome,
        0x1122_3344,
        0xffff_fff0,
        0x5566_7788,
        0x8000_0011,
        16,
        0,
        0x5060_7080,
        0x1020_3040,
        0xd0e0_f001,
        0x90a0_b0c0,
    ])
}

#[test]
fn resident_record_has_exact_full_width_literal_bytes_and_preserves_legacy64() {
    assert_eq!(RESIDENT_CALLBACK_RECORD_SIZE, 72);
    for outcome in 0..=3 {
        let r = ResidentCallbackRecord32 {
            outcome,
            ..record()
        };
        let mut bytes = [0xa5; 72];
        encode_resident_callback(&r, &mut bytes).unwrap();
        assert_eq!(bytes.as_slice(), literal(outcome));
        assert_eq!(decode_resident_callback(&literal(outcome)), Ok(r));
    }
    let mut alias = record();
    alias.callback_unit_id = alias.outer_unit_id;
    alias.token = u32::MAX;
    alias.outer_token = u32::MAX - 1;
    alias.stack_words = 0;
    let mut bytes = [0; 72];
    encode_resident_callback(&alias, &mut bytes).unwrap();
    let mut wanted = literal(0);
    wanted[16..24].copy_from_slice(&words(&[u32::MAX, u32::MAX - 1]));
    wanted[48..52].fill(0);
    wanted[64..72].copy_from_slice(&words(&[0x5060_7080, 0x1020_3040]));
    assert_eq!(bytes.as_slice(), wanted);
    assert_eq!(decode_resident_callback(&wanted), Ok(alias));
    let old = CallbackRecord32 {
        token: 0x0102_0305,
        outer_token: 0x0102_0304,
        phase: 1,
        outcome: 1,
        entry_pc: 0x1122_3344,
        entry_esp: 0xffff_fff0,
        return_pc: 0x5566_7788,
        return_id: 0x8000_0011,
        stack_words: 16,
        result: 0,
        generation: 7,
    };
    let old_literal = words(&[
        u32::from_le_bytes(*b"R3CB"),
        0x10001,
        64,
        0,
        0x0102_0305,
        0x0102_0304,
        1,
        1,
        0x1122_3344,
        0xffff_fff0,
        0x5566_7788,
        0x8000_0011,
        16,
        0,
        7,
        0,
    ]);
    let mut old_bytes = [0xa5; 64];
    encode_callback(&old, &mut old_bytes).unwrap();
    assert_eq!(old_bytes.as_slice(), old_literal);
    assert_eq!(decode_callback(&old_literal), Ok(old));
    assert_eq!(decode_callback(&literal(0)), Err(AbiError::Length));
    assert_eq!(
        decode_resident_callback(&old_literal),
        Err(AbiError::Length)
    );
}

#[test]
fn resident_record_rejects_malformed_headers_fields_and_encoder_mutation() {
    for length in [0, 64, 71, 73] {
        let mut bytes = vec![0xa5; length];
        let before = bytes.clone();
        assert_eq!(
            encode_resident_callback(&record(), &mut bytes),
            Err(AbiError::Length)
        );
        assert_eq!(bytes, before);
        assert_eq!(decode_resident_callback(&bytes), Err(AbiError::Length));
    }
    for (offset, value, error) in [
        (0, u32::from_le_bytes(*b"R3CB"), AbiError::Magic),
        (4, 0x10002, AbiError::Version),
        (4, 0x20001, AbiError::Profile),
        (8, 64, AbiError::Length),
        (12, 1, AbiError::Reserved),
    ] {
        let mut bytes = literal(0);
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert_eq!(decode_resident_callback(&bytes), Err(error));
    }
    let mutations: [fn(&mut ResidentCallbackRecord32); 13] = [
        |r| r.token = 0,
        |r| r.outer_token = 0,
        |r| r.token = r.outer_token,
        |r| r.token = r.outer_token - 1,
        |r| r.phase = 0,
        |r| r.phase = 2,
        |r| r.outcome = 4,
        |r| r.result = 1,
        |r| r.outer_unit_id = 0,
        |r| r.callback_unit_id = 0,
        |r| r.return_id = 0,
        |r| r.stack_words = 17,
        |r| r.entry_pc = r.return_pc,
    ];
    for mutate in mutations {
        let mut bad = record();
        mutate(&mut bad);
        let mut output = [0xa5; 72];
        assert_eq!(
            encode_resident_callback(&bad, &mut output),
            Err(AbiError::CallbackFrame)
        );
        assert_eq!(output, [0xa5; 72]);
    }
    for (offset, value) in [
        (16, 0_u32),
        (20, 0),
        (16, 0x0102_0304),
        (16, 0x0102_0303),
        (24, 0),
        (24, 2),
        (28, 4),
        (44, 0),
        (48, 17),
        (52, 1),
        (32, 0x5566_7788),
    ] {
        let mut bytes = literal(0);
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert_eq!(
            decode_resident_callback(&bytes),
            Err(AbiError::CallbackFrame)
        );
    }
    for range in [56..64, 64..72] {
        let mut bytes = literal(0);
        bytes[range].fill(0);
        assert_eq!(
            decode_resident_callback(&bytes),
            Err(AbiError::CallbackFrame)
        );
    }
}
