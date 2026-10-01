use ring3_engine::abi::AbiError;
use ring3_engine::abi::x86::{
    EFLAGS_OFFSET, EIP_OFFSET, REGISTERS_OFFSET, STATE_SIZE, decode_state, encode_state,
};
use ring3_engine::cpu::x86::State32;

const GOLDEN: [u8; 56] = [
    0x52, 0x33, 0x53, 0x54, 0x01, 0x00, 0x01, 0x00, 0x38, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x01, 0x02, 0x03, 0x10, 0x11, 0x12, 0x13, 0x20, 0x21, 0x22, 0x23, 0x30, 0x31, 0x32, 0x33,
    0x40, 0x41, 0x42, 0x43, 0x50, 0x51, 0x52, 0x53, 0x60, 0x61, 0x62, 0x63, 0x70, 0x71, 0x72, 0x73,
    0x80, 0x81, 0x82, 0x83, 0xd7, 0x0c, 0x00, 0x00,
];

fn golden_state() -> State32 {
    State32 {
        registers: [
            0x0302_0100,
            0x1312_1110,
            0x2322_2120,
            0x3332_3130,
            0x4342_4140,
            0x5352_5150,
            0x6362_6160,
            0x7372_7170,
        ],
        eip: 0x8382_8180,
        eflags: 0x0cd7,
    }
}

fn assert_state(actual: &State32, expected: &State32) {
    assert_eq!(actual.registers, expected.registers);
    assert_eq!(actual.eip, expected.eip);
    assert_eq!(actual.eflags, expected.eflags);
}

#[test]
fn default_state_has_zero_registers_and_fixed_flag_bit() {
    let state = State32::default();
    assert_eq!(state.registers, [0; 8]);
    assert_eq!(state.eip, 0);
    assert_eq!(state.eflags, 2);
}

#[test]
fn public_offsets_and_size_match_the_byte_contract() {
    assert_eq!(STATE_SIZE, 56);
    assert_eq!(REGISTERS_OFFSET, 16);
    assert_eq!(EIP_OFFSET, 48);
    assert_eq!(EFLAGS_OFFSET, 52);
}

#[test]
fn encoding_matches_independent_literal_record() {
    let mut output = [0xa5; 56];
    encode_state(&golden_state(), &mut output).unwrap();
    assert_eq!(output, GOLDEN);
}

#[test]
fn decoding_literal_preserves_every_register_in_contract_order() {
    let decoded = decode_state(&GOLDEN).unwrap();
    let expected = golden_state();
    assert_state(&decoded, &expected);
    for (index, register) in expected.registers.into_iter().enumerate() {
        let start = 16 + index * 4;
        assert_eq!(&GOLDEN[start..start + 4], &register.to_le_bytes());
    }
    assert_eq!(&GOLDEN[48..52], &expected.eip.to_le_bytes());
    assert_eq!(&GOLDEN[52..56], &expected.eflags.to_le_bytes());
}

#[test]
fn successful_roundtrip_preserves_default_and_distinctive_states() {
    for original in [State32::default(), golden_state()] {
        let mut record = [0; 56];
        encode_state(&original, &mut record).unwrap();
        let decoded = decode_state(&record).unwrap();
        assert_state(&decoded, &original);
    }
}

#[test]
fn every_truncation_and_oversized_input_is_rejected() {
    for length in 0..56 {
        assert_eq!(
            decode_state(&GOLDEN[..length]).err(),
            Some(AbiError::Length)
        );
    }
    for length in [57, 64, 112] {
        let mut record = vec![0; length];
        record[..56].copy_from_slice(&GOLDEN);
        assert_eq!(decode_state(&record).err(), Some(AbiError::Length));
    }
}

#[test]
fn invalid_output_lengths_preserve_every_sentinel_byte() {
    for length in (0..56).chain([57, 64, 112]) {
        let mut output = vec![0xa5; length];
        assert_eq!(
            encode_state(&golden_state(), &mut output),
            Err(AbiError::Length)
        );
        assert!(output.iter().all(|byte| *byte == 0xa5));
    }
}

#[test]
fn each_malformed_header_byte_returns_its_field_error() {
    for offset in 0..16 {
        let mut record = GOLDEN;
        record[offset] ^= 0x80;
        let expected = match offset {
            0..=3 => AbiError::Magic,
            4..=5 => AbiError::Version,
            6..=7 => AbiError::Profile,
            8..=11 => AbiError::Length,
            12..=15 => AbiError::Reserved,
            _ => unreachable!(),
        };
        assert_eq!(decode_state(&record).err(), Some(expected));
    }
}

#[test]
fn every_supported_mutable_flag_bit_roundtrips_without_masking() {
    for flags in [2, 0x0cd7] {
        let original = State32 {
            eflags: flags,
            ..State32::default()
        };
        let mut record = [0; 56];
        encode_state(&original, &mut record).unwrap();
        assert_eq!(decode_state(&record).unwrap().eflags, flags);
    }
    for bit in [0, 2, 4, 6, 7, 10, 11] {
        let flags = 2 | (1 << bit);
        let original = State32 {
            eflags: flags,
            ..State32::default()
        };
        let mut record = [0; 56];
        encode_state(&original, &mut record).unwrap();
        assert_eq!(&record[52..56], &flags.to_le_bytes());
        assert_eq!(decode_state(&record).unwrap().eflags, flags);
    }
}

#[test]
fn each_unsupported_flag_bit_is_rejected_by_encode_and_decode() {
    for bit in 0..32 {
        if 0x0cd7 & (1u32 << bit) != 0 {
            continue;
        }
        let flags = 2 | (1u32 << bit);
        let invalid = State32 {
            eflags: flags,
            ..State32::default()
        };
        let mut output = [0xa5; 56];
        assert_eq!(encode_state(&invalid, &mut output), Err(AbiError::Flags));
        assert_eq!(output, [0xa5; 56]);
        let mut record = GOLDEN;
        record[52..56].copy_from_slice(&flags.to_le_bytes());
        assert_eq!(decode_state(&record).err(), Some(AbiError::Flags));
    }
}

#[test]
fn absent_fixed_flag_bit_is_rejected_without_repairing_it() {
    for flags in [0u32, 1, 0x0cd5] {
        let invalid = State32 {
            eflags: flags,
            ..State32::default()
        };
        let mut output = [0xa5; 56];
        assert_eq!(encode_state(&invalid, &mut output), Err(AbiError::Flags));
        assert_eq!(output, [0xa5; 56]);
        let mut record = GOLDEN;
        record[52..56].copy_from_slice(&flags.to_le_bytes());
        assert_eq!(decode_state(&record).err(), Some(AbiError::Flags));
    }
}

#[test]
fn length_errors_take_priority_over_malformed_state_and_header() {
    let invalid = State32 {
        eflags: 0,
        ..State32::default()
    };
    for length in [0, 55, 57] {
        let mut output = vec![0xa5; length];
        assert_eq!(encode_state(&invalid, &mut output), Err(AbiError::Length));
        assert!(output.iter().all(|byte| *byte == 0xa5));
    }
    let mut malformed = GOLDEN;
    malformed[0] = 0;
    malformed[52..56].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(decode_state(&malformed[..55]).err(), Some(AbiError::Length));
    let mut oversized = malformed.to_vec();
    oversized.push(0);
    assert_eq!(decode_state(&oversized).err(), Some(AbiError::Length));
}
