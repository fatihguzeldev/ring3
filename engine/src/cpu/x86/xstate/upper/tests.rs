use super::*;

#[test]
fn every_request_and_presence_mask_preserves_unselected_bits_and_ia32_reserved_tail() {
    let x87 = X87State {
        control: 0x077f,
        ..X87State::default()
    };
    let simd = SimdState {
        mxcsr: 0x5f80,
        xmm: [[0x37; 16]; 8],
    };
    let current_simd = SimdState {
        mxcsr: 0xdfe5,
        xmm: [[0x71; 16]; 8],
    };
    let upper = YmmUpperState {
        registers: std::array::from_fn(|i| std::array::from_fn(|j| (i * 29 + j * 73) as u8)),
    };
    let current_upper = YmmUpperState {
        registers: [[0x9b; 16]; 8],
    };
    let mut current_x87 = x87;
    current_x87.control = 0x027f;
    for request in 0_u64..8 {
        let mut image = [0x9b; IMAGE_SIZE];
        image[512..520].copy_from_slice(&0x8000_0000_0000_0040_u64.to_le_bytes());
        save(request, &x87, &simd, &upper, &mut image);
        for (index, byte) in image.iter().enumerate() {
            let selected = request & 1 != 0 && (index < 24 || (32..160).contains(&index))
                || request & 6 != 0 && (24..32).contains(&index)
                || request & 2 != 0 && (160..288).contains(&index)
                || request & 4 != 0 && (576..704).contains(&index)
                || (512..520).contains(&index);
            if !selected {
                assert_eq!(*byte, 0x9b, "request{request} offset{index}");
            }
        }
        assert_eq!(
            u64::from_le_bytes(image[512..520].try_into().unwrap()),
            0x8000_0000_0000_0040 | request
        );
        let (ranges, count) = access::save_ranges(request);
        assert!(count <= 4);
        for index in 0..IMAGE_SIZE {
            let expected = request & 1 != 0 && (index < 24 || (32..160).contains(&index))
                || request & 6 != 0 && (24..32).contains(&index)
                || request & 2 != 0 && (160..288).contains(&index)
                || request & 4 != 0 && (576..704).contains(&index)
                || (512..520).contains(&index);
            assert_eq!(
                ranges[..count]
                    .iter()
                    .any(|(a, b)| (*a..*b).contains(&index)),
                expected
            );
        }
        let mut source = [0x9b; IMAGE_SIZE];
        save(7, &x87, &simd, &upper, &mut source);
        source[520..536].fill(0);
        for present in 0_u64..8 {
            source[512..520].copy_from_slice(&present.to_le_bytes());
            let (after_x87, after_simd, after_upper) = restore(
                request,
                &current_x87,
                &current_simd,
                &current_upper,
                &source,
            )
            .unwrap();
            assert_eq!(
                after_x87,
                if request & 1 == 0 {
                    current_x87
                } else if present & 1 == 0 {
                    X87State::default()
                } else {
                    x87
                }
            );
            assert_eq!(
                after_simd.mxcsr,
                if request & 6 == 0 {
                    current_simd.mxcsr
                } else {
                    simd.mxcsr
                }
            );
            assert_eq!(
                after_simd.xmm,
                if request & 2 == 0 {
                    current_simd.xmm
                } else if present & 2 == 0 {
                    [[0; 16]; 8]
                } else {
                    simd.xmm
                }
            );
            assert_eq!(
                after_upper,
                if request & 4 == 0 {
                    current_upper
                } else if present & 4 == 0 {
                    YmmUpperState::default()
                } else {
                    upper
                }
            );
        }
    }
}

#[test]
fn header_is_unconditional_but_mxcsr_validation_follows_either_vector_request() {
    let x87 = X87State::default();
    let simd = SimdState::default();
    let upper = YmmUpperState::default();
    for request in 0..8 {
        for offset in [512, 519, 520, 527, 535] {
            let mut image = [0; IMAGE_SIZE];
            image[24..28].copy_from_slice(&0x1f80_u32.to_le_bytes());
            image[offset] = if offset == 512 { 8 } else { 1 };
            assert_eq!(
                restore(request, &x87, &simd, &upper, &image),
                Err(XStateError)
            );
        }
        let mut image = [0; IMAGE_SIZE];
        image[24..28].copy_from_slice(&0x10000_u32.to_le_bytes());
        assert_eq!(
            restore(request, &x87, &simd, &upper, &image).is_err(),
            request & 6 != 0
        );
    }
    assert_eq!(requested(u32::MAX, u32::MAX), 7);
    assert_eq!(super::super::requested(u32::MAX, u32::MAX), 3);
    assert_eq!(super::super::read_control(0), Ok(3));
    let mut header = [0; 24];
    header[..8].copy_from_slice(&4_u64.to_le_bytes());
    assert_eq!(validate_header(&header), Ok(4));
    assert!(super::super::validate_header(&header).is_err());
}
