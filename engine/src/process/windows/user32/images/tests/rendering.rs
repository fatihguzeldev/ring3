use super::*;

#[test]
fn captured_drawiconex_preserves_native_and_xor_and_straight_alpha_pixel_results() {
    for (alpha, expected) in [
        (false, [16711935u32, 65535, 255, 16776960]),
        (true, [2151678079, 4278255360, 255, 4294967295]),
    ] {
        let mut engine = fixture(alpha);
        let icon = call(&mut engine, Api::LoadImageW, &[BASE, 6, 1, 0, 0, 0]);
        assert_ne!(icon, 0);
        let header = [40u32, 2, (-2i32) as u32, 1 | 32 << 16, 0, 16, 0, 0, 0, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(DATA + 0x100), &header)
            .unwrap();
        let bitmap = gdi(
            &mut engine,
            GdiApi32::CreateDIBSection,
            &[0, DATA + 0x100, 0, DATA, 0, 0],
        );
        assert_ne!(bitmap, 0);
        let pointer = read_word(&engine, DATA);
        let dc = gdi(&mut engine, GdiApi32::CreateCompatibleDC, &[0]);
        let old = gdi(&mut engine, GdiApi32::SelectObject, &[dc, bitmap]);
        for (flags, pixels) in [
            (3, expected),
            (
                2,
                if alpha {
                    [0x80800000, 0xff00ff00, 0, 0xffffffff]
                } else {
                    [0xff0000, 0xff00, 0xff, 0xffffff]
                },
            ),
            (1, [0xffffff, 0xffffff, 0, 0xffffff]),
        ] {
            engine
                .memory
                .as_mut()
                .unwrap()
                .write(
                    GuestAddress(pointer),
                    &[0xffu32; 4]
                        .into_iter()
                        .flat_map(u32::to_le_bytes)
                        .collect::<Vec<_>>(),
                )
                .unwrap();
            assert_eq!(
                call(
                    &mut engine,
                    Api::DrawIconEx,
                    &[dc, 0, 0, icon, 2, 2, 0, 0, flags]
                ),
                1
            );
            assert_eq!(
                [
                    read_word(&engine, pointer),
                    read_word(&engine, pointer + 4),
                    read_word(&engine, pointer + 8),
                    read_word(&engine, pointer + 12)
                ],
                pixels
            );
        }
        assert_eq!(gdi(&mut engine, GdiApi32::SelectObject, &[dc, old]), bitmap);
        assert_eq!(gdi(&mut engine, GdiApi32::DeleteObject, &[bitmap]), 1);
        assert_eq!(gdi(&mut engine, GdiApi32::DeleteDC, &[dc]), 1);
        assert_eq!(call(&mut engine, Api::DestroyIcon, &[icon]), 1);
    }
}
