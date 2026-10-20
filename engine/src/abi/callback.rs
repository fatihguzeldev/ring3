#![forbid(unsafe_code)]

use super::{AbiError, header};
use crate::windows::MAX_STACK_WORDS;

pub const CALLBACK_RECORD_SIZE: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallbackRecord32 {
    pub token: u32,
    pub outer_token: u32,
    pub phase: u32,
    pub outcome: u32,
    pub entry_pc: u32,
    pub entry_esp: u32,
    pub return_pc: u32,
    pub return_id: u32,
    pub stack_words: u32,
    pub result: u32,
    pub generation: u32,
}

pub fn encode_callback(record: &CallbackRecord32, output: &mut [u8]) -> Result<(), AbiError> {
    encode(record, output, false)
}

pub fn encode_core_callback(record: &CallbackRecord32, output: &mut [u8]) -> Result<(), AbiError> {
    encode(record, output, true)
}

fn encode(record: &CallbackRecord32, output: &mut [u8], core: bool) -> Result<(), AbiError> {
    if output.len() != CALLBACK_RECORD_SIZE {
        return Err(AbiError::Length);
    }
    validate(record, core)?;
    header::write_version(output, *b"R3CB", if core { 2 } else { 1 });
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
        header::write_u32(output, 16 + index * 4, value);
    }
    Ok(())
}

pub fn decode_callback(input: &[u8]) -> Result<CallbackRecord32, AbiError> {
    decode(input, false)
}

pub fn decode_core_callback(input: &[u8]) -> Result<CallbackRecord32, AbiError> {
    decode(input, true)
}

fn decode(input: &[u8], core: bool) -> Result<CallbackRecord32, AbiError> {
    header::validate_versions(
        input,
        *b"R3CB",
        CALLBACK_RECORD_SIZE,
        &[if core { 2 } else { 1 }],
    )?;
    if header::read_u32(input, 60) != 0 {
        return Err(AbiError::Reserved);
    }
    let record = CallbackRecord32 {
        token: header::read_u32(input, 16),
        outer_token: header::read_u32(input, 20),
        phase: header::read_u32(input, 24),
        outcome: header::read_u32(input, 28),
        entry_pc: header::read_u32(input, 32),
        entry_esp: header::read_u32(input, 36),
        return_pc: header::read_u32(input, 40),
        return_id: header::read_u32(input, 44),
        stack_words: header::read_u32(input, 48),
        result: header::read_u32(input, 52),
        generation: header::read_u32(input, 56),
    };
    validate(&record, core)?;
    Ok(record)
}

fn validate(record: &CallbackRecord32, core: bool) -> Result<(), AbiError> {
    if record.token == 0
        || record.outer_token == 0
        || record.token <= record.outer_token
        || if core {
            record.generation != 0
        } else {
            record.generation == 0
        }
        || record.return_id == 0
        || record.stack_words > MAX_STACK_WORDS as u32
        || record.entry_pc == record.return_pc
    {
        return Err(AbiError::CallbackFrame);
    }
    match record.phase {
        1 if record.outcome <= 1 && record.result == 0 => Ok(()),
        2 if record.outcome == 0 => Ok(()),
        _ => Err(AbiError::CallbackFrame),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn core_record() -> CallbackRecord32 {
        CallbackRecord32 {
            token: 9,
            outer_token: 8,
            phase: 1,
            outcome: 0,
            entry_pc: 0x1000,
            entry_esp: 0x47f4,
            return_pc: 0x2000,
            return_id: 0xffffe,
            stack_words: 2,
            result: 0,
            generation: 0,
        }
    }

    fn fields(record: CallbackRecord32) -> [u32; 11] {
        [
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
    }

    #[test]
    fn core_begin_has_literal_v2_wire_fields_and_roundtrips() {
        let mut bytes = [0xa5; CALLBACK_RECORD_SIZE];
        encode_core_callback(&core_record(), &mut bytes).unwrap();
        assert_eq!(
            &bytes[..16],
            b"R3CB\x02\x00\x01\x00\x40\x00\x00\x00\x00\x00\x00\x00"
        );
        let wanted = [9, 8, 1, 0, 0x1000, 0x47f4, 0x2000, 0xffffe, 2, 0, 0];
        for (index, value) in wanted.into_iter().enumerate() {
            assert_eq!(
                u32::from_le_bytes(bytes[16 + index * 4..20 + index * 4].try_into().unwrap()),
                value
            );
        }
        assert_eq!(&bytes[60..], &[0; 4]);
        assert_eq!(fields(decode_core_callback(&bytes).unwrap()), wanted);
    }

    #[test]
    fn core_finish_preserves_the_result_and_zero_generation() {
        let record = CallbackRecord32 {
            phase: 2,
            result: 0xfedcba98,
            ..core_record()
        };
        let mut bytes = [0; CALLBACK_RECORD_SIZE];
        encode_core_callback(&record, &mut bytes).unwrap();
        assert_eq!(&bytes[52..56], &[0x98, 0xba, 0xdc, 0xfe]);
        assert_eq!(&bytes[56..64], &[0; 8]);
        assert_eq!(
            fields(decode_core_callback(&bytes).unwrap()),
            [
                9, 8, 2, 0, 0x1000, 0x47f4, 0x2000, 0xffffe, 2, 0xfedcba98, 0
            ]
        );
    }

    #[test]
    fn core_and_replacement_versions_cannot_be_interchanged() {
        let mut core = [0; CALLBACK_RECORD_SIZE];
        let mut replacement = [0; CALLBACK_RECORD_SIZE];
        encode_core_callback(&core_record(), &mut core).unwrap();
        let record = CallbackRecord32 {
            generation: 7,
            ..core_record()
        };
        encode_callback(&record, &mut replacement).unwrap();
        assert!(decode_callback(&core).is_err());
        assert!(decode_core_callback(&replacement).is_err());
        assert_eq!(
            fields(decode_callback(&replacement).unwrap()),
            fields(record)
        );
        core[4] = 1;
        replacement[4] = 2;
        assert!(decode_callback(&core).is_err());
        assert!(decode_core_callback(&replacement).is_err());
    }

    #[test]
    fn rejected_core_encode_never_changes_the_output() {
        let original = core_record();
        for invalid in [
            CallbackRecord32 {
                generation: 1,
                ..original
            },
            CallbackRecord32 {
                token: 0,
                ..original
            },
            CallbackRecord32 {
                token: 8,
                ..original
            },
            CallbackRecord32 {
                outer_token: 0,
                ..original
            },
            CallbackRecord32 {
                return_pc: 0x1000,
                ..original
            },
            CallbackRecord32 {
                return_id: 0,
                ..original
            },
            CallbackRecord32 {
                stack_words: MAX_STACK_WORDS as u32 + 1,
                ..original
            },
            CallbackRecord32 {
                phase: 3,
                ..original
            },
            CallbackRecord32 {
                result: 1,
                ..original
            },
            CallbackRecord32 {
                outcome: 2,
                ..original
            },
            CallbackRecord32 {
                phase: 2,
                outcome: 1,
                ..original
            },
        ] {
            let mut bytes = [0xa5; CALLBACK_RECORD_SIZE];
            assert!(encode_core_callback(&invalid, &mut bytes).is_err());
            assert_eq!(bytes, [0xa5; CALLBACK_RECORD_SIZE]);
        }
        let mut bytes = [0xa5; CALLBACK_RECORD_SIZE];
        assert!(encode_callback(&original, &mut bytes).is_err());
        assert_eq!(bytes, [0xa5; CALLBACK_RECORD_SIZE]);
    }

    #[test]
    fn core_decode_rejects_header_reserved_and_payload_corruption() {
        let mut encoded = [0; CALLBACK_RECORD_SIZE];
        encode_core_callback(&core_record(), &mut encoded).unwrap();
        for (offset, value) in [(8, 63_u32), (12, 1), (16, 8), (56, 1), (60, 1)] {
            let mut bytes = encoded;
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(decode_core_callback(&bytes).is_err(), "offset={offset}");
        }
        let mut profile = encoded;
        profile[6] = 2;
        assert!(decode_core_callback(&profile).is_err());
        assert!(decode_core_callback(&encoded[..63]).is_err());
    }
}
