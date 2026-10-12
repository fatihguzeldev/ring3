use crate::abi::AbiError;
use crate::abi::header::{self, read_u32, write_u32};
use crate::cpu::x86::State32;

pub const STATE_SIZE: usize = 56;
pub const REGISTERS_OFFSET: usize = 16;
pub const EIP_OFFSET: usize = 48;
pub const EFLAGS_OFFSET: usize = 52;
pub const STATE_VERSION_2: u16 = 2;
pub const LEGACY_FLAGS_MASK: u32 = 0xcd7;
pub const USER_FLAGS_MASK: u32 = 0x244fd7;
pub const STATE_METADATA_OFFSET: usize = 12;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct StateMetadata {
    pub(crate) ss_debug_shadow: bool,
}

impl StateMetadata {
    pub(crate) fn write(self, state: &mut [u8; STATE_SIZE]) {
        if self.ss_debug_shadow {
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

pub(crate) fn encode_state_current(state: &State32, output: &mut [u8]) -> Result<(), AbiError> {
    if state.eflags & !LEGACY_FLAGS_MASK == 0 {
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
    let (version, _) = validate_header(input)?;
    let eflags = read_u32(input, EFLAGS_OFFSET);
    validate_flags(eflags, version)?;
    Ok(State32 {
        registers: std::array::from_fn(|index| read_u32(input, REGISTERS_OFFSET + index * 4)),
        eip: read_u32(input, EIP_OFFSET),
        eflags,
    })
}

pub(crate) fn decode_state_metadata(input: &[u8]) -> Result<StateMetadata, AbiError> {
    let (version, metadata) = validate_header(input)?;
    validate_flags(read_u32(input, EFLAGS_OFFSET), version)?;
    Ok(StateMetadata {
        ss_debug_shadow: metadata == 1,
    })
}

fn validate_header(input: &[u8]) -> Result<(u16, u32), AbiError> {
    let (version, metadata) = header::validate_versions_with_metadata(
        input,
        *b"R3ST",
        STATE_SIZE,
        &[crate::abi::ABI_VERSION, STATE_VERSION_2],
    )?;
    if metadata > u32::from(version == STATE_VERSION_2) {
        return Err(AbiError::Reserved);
    }
    Ok((version, metadata))
}

fn validate_flags(flags: u32, version: u16) -> Result<(), AbiError> {
    let mask = if version == STATE_VERSION_2 {
        USER_FLAGS_MASK
    } else {
        LEGACY_FLAGS_MASK
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
}
