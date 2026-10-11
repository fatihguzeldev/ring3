use crate::abi::AbiError;
use crate::abi::header::{self, read_u32, write_u32};
use crate::cpu::x86::SegmentState;

pub const SEGMENTS_SIZE: usize = 64;
pub const SEGMENTS_SELECTORS_OFFSET: usize = 16;
pub const SEGMENTS_BASES_OFFSET: usize = 32;

pub fn encode_segments(state: &SegmentState, output: &mut [u8]) -> Result<(), AbiError> {
    if output.len() != SEGMENTS_SIZE {
        return Err(AbiError::Length);
    }
    if !state.is_valid() {
        return Err(AbiError::Reserved);
    }
    header::write(output, *b"R3SG");
    for index in 0..6 {
        let selector = SEGMENTS_SELECTORS_OFFSET + index * 2;
        output[selector..selector + 2].copy_from_slice(&state.selectors[index].to_le_bytes());
        write_u32(
            output,
            SEGMENTS_BASES_OFFSET + index * 4,
            state.bases[index],
        );
    }
    Ok(())
}

pub fn decode_segments(input: &[u8]) -> Result<SegmentState, AbiError> {
    header::validate(input, *b"R3SG", SEGMENTS_SIZE)?;
    if input[28..32]
        .iter()
        .chain(&input[56..64])
        .any(|byte| *byte != 0)
    {
        return Err(AbiError::Reserved);
    }
    let state = SegmentState {
        selectors: std::array::from_fn(|index| {
            let selector = SEGMENTS_SELECTORS_OFFSET + index * 2;
            u16::from_le_bytes(input[selector..selector + 2].try_into().unwrap())
        }),
        bases: std::array::from_fn(|index| read_u32(input, SEGMENTS_BASES_OFFSET + index * 4)),
    };
    if !state.is_valid() {
        return Err(AbiError::Reserved);
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::x86::SegmentRegister;

    #[test]
    fn segment_record_roundtrips_nonzero_fs_and_gs_bases() {
        let mut state = SegmentState::default();
        assert!(state.set_base(SegmentRegister::Fs, 0x7fff_0000));
        assert!(state.set_base(SegmentRegister::Gs, 0xffff_fff0));
        let mut record = [0; SEGMENTS_SIZE];
        encode_segments(&state, &mut record).unwrap();
        assert_eq!(decode_segments(&record), Ok(state));
        assert!(!state.set_base(SegmentRegister::Ds, 1));
    }

    #[test]
    fn segment_record_rejects_reserved_bytes_and_nonflat_legacy_segments() {
        let mut record = [0; SEGMENTS_SIZE];
        encode_segments(&SegmentState::default(), &mut record).unwrap();
        for index in [28, 31, 56, 63] {
            let mut malformed = record;
            malformed[index] = 1;
            assert_eq!(decode_segments(&malformed), Err(AbiError::Reserved));
        }
        for index in 0..4 {
            let mut malformed = record;
            malformed[SEGMENTS_BASES_OFFSET + index * 4] = 1;
            assert_eq!(decode_segments(&malformed), Err(AbiError::Reserved));
        }
    }

    #[test]
    fn segment_record_requires_live_ring3_code_and_stack_but_allows_null_data_selectors() {
        let valid = SegmentState::default();
        let mut record = [0; SEGMENTS_SIZE];
        encode_segments(&valid, &mut record).unwrap();
        for segment in [SegmentRegister::Cs, SegmentRegister::Ss] {
            for selector in [0_u16, 1, 2, 3, 0x20, 0x21, 0x22] {
                let mut malformed = record;
                let offset = SEGMENTS_SELECTORS_OFFSET + segment.index() * 2;
                malformed[offset..offset + 2].copy_from_slice(&selector.to_le_bytes());
                assert_eq!(decode_segments(&malformed), Err(AbiError::Reserved));
                let mut state = valid;
                state.selectors[segment.index()] = selector;
                assert!(!state.is_valid());
                let mut untouched = [0xa5; SEGMENTS_SIZE];
                assert_eq!(
                    encode_segments(&state, &mut untouched),
                    Err(AbiError::Reserved)
                );
                assert_eq!(untouched, [0xa5; SEGMENTS_SIZE]);
            }
        }
        let mut null_data = valid;
        for segment in [
            SegmentRegister::Es,
            SegmentRegister::Ds,
            SegmentRegister::Fs,
            SegmentRegister::Gs,
        ] {
            null_data.selectors[segment.index()] = 0;
        }
        encode_segments(&null_data, &mut record).unwrap();
        assert_eq!(decode_segments(&record), Ok(null_data));
    }
}
