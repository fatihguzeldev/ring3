use crate::abi::AbiError;
use crate::abi::header::{self, read_u32, write_u32};
use crate::cpu::x86::{SegmentCache, SegmentContext, SegmentState};

pub const SEGMENTS_SIZE: usize = 64;
pub const SEGMENTS_SELECTORS_OFFSET: usize = 16;
pub const SEGMENTS_BASES_OFFSET: usize = 32;
pub const SEGMENTS_VERSION_2: u16 = 2;
pub(crate) const SEGMENTS_CACHE_OFFSETS: [usize; 6] = [28, 32, 36, 40, 44, 56];

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

/// encode the current context, retaining byte-identical v1 for legacy flat caches.
pub fn encode_segment_context(context: &SegmentContext, output: &mut [u8]) -> Result<(), AbiError> {
    if output.len() != SEGMENTS_SIZE {
        return Err(AbiError::Length);
    }
    if !context.is_valid() {
        return Err(AbiError::Reserved);
    }
    if context.is_legacy() {
        return encode_segments(&context.state, output);
    }
    header::write_version(output, *b"R3SG", SEGMENTS_VERSION_2);
    for (index, cache_offset) in SEGMENTS_CACHE_OFFSETS.into_iter().enumerate() {
        let selector = SEGMENTS_SELECTORS_OFFSET + index * 2;
        output[selector..selector + 2]
            .copy_from_slice(&context.state.selectors[index].to_le_bytes());
        write_u32(output, cache_offset, context.caches[index].bits());
    }
    for index in 4..6 {
        write_u32(
            output,
            SEGMENTS_BASES_OFFSET + index * 4,
            context.state.bases[index],
        );
    }
    Ok(())
}

pub fn decode_segment_context(input: &[u8]) -> Result<SegmentContext, AbiError> {
    let version =
        header::validate_versions(input, *b"R3SG", SEGMENTS_SIZE, &[1, SEGMENTS_VERSION_2])?;
    if version == 1 {
        return decode_segments(input).map(SegmentContext::legacy);
    }
    if read_u32(input, 60) != 0 {
        return Err(AbiError::Reserved);
    }
    let mut context = SegmentContext {
        state: SegmentState {
            selectors: std::array::from_fn(|index| {
                let offset = SEGMENTS_SELECTORS_OFFSET + index * 2;
                u16::from_le_bytes(input[offset..offset + 2].try_into().unwrap())
            }),
            bases: [0; 6],
        },
        caches: [SegmentCache::UNUSABLE; 6],
    };
    for (index, offset) in SEGMENTS_CACHE_OFFSETS.into_iter().enumerate() {
        context.caches[index] =
            SegmentCache::from_bits(read_u32(input, offset)).ok_or(AbiError::Reserved)?;
    }
    for index in 4..6 {
        context.state.bases[index] = read_u32(input, SEGMENTS_BASES_OFFSET + index * 4);
    }
    if !context.is_valid() {
        return Err(AbiError::Reserved);
    }
    Ok(context)
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

    #[test]
    fn current_context_keeps_legacy_bytes_and_strict_v1_decoder() {
        let mut state = SegmentState::default();
        state.bases[4] = 0x7fff_0000;
        let mut legacy = [0; SEGMENTS_SIZE];
        let mut current = [0; SEGMENTS_SIZE];
        encode_segments(&state, &mut legacy).unwrap();
        let context = SegmentContext::legacy(state);
        encode_segment_context(&context, &mut current).unwrap();
        assert_eq!(current, legacy);
        assert_eq!(decode_segment_context(&current), Ok(context));
        let mut cached = context;
        cached.caches[4] = SegmentCache::from_bits(0xf310_1234).unwrap();
        encode_segment_context(&cached, &mut current).unwrap();
        assert_eq!(decode_segments(&current), Err(AbiError::Version));
        assert_eq!(decode_segment_context(&current), Ok(cached));
        assert_eq!(&current[48..56], &legacy[48..56]);
    }

    #[test]
    fn context_refuses_malformed_caches_and_encoding_is_atomic() {
        let mut context = SegmentContext::default();
        context.caches[4] = SegmentCache::from_bits(0xf310_1234).unwrap();
        let mut valid = [0; SEGMENTS_SIZE];
        encode_segment_context(&context, &mut valid).unwrap();
        for (offset, bits) in [
            (28, 1),
            (32, 0xf1bf_ffff),
            (36, 0xf13f_ffff),
            (40, 0xf39f_ffff),
            (44, 0x933f_ffff),
            (56, 0xf33f_ffff),
            (60, 1),
        ] {
            let mut malformed = valid;
            write_u32(&mut malformed, offset, bits);
            assert_eq!(
                decode_segment_context(&malformed),
                Err(AbiError::Reserved),
                "offset={offset}"
            );
        }
        context.caches[1] = SegmentCache::from_bits(0xf33f_ffff).unwrap();
        let mut untouched = [0xa5; SEGMENTS_SIZE];
        assert_eq!(
            encode_segment_context(&context, &mut untouched),
            Err(AbiError::Reserved)
        );
        assert_eq!(untouched, [0xa5; SEGMENTS_SIZE]);
    }
}
