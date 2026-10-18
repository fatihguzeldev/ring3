use crate::abi::AbiError;
use crate::abi::header::{self, read_u32, write_u32};
use crate::cpu::x86::State32;

pub const STATE_SIZE: usize = 56;
pub const REGISTERS_OFFSET: usize = 16;
pub const EIP_OFFSET: usize = 48;
pub const EFLAGS_OFFSET: usize = 52;
pub const STATE_VERSION_2: u16 = 2;
pub const STATE_VERSION_3: u16 = 3;
pub const LEGACY_FLAGS_MASK: u32 = 0xcd7;
pub const USER_FLAGS_MASK: u32 = 0x244fd7;
pub const USER_FLAGS_MASK_V3: u32 = 0x254fd7;
pub const STATE_METADATA_OFFSET: usize = 12;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct StateMetadata {
    pub(crate) ss_debug_shadow: bool,
}

/// validation applies only while these same record bytes remain immutably borrowed.
pub(crate) struct ValidatedState<'a> {
    input: &'a [u8],
    metadata: StateMetadata,
    eflags: u32,
}

impl<'a> ValidatedState<'a> {
    pub(crate) fn decode(input: &'a [u8]) -> Result<Self, AbiError> {
        let (version, metadata) = validate_header(input)?;
        let eflags = read_u32(input, EFLAGS_OFFSET);
        validate_flags(eflags, version)?;
        Ok(Self {
            input,
            metadata: StateMetadata {
                ss_debug_shadow: metadata == 1,
            },
            eflags,
        })
    }

    pub(crate) fn state(&self) -> State32 {
        State32 {
            registers: std::array::from_fn(|index| {
                read_u32(self.input, REGISTERS_OFFSET + index * 4)
            }),
            eip: read_u32(self.input, EIP_OFFSET),
            eflags: self.eflags,
        }
    }

    pub(crate) fn metadata(&self) -> StateMetadata {
        self.metadata
    }
}

impl StateMetadata {
    pub(crate) fn write(self, state: &mut [u8; STATE_SIZE]) {
        if self.ss_debug_shadow && state[4..6] != STATE_VERSION_3.to_le_bytes() {
            state[4..6].copy_from_slice(&STATE_VERSION_2.to_le_bytes());
        }
        write_u32(
            state,
            STATE_METADATA_OFFSET,
            u32::from(self.ss_debug_shadow),
        );
    }
}

pub fn encode_state(state: &State32, output: &mut [u8]) -> Result<(), AbiError> {
    encode(state, output, crate::abi::ABI_VERSION)
}

pub fn encode_state_v2(state: &State32, output: &mut [u8]) -> Result<(), AbiError> {
    encode(state, output, STATE_VERSION_2)
}

pub fn encode_state_v3(state: &State32, output: &mut [u8]) -> Result<(), AbiError> {
    encode(state, output, STATE_VERSION_3)
}

pub(crate) fn encode_state_current(state: &State32, output: &mut [u8]) -> Result<(), AbiError> {
    if state.eflags & 0x10000 != 0
        || output.get(4..6) == Some(STATE_VERSION_3.to_le_bytes().as_slice())
    {
        encode_state_v3(state, output)
    } else if state.eflags & !LEGACY_FLAGS_MASK == 0 {
        encode_state(state, output)
    } else {
        encode_state_v2(state, output)
    }
}

fn encode(state: &State32, output: &mut [u8], version: u16) -> Result<(), AbiError> {
    if output.len() != STATE_SIZE {
        return Err(AbiError::Length);
    }
    validate_flags(state.eflags, version)?;
    header::write_version(output, *b"R3ST", version);
    for (index, value) in state.registers.iter().enumerate() {
        write_u32(output, REGISTERS_OFFSET + index * 4, *value);
    }
    write_u32(output, EIP_OFFSET, state.eip);
    write_u32(output, EFLAGS_OFFSET, state.eflags);
    Ok(())
}

pub fn decode_state(input: &[u8]) -> Result<State32, AbiError> {
    Ok(ValidatedState::decode(input)?.state())
}

pub(crate) fn decode_state_metadata(input: &[u8]) -> Result<StateMetadata, AbiError> {
    Ok(ValidatedState::decode(input)?.metadata())
}

fn validate_header(input: &[u8]) -> Result<(u16, u32), AbiError> {
    let (version, metadata) = header::validate_versions_with_metadata(
        input,
        *b"R3ST",
        STATE_SIZE,
        &[crate::abi::ABI_VERSION, STATE_VERSION_2, STATE_VERSION_3],
    )?;
    if metadata > u32::from(version != crate::abi::ABI_VERSION) {
        return Err(AbiError::Reserved);
    }
    Ok((version, metadata))
}

fn validate_flags(flags: u32, version: u16) -> Result<(), AbiError> {
    let mask = match version {
        STATE_VERSION_3 => USER_FLAGS_MASK_V3,
        STATE_VERSION_2 => USER_FLAGS_MASK,
        _ => LEGACY_FLAGS_MASK,
    };
    if flags & 2 == 0 || flags & !mask != 0 {
        return Err(AbiError::Flags);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ss_debug_shadow_is_validated_version_two_metadata() {
        let state = State32::default();
        let mut bytes = [0; STATE_SIZE];
        encode_state(&state, &mut bytes).unwrap();
        let metadata = StateMetadata {
            ss_debug_shadow: true,
        };
        metadata.write(&mut bytes);
        assert_eq!(decode_state(&bytes).unwrap(), state);
        assert_eq!(decode_state_metadata(&bytes).unwrap(), metadata);
        bytes[4..6].copy_from_slice(&1_u16.to_le_bytes());
        assert_eq!(decode_state(&bytes), Err(AbiError::Reserved));
        bytes[4..6].copy_from_slice(&STATE_VERSION_2.to_le_bytes());
        write_u32(&mut bytes, STATE_METADATA_OFFSET, 2);
        assert_eq!(decode_state_metadata(&bytes), Err(AbiError::Reserved));
        write_u32(&mut bytes, STATE_METADATA_OFFSET, u32::MAX);
        assert_eq!(decode_state(&bytes), Err(AbiError::Reserved));
    }

    #[test]
    fn expanded_flags_require_the_version_two_state_contract() {
        let state = State32 {
            eflags: 0x244dd7,
            ..State32::default()
        };
        let mut bytes = [0; STATE_SIZE];
        assert_eq!(encode_state(&state, &mut bytes), Err(AbiError::Flags));
        encode_state_v2(&state, &mut bytes).unwrap();
        assert_eq!(decode_state(&bytes).unwrap(), state);
        bytes[4..6].copy_from_slice(&1_u16.to_le_bytes());
        assert_eq!(decode_state(&bytes), Err(AbiError::Flags));
    }

    #[test]
    fn version_two_still_rejects_reserved_and_privileged_state() {
        for flags in [0, 2 | 8, 2 | 0x2000, 2 | 0x8000, 2 | 0x20000] {
            let mut bytes = [0; STATE_SIZE];
            assert_eq!(
                encode_state_v2(
                    &State32 {
                        eflags: flags,
                        ..State32::default()
                    },
                    &mut bytes
                ),
                Err(AbiError::Flags)
            );
        }
    }

    #[test]
    fn default_state_has_a_valid_binary_roundtrip() {
        let state = State32::default();
        let mut bytes = [0; STATE_SIZE];
        encode_state(&state, &mut bytes).unwrap();
        assert_eq!(decode_state(&bytes).unwrap(), state);
    }

    #[test]
    fn resume_flag_requires_version_three_and_old_encoders_are_atomic() {
        let state = State32 {
            eflags: 0x254fd7,
            ..State32::default()
        };
        for encoder in [encode_state, encode_state_v2] {
            let mut untouched = [0xa5; STATE_SIZE];
            assert_eq!(encoder(&state, &mut untouched), Err(AbiError::Flags));
            assert_eq!(untouched, [0xa5; STATE_SIZE]);
        }
        let mut bytes = [0; STATE_SIZE];
        encode_state_v3(&state, &mut bytes).unwrap();
        assert_eq!(
            &bytes[..16],
            b"R3ST\x03\x00\x01\x00\x38\x00\x00\x00\x00\x00\x00\x00"
        );
        assert_eq!(&bytes[EFLAGS_OFFSET..], &[0xd7, 0x4f, 0x25, 0]);
        assert_eq!(decode_state(&bytes), Ok(state));
        for version in [1_u16, 2] {
            bytes[4..6].copy_from_slice(&version.to_le_bytes());
            assert_eq!(decode_state(&bytes), Err(AbiError::Flags));
        }
    }

    #[test]
    fn state_version_flag_admission_matches_independent_architectural_bit_lists() {
        let legacy = [0, 1, 2, 4, 6, 7, 10, 11];
        let user = [0, 1, 2, 4, 6, 7, 8, 9, 10, 11, 14, 18, 21];
        let resume = [0, 1, 2, 4, 6, 7, 8, 9, 10, 11, 14, 16, 18, 21];
        for (version, allowed) in [
            (1_u16, legacy.as_slice()),
            (2, user.as_slice()),
            (3, resume.as_slice()),
        ] {
            let mut bytes = [0; STATE_SIZE];
            bytes[..4].copy_from_slice(b"R3ST");
            bytes[4..6].copy_from_slice(&version.to_le_bytes());
            bytes[6..8].copy_from_slice(&1_u16.to_le_bytes());
            bytes[8..12].copy_from_slice(&56_u32.to_le_bytes());
            for bit in 0..32 {
                let flags = 2 | (1_u32 << bit);
                write_u32(&mut bytes, EFLAGS_OFFSET, flags);
                if allowed.contains(&bit) {
                    assert_eq!(
                        decode_state(&bytes).unwrap().eflags,
                        flags,
                        "version={version} bit={bit}"
                    );
                } else {
                    assert_eq!(
                        decode_state(&bytes),
                        Err(AbiError::Flags),
                        "version={version} bit={bit}"
                    );
                }
            }
            write_u32(&mut bytes, EFLAGS_OFFSET, 0);
            assert_eq!(decode_state(&bytes), Err(AbiError::Flags));
        }
    }

    #[test]
    fn version_three_metadata_and_current_publication_preserve_resume_contract() {
        let state = State32 {
            eflags: 0x10002,
            ..State32::default()
        };
        let mut bytes = [0; STATE_SIZE];
        encode_state_current(&state, &mut bytes).unwrap();
        let before = bytes;
        for shadow in [true, false] {
            StateMetadata {
                ss_debug_shadow: shadow,
            }
            .write(&mut bytes);
            assert_eq!(&bytes[4..6], &3_u16.to_le_bytes());
            assert_eq!(decode_state(&bytes), Ok(state));
            assert_eq!(
                decode_state_metadata(&bytes).unwrap().ss_debug_shadow,
                shadow
            );
            let mut expected = before;
            write_u32(&mut expected, STATE_METADATA_OFFSET, u32::from(shadow));
            assert_eq!(bytes, expected);
        }
        encode_state_current(&State32::default(), &mut bytes).unwrap();
        assert_eq!(&bytes[4..6], &3_u16.to_le_bytes());
        assert_eq!(decode_state(&bytes), Ok(State32::default()));
        for metadata in [2, u32::MAX] {
            let mut malformed = bytes;
            write_u32(&mut malformed, STATE_METADATA_OFFSET, metadata);
            assert_eq!(decode_state_metadata(&malformed), Err(AbiError::Reserved));
        }
        let mut malformed = bytes;
        malformed[6] = 2;
        assert!(decode_state(&malformed).is_err());
        let mut malformed = bytes;
        malformed[4] = 4;
        assert!(decode_state(&malformed).is_err());
    }
}
