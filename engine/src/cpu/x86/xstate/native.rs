use super::{IMAGE_SIZE, read_control, requested, restore, save, validate_header};
use crate::cpu::x86::{X87State, simd::SimdState};

#[test]
fn legacy_helper_mask_and_header_stay_independent_of_enabled_ymm_state() {
    assert_eq!(read_control(0), Ok(7));
    assert_eq!(IMAGE_SIZE, 576);
    assert_eq!(requested(u32::MAX, u32::MAX), 3);
    assert_eq!(requested(4, 0), 0);
    for present in [4u64, 7, 1 << 63] {
        let mut header = [0; 24];
        header[..8].copy_from_slice(&present.to_le_bytes());
        assert!(validate_header(&header).is_err());
    }
    let mut header = [0; 24];
    header[..8].copy_from_slice(&3u64.to_le_bytes());
    assert_eq!(validate_header(&header), Ok(3));
}

#[test]
fn legacy_save_restore_ignore_upper_request_and_preserve_every_unrequested_byte() {
    let x87 = X87State {
        control: 0x027e,
        ..X87State::default()
    };
    let simd = SimdState {
        mxcsr: 0x5f80,
        xmm: [[0x71; 16]; 8],
    };
    let mut image = [0xa5; IMAGE_SIZE];
    image[512..536].fill(0);
    let before = image;
    save(4, &x87, &simd, &mut image);
    assert_eq!(image, before);
    assert_eq!(restore(4, &x87, &simd, &image), Ok((x87, simd)));
    let mut full = before;
    save(7, &x87, &simd, &mut full);
    assert_eq!(&full[512..520], &3u64.to_le_bytes());
    full[512..520].copy_from_slice(&4u64.to_le_bytes());
    assert!(restore(0, &x87, &simd, &full).is_err());
}
