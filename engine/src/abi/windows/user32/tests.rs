use super::*;

fn info() -> User32WindowInfo32 {
    User32WindowInfo32 {
        window: 0x12345678,
        parent: 0,
        owner: 0x23456789,
        thread: 7,
        style: 0x9000_0000,
        ex_style: 0x200,
        outer: Rect32 {
            left: -220,
            top: -120,
            right: 420,
            bottom: 360,
        },
        client_origin: Point32 { x: -212, y: -92 },
        client_extent: WindowExtent32 {
            width: 624,
            height: 444,
        },
        visible: true,
        enabled: false,
    }
}

#[test]
fn literal_layout_round_trips_signed_geometry_and_split_relationships() {
    let info = info();
    let mut bytes = [0; USER32_WINDOW_INFO_SIZE];
    encode_user32_window_info(Some(info), &mut bytes).unwrap();
    assert_eq!(&bytes[..4], b"R3UW");
    assert_eq!(&bytes[4..12], &[1, 0, 1, 0, 80, 0, 0, 0]);
    for (offset, expected) in [
        (12, 1),
        (16, info.window),
        (20, 0),
        (24, info.owner),
        (28, 7),
        (32, 0x9000_0000),
        (36, 0x200),
        (40, -220i32 as u32),
        (44, -120i32 as u32),
        (48, 420),
        (52, 360),
        (56, -212i32 as u32),
        (60, -92i32 as u32),
        (64, 624),
        (68, 444),
        (72, 1),
        (76, 0),
    ] {
        assert_eq!(
            header::read_u32(&bytes, offset),
            expected,
            "offset {offset}"
        );
    }
    assert_eq!(decode_user32_window_info(&bytes).unwrap(), Some(info));
    let child = User32WindowInfo32 {
        style: 0x5000_0000,
        parent: info.owner,
        owner: 0,
        ..info
    };
    encode_user32_window_info(Some(child), &mut bytes).unwrap();
    assert_eq!(decode_user32_window_info(&bytes).unwrap(), Some(child));
}

#[test]
fn absent_payload_is_zero_and_malformed_presence_booleans_and_relationships_are_rejected() {
    let mut bytes = [0xa5; USER32_WINDOW_INFO_SIZE];
    encode_user32_window_info(None, &mut bytes).unwrap();
    assert!(bytes[12..].iter().all(|byte| *byte == 0));
    assert_eq!(decode_user32_window_info(&bytes).unwrap(), None);
    bytes[16] = 1;
    assert_eq!(decode_user32_window_info(&bytes), Err(AbiError::Reserved));
    for (offset, value) in [
        (12, 2),
        (16, 0),
        (20, 3),
        (28, 0),
        (64, u32::MAX),
        (72, 2),
        (76, 2),
    ] {
        encode_user32_window_info(Some(info()), &mut bytes).unwrap();
        header::write_u32(&mut bytes, offset, value);
        assert!(
            decode_user32_window_info(&bytes).is_err(),
            "offset {offset}"
        );
    }
    encode_user32_window_info(Some(info()), &mut bytes).unwrap();
    header::write_u32(&mut bytes, 32, 0x5000_0000);
    assert_eq!(decode_user32_window_info(&bytes), Err(AbiError::Reserved));
}

#[test]
fn invalid_encode_and_invalid_record_headers_leave_the_output_untouched() {
    let mut bytes = [0xa5; USER32_WINDOW_INFO_SIZE];
    let invalid = User32WindowInfo32 {
        window: 0,
        ..info()
    };
    assert_eq!(
        encode_user32_window_info(Some(invalid), &mut bytes),
        Err(AbiError::Reserved)
    );
    assert_eq!(bytes, [0xa5; USER32_WINDOW_INFO_SIZE]);
    assert_eq!(
        encode_user32_window_info(None, &mut bytes[..79]),
        Err(AbiError::Length)
    );
    assert_eq!(bytes, [0xa5; USER32_WINDOW_INFO_SIZE]);
    for offset in [0, 4, 6, 8] {
        encode_user32_window_info(Some(info()), &mut bytes).unwrap();
        bytes[offset] ^= 0x80;
        assert!(
            decode_user32_window_info(&bytes).is_err(),
            "offset {offset}"
        );
    }
}
