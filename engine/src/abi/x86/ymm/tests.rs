use super::*;

#[test]
fn every_upper_bit_roundtrips_through_an_exact_strict_record() {
    let state = YmmUpperState {
        registers: std::array::from_fn(|r| std::array::from_fn(|b| (r * 31 + b * 17) as u8)),
    };
    let mut record = [0xa5; YMM_SIZE];
    encode_ymm(&state, &mut record).unwrap();
    assert_eq!(
        &record[..16],
        b"R3YU\x01\x00\x01\x00\x90\x00\x00\x00\x00\x00\x00\x00"
    );
    assert_eq!(decode_ymm(&record), Ok(state));
    for (index, byte) in record[16..].iter().enumerate() {
        assert_eq!(*byte, state.registers[index / 16][index % 16]);
    }
    for offset in 0..16 {
        let mut malformed = record;
        malformed[offset] ^= 0x80;
        assert!(decode_ymm(&malformed).is_err(), "header byte {offset}");
    }
    assert_eq!(decode_ymm(&record[..143]), Err(AbiError::Length));
    for length in [0, 143, 145] {
        let mut output = vec![0x6d; length];
        let before = output.clone();
        assert_eq!(encode_ymm(&state, &mut output), Err(AbiError::Length));
        assert_eq!(output, before);
    }
}
