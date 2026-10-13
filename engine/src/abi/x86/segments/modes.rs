use super::*;

#[test]
fn zero_base_d16_requires_current_companion_and_never_legacy_v2() {
    let mut context = SegmentContext::default();
    context.caches[1] = SegmentCache::from_bits(0xf1bf_ffff).unwrap();
    assert!(context.is_valid());
    let mut segments = [0xa5; SEGMENTS_SIZE];
    let mut bases = [0x5a; SEGMENT_BASES_RECORD_SIZE];
    encode_current_segment_context(&context, &mut segments, &mut bases).unwrap();
    assert_eq!(u16::from_le_bytes(segments[4..6].try_into().unwrap()), 3);
    assert_eq!(&bases[..4], b"R3SB");
    assert!(bases[16..].iter().all(|byte| *byte == 0));
    assert_eq!(
        decode_current_segment_context(&segments, Some(&bases)),
        Ok(context)
    );
    assert_eq!(
        decode_current_segment_context(&segments, None),
        Err(AbiError::Length)
    );
    let mut legacy = [0xa5; SEGMENTS_SIZE];
    assert_eq!(
        encode_segment_context(&context, &mut legacy),
        Err(AbiError::Reserved)
    );
    assert_eq!(legacy, [0xa5; SEGMENTS_SIZE]);
    segments[4..6].copy_from_slice(&2_u16.to_le_bytes());
    assert_eq!(decode_segment_context(&segments), Err(AbiError::Reserved));
    assert_eq!(
        decode_current_segment_context(&segments, Some(&bases)),
        Err(AbiError::Reserved)
    );
    segments[4..6].copy_from_slice(&3_u16.to_le_bytes());
    bases[12] = 1;
    assert_eq!(
        decode_current_segment_context(&segments, Some(&bases)),
        Err(AbiError::Reserved)
    );
}
