use super::*;

#[test]
fn encoding_initializes_every_record_byte_without_depending_on_the_destination() {
    for seed in 0..32_u16 {
        let mut state = X87State {
            control: 0xa55a ^ seed,
            status: 0x93f1 ^ seed,
            tag: 0x2487 ^ seed,
            opcode: 0x7ff - seed,
            instruction_pointer: 0x1234_5678 ^ u32::from(seed),
            data_pointer: 0x8765_4321 ^ u32::from(seed),
            code_selector: 0x7654 ^ seed,
            data_selector: 0x4321 ^ seed,
            registers: [[0; 10]; 8],
        };
        for (index, byte) in state.registers.as_flattened_mut().iter_mut().enumerate() {
            *byte = (index as u8).wrapping_mul(37).wrapping_add(seed as u8);
        }
        let mut expected = [0; X87_SIZE];
        expected[..16]
            .copy_from_slice(&[b'R', b'3', b'F', b'P', 1, 0, 1, 0, 128, 0, 0, 0, 0, 0, 0, 0]);
        expected[16..18].copy_from_slice(&state.control.to_le_bytes());
        expected[18..20].copy_from_slice(&state.status.to_le_bytes());
        expected[20..22].copy_from_slice(&state.tag.to_le_bytes());
        expected[22..24].copy_from_slice(&state.opcode.to_le_bytes());
        expected[24..28].copy_from_slice(&state.instruction_pointer.to_le_bytes());
        expected[28..32].copy_from_slice(&state.data_pointer.to_le_bytes());
        expected[32..34].copy_from_slice(&state.code_selector.to_le_bytes());
        expected[34..36].copy_from_slice(&state.data_selector.to_le_bytes());
        expected[40..120].copy_from_slice(state.registers.as_flattened());
        for poison in [0, 0xa5, 0xff] {
            let mut output = [poison; X87_SIZE];
            encode_x87(&state, &mut output).unwrap();
            assert_eq!(output, expected);
            assert_eq!(decode_x87(&output), Ok(state));
        }
    }
}

#[test]
fn invalid_length_precedes_opcode_and_encoding_errors_leave_output_unchanged() {
    let state = X87State {
        opcode: 0x800,
        ..X87State::default()
    };
    for length in [0, 15, 127, 128, 129] {
        let mut output = vec![0xa5; length];
        let before = output.clone();
        assert_eq!(
            encode_x87(&state, &mut output),
            Err(if length == X87_SIZE {
                AbiError::Reserved
            } else {
                AbiError::Length
            })
        );
        assert_eq!(output, before);
    }
}

#[test]
fn encoded_record_still_rejects_nonzero_metadata_and_reserved_bytes() {
    let mut output = [0xa5; X87_SIZE];
    encode_x87(&X87State::default(), &mut output).unwrap();
    for offset in (12..16).chain(36..40).chain(120..128) {
        let mut malformed = output;
        malformed[offset] = 1;
        assert_eq!(decode_x87(&malformed), Err(AbiError::Reserved));
    }
}
