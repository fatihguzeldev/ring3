use super::*;

#[test]
fn exact_address_record_rejects_every_header_and_selector_mutation() {
    let mut bytes = [0; AVX_ADDRESS_SIZE];
    header::write_version(&mut bytes, AVX_ADDRESS_MAGIC, AVX_ADDRESS_VERSION);
    header::write_u32(&mut bytes, AVX_ADDRESS_LOGICAL_OFFSET, u32::MAX);
    for (selector, segment) in [
        SegmentRegister::Es,
        SegmentRegister::Cs,
        SegmentRegister::Ss,
        SegmentRegister::Ds,
        SegmentRegister::Fs,
        SegmentRegister::Gs,
    ]
    .into_iter()
    .enumerate()
    {
        header::write_u32(&mut bytes, AVX_ADDRESS_SEGMENT_OFFSET, selector as u32);
        assert_eq!(
            decode_avx_address(&bytes),
            Ok(AvxAddress {
                segment,
                offset: u32::MAX
            })
        );
    }
    for offset in [0, 4, 6, 8, 12] {
        let mut invalid = bytes;
        invalid[offset] ^= 0x80;
        assert!(
            decode_avx_address(&invalid).is_err(),
            "header byte {offset}"
        );
    }
    for segment in [6, 7, u32::MAX] {
        header::write_u32(&mut bytes, AVX_ADDRESS_SEGMENT_OFFSET, segment);
        assert!(decode_avx_address(&bytes).is_err());
    }
    assert!(decode_avx_address(&bytes[..23]).is_err());
}
