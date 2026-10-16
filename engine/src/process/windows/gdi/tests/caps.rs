use super::{call, fixture};
use crate::windows::gdi::GdiApi32;

#[test]
fn device_capability_indices_describe_actual_raster_rectangle_clip_and_pixel_geometry() {
    let (mut runtime, mut memory) = fixture();
    let dc = call(
        &mut runtime,
        &mut memory,
        GdiApi32::CreateCompatibleDC,
        &[0],
    )
    .unwrap();
    for (index, expected) in [
        (32, 2 | 16 | 128),
        (36, 1),
        (38, 1 | 8 | 128 | 512 | 2048 | 8192),
        (40, 36),
        (42, 36),
        (44, 51),
        (104, 0),
        (106, 0),
    ] {
        assert_eq!(
            call(
                &mut runtime,
                &mut memory,
                GdiApi32::GetDeviceCaps,
                &[dc, index]
            ),
            Ok(expected),
            "GetDeviceCaps index {index}"
        );
    }
    let text = call(
        &mut runtime,
        &mut memory,
        GdiApi32::GetDeviceCaps,
        &[dc, 34],
    )
    .unwrap();
    assert_eq!(text & (8 | 16), 0, "rotated text remains unsupported");
    assert_ne!(text & 2048, 0, "underline has a real font raster path");
}
