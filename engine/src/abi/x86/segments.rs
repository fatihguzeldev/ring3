use crate::abi::AbiError;
use crate::abi::header::{self, read_u32, write_u32};
use crate::cpu::x86::{SegmentCache, SegmentContext, SegmentState};

pub const SEGMENTS_SIZE: usize = 64;
pub const SEGMENTS_SELECTORS_OFFSET: usize = 16;
pub const SEGMENTS_BASES_OFFSET: usize = 32;
pub const SEGMENTS_VERSION_2: u16 = 2;
pub const SEGMENTS_VERSION_3: u16 = 3;
pub const SEGMENT_BASES_RECORD_SIZE: usize = 40;
pub(crate) const SEGMENTS_CACHE_OFFSETS: [usize; 6] = [28, 32, 36, 40, 44, 56];

pub fn encode_segments(state: &SegmentState, output: &mut [u8]) -> Result<(), AbiError> {
    if output.len() != SEGMENTS_SIZE {
        return Err(AbiError::Length);
    }
    if !state.is_valid() || state.bases[..4].iter().any(|base| *base != 0) {
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
    if !state.is_valid() || state.bases[..4].iter().any(|base| *base != 0) {
        return Err(AbiError::Reserved);
    }
    Ok(state)
}

/// encode the legacy flat-base profile, retaining byte-identical v1 for legacy caches.
pub fn encode_segment_context(context: &SegmentContext, output: &mut [u8]) -> Result<(), AbiError> {
    if output.len() != SEGMENTS_SIZE {
        return Err(AbiError::Length);
    }
    if !context.is_valid() || context.state.bases[..4].iter().any(|base| *base != 0) {
        return Err(AbiError::Reserved);
    }
    if context.is_legacy() {
        return encode_segments(&context.state, output);
    }
    encode_cached_context(context, SEGMENTS_VERSION_2, output);
    Ok(())
}

fn encode_cached_context(context: &SegmentContext, version: u16, output: &mut [u8]) {
    header::write_version(output, *b"R3SG", version);
    for (index, cache_offset) in SEGMENTS_CACHE_OFFSETS.into_iter().enumerate() {
        let selector = SEGMENTS_SELECTORS_OFFSET + index * 2;
        output[selector..selector + 2]
            .copy_from_slice(&context.state.selectors[index].to_le_bytes());
        write_u32(output, cache_offset, context.caches[index].bits());
    }
    if version == SEGMENTS_VERSION_2 {
        for index in 4..6 {
            write_u32(
                output,
                SEGMENTS_BASES_OFFSET + index * 4,
                context.state.bases[index],
            );
        }
    }
}

pub fn decode_segment_context(input: &[u8]) -> Result<SegmentContext, AbiError> {
    let version =
        header::validate_versions(input, *b"R3SG", SEGMENTS_SIZE, &[1, SEGMENTS_VERSION_2])?;
    decode_legacy_context(input, version)
}

fn decode_legacy_context(input: &[u8], version: u16) -> Result<SegmentContext, AbiError> {
    if version == 1 {
        return decode_segments(input).map(SegmentContext::legacy);
    }
    if read_u32(input, 60) != 0 {
        return Err(AbiError::Reserved);
    }
    let mut bases = [0; 6];
    for (index, base) in bases.iter_mut().enumerate().skip(4) {
        *base = read_u32(input, SEGMENTS_BASES_OFFSET + index * 4);
    }
    decode_cached_context(input, bases)
}

fn decode_cached_context(input: &[u8], bases: [u32; 6]) -> Result<SegmentContext, AbiError> {
    let mut context = SegmentContext {
        state: SegmentState {
            selectors: std::array::from_fn(|index| {
                let offset = SEGMENTS_SELECTORS_OFFSET + index * 2;
                u16::from_le_bytes(input[offset..offset + 2].try_into().unwrap())
            }),
            bases,
        },
        caches: [SegmentCache::UNUSABLE; 6],
    };
    for (index, offset) in SEGMENTS_CACHE_OFFSETS.into_iter().enumerate() {
        context.caches[index] =
            SegmentCache::from_bits(read_u32(input, offset)).ok_or(AbiError::Reserved)?;
    }
    if !context.is_valid() {
        return Err(AbiError::Reserved);
    }
    Ok(context)
}

/// stage both records before publication; legacy contexts keep their existing bytes.
pub fn encode_current_segment_context(
    context: &SegmentContext,
    segments: &mut [u8],
    bases: &mut [u8],
) -> Result<(), AbiError> {
    if segments.len() != SEGMENTS_SIZE || bases.len() != SEGMENT_BASES_RECORD_SIZE {
        return Err(AbiError::Length);
    }
    if !context.is_valid() {
        return Err(AbiError::Reserved);
    }
    let mut staged_segments = [0; SEGMENTS_SIZE];
    let mut staged_bases = [0; SEGMENT_BASES_RECORD_SIZE];
    header::write(&mut staged_bases, *b"R3SB");
    if context.state.bases[..4].iter().any(|base| *base != 0) {
        encode_cached_context(context, SEGMENTS_VERSION_3, &mut staged_segments);
        for (index, base) in context.state.bases.into_iter().enumerate() {
            write_u32(&mut staged_bases, 16 + index * 4, base);
        }
    } else {
        encode_segment_context(context, &mut staged_segments)?;
    }
    segments.copy_from_slice(&staged_segments);
    bases.copy_from_slice(&staged_bases);
    Ok(())
}

pub fn decode_current_segment_context(
    segments: &[u8],
    bases: Option<&[u8]>,
) -> Result<SegmentContext, AbiError> {
    let version = header::validate_versions(
        segments,
        *b"R3SG",
        SEGMENTS_SIZE,
        &[1, SEGMENTS_VERSION_2, SEGMENTS_VERSION_3],
    )?;
    if version != SEGMENTS_VERSION_3 {
        return decode_legacy_context(segments, version);
    }
    if [48, 52, 60]
        .into_iter()
        .any(|offset| read_u32(segments, offset) != 0)
    {
        return Err(AbiError::Reserved);
    }
    let bases = bases.ok_or(AbiError::Length)?;
    header::validate(bases, *b"R3SB", SEGMENT_BASES_RECORD_SIZE)?;
    decode_cached_context(
        segments,
        std::array::from_fn(|index| read_u32(bases, 16 + index * 4)),
    )
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
        let mut nonflat_ss = state;
        assert!(nonflat_ss.set_base(SegmentRegister::Ss, 1));
        assert!(encode_segments(&nonflat_ss, &mut record).is_err());
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
    #[test]
    fn current_nonflat_context_roundtrips_all_bases_without_reusing_cache_words() {
        let mut context = SegmentContext::default();
        context.state.bases = [
            0x1234_0000,
            0,
            0x5000,
            0xffff_fff0,
            0x7654_0000,
            0x8765_0000,
        ];
        context.caches[3] = SegmentCache::from_bits(0xf310_1234).unwrap();
        let mut segments = [0; SEGMENTS_SIZE];
        let mut bases = [0; SEGMENT_BASES_RECORD_SIZE];
        encode_current_segment_context(&context, &mut segments, &mut bases).unwrap();
        assert_eq!(&segments[4..6], &3_u16.to_le_bytes());
        assert_eq!(&bases[..4], b"R3SB");
        assert_eq!(read_u32(&bases, 8), 40);
        for (index, offset) in SEGMENTS_CACHE_OFFSETS.into_iter().enumerate() {
            assert_eq!(read_u32(&segments, offset), context.caches[index].bits());
            assert_eq!(read_u32(&bases, 16 + index * 4), context.state.bases[index]);
        }
        for offset in [48, 52, 60] {
            assert_eq!(read_u32(&segments, offset), 0);
        }
        assert_eq!(
            decode_current_segment_context(&segments, Some(&bases)),
            Ok(context)
        );
        assert_eq!(decode_segment_context(&segments), Err(AbiError::Version));
        let mut untouched = [0xa5; SEGMENTS_SIZE];
        assert_eq!(
            encode_segment_context(&context, &mut untouched),
            Err(AbiError::Reserved)
        );
        assert_eq!(untouched, [0xa5; SEGMENTS_SIZE]);
        assert_eq!(
            encode_segments(&context.state, &mut untouched),
            Err(AbiError::Reserved)
        );
        assert_eq!(untouched, [0xa5; SEGMENTS_SIZE]);
    }

    #[test]
    fn legacy_current_context_ignores_companion_and_writes_canonical_zero_bases() {
        let mut context = SegmentContext::default();
        context.state.bases[4] = 0x1234_0000;
        for cache in [
            context.caches[4],
            SegmentCache::from_bits(0xf310_1234).unwrap(),
        ] {
            context.caches[4] = cache;
            let mut legacy = [0; SEGMENTS_SIZE];
            encode_segment_context(&context, &mut legacy).unwrap();
            let mut segments = [0; SEGMENTS_SIZE];
            let mut bases = [0xa5; SEGMENT_BASES_RECORD_SIZE];
            encode_current_segment_context(&context, &mut segments, &mut bases).unwrap();
            assert_eq!(segments, legacy);
            assert_eq!(&bases[..4], b"R3SB");
            assert!(bases[16..].iter().all(|byte| *byte == 0));
            for ignored in [
                None,
                Some(&[][..]),
                Some(&[0xa5; SEGMENT_BASES_RECORD_SIZE][..]),
            ] {
                assert_eq!(
                    decode_current_segment_context(&segments, ignored),
                    Ok(context)
                );
            }
        }
    }

    #[test]
    fn current_codec_refuses_malformed_companions_and_stages_both_outputs() {
        let mut context = SegmentContext::default();
        context.state.bases[3] = 0x1000;
        let mut segments = [0; SEGMENTS_SIZE];
        let mut bases = [0; SEGMENT_BASES_RECORD_SIZE];
        encode_current_segment_context(&context, &mut segments, &mut bases).unwrap();
        assert_eq!(
            decode_current_segment_context(&segments, None),
            Err(AbiError::Length)
        );
        for (offset, expected) in [
            (0, AbiError::Magic),
            (4, AbiError::Version),
            (6, AbiError::Profile),
            (8, AbiError::Length),
            (12, AbiError::Reserved),
            (20, AbiError::Reserved),
        ] {
            let mut malformed = bases;
            malformed[offset] ^= 0x80;
            assert_eq!(
                decode_current_segment_context(&segments, Some(&malformed)),
                Err(expected),
                "offset={offset}"
            );
        }
        for offset in [48, 52, 60] {
            let mut malformed = segments;
            malformed[offset] = 1;
            assert_eq!(
                decode_current_segment_context(&malformed, Some(&bases)),
                Err(AbiError::Reserved)
            );
        }
        for invalid_length in [SEGMENT_BASES_RECORD_SIZE - 1, SEGMENT_BASES_RECORD_SIZE + 1] {
            let mut untouched_segments = [0xa5; SEGMENTS_SIZE];
            let mut untouched_bases = vec![0x5a; invalid_length];
            assert_eq!(
                encode_current_segment_context(
                    &context,
                    &mut untouched_segments,
                    &mut untouched_bases
                ),
                Err(AbiError::Length)
            );
            assert_eq!(untouched_segments, [0xa5; SEGMENTS_SIZE]);
            assert_eq!(untouched_bases, vec![0x5a; invalid_length]);
        }
        context.caches[2] =
            SegmentCache::from_bits(context.caches[2].bits() & !0x0200_0000).unwrap();
        let mut untouched_segments = [0xa5; SEGMENTS_SIZE];
        let mut untouched_bases = [0x5a; SEGMENT_BASES_RECORD_SIZE];
        assert_eq!(
            encode_current_segment_context(&context, &mut untouched_segments, &mut untouched_bases),
            Err(AbiError::Reserved)
        );
        assert_eq!(untouched_segments, [0xa5; SEGMENTS_SIZE]);
        assert_eq!(untouched_bases, [0x5a; SEGMENT_BASES_RECORD_SIZE]);
    }
}
