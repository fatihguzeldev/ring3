use super::*;

fn status(
    engine: &mut EngineInstance,
    parent_callback: u64,
    common: u64,
    procedure: u32,
    parent: u32,
) -> u32 {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12400),
            &"ready\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    capture(
        engine,
        CommonControlsApi32::CreateStatusWindowW.id(),
        &[0x50000000, 0x12400, parent, 91],
    );
    callbacks(engine, parent_callback, common, procedure);
    let window = eax(engine);
    assert_ne!(window, 0);
    window
}

#[test]
fn captured_status_window_uses_real_creation_placement_parts_and_ansi_wide_text() {
    let (mut engine, parent_callback, common, procedure, _, parent) = fixture();
    let window = status(&mut engine, parent_callback, common, procedure, parent);
    let handle = WindowHandle32::try_from(window).unwrap();
    let extent = engine
        .user32_windows()
        .unwrap()
        .get(handle)
        .unwrap()
        .description
        .extent;
    assert_eq!(extent.width, 8);
    assert!(extent.height > 4);
    capture(
        &mut engine,
        User32Api32::GetWindowRect.id(),
        &[window, 0x12580],
    );
    assert_eq!(eax(&engine), 1);
    let [left, top, right, bottom] = words::<4>(&engine, 0x12580);
    assert_eq!((left as i32, top as i32), (0, 6 - extent.height as i32));
    assert_eq!((right - left, bottom.wrapping_sub(top)), (8, extent.height));
    assert_eq!(
        send(
            &mut engine,
            parent_callback,
            common,
            procedure,
            window,
            [0x40c, 0, 0]
        ),
        5
    );
    assert_eq!(
        send(
            &mut engine,
            parent_callback,
            common,
            procedure,
            window,
            [0x40d, 0, 0x12500]
        ),
        5
    );
    assert_eq!(
        words::<3>(&engine, 0x12500),
        [0x00650072, 0x00640061, 0x00000079]
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12480),
            &[20i32, 40, -1]
                .into_iter()
                .flat_map(i32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(
        send(
            &mut engine,
            parent_callback,
            common,
            procedure,
            window,
            [0x405, 3, 0x12480]
        ),
        1
    );
    assert_eq!(
        send(
            &mut engine,
            parent_callback,
            common,
            procedure,
            window,
            [0x406, 8, 0x12520]
        ),
        3
    );
    assert_eq!(words::<3>(&engine, 0x12520), [20, 40, u32::MAX]);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x124a0), &[0xe9, 0])
        .unwrap();
    assert_eq!(
        send(
            &mut engine,
            parent_callback,
            common,
            procedure,
            window,
            [0x401, 1 | 0x200, 0x124a0]
        ),
        1
    );
    assert_eq!(
        send(
            &mut engine,
            parent_callback,
            common,
            procedure,
            window,
            [0x40c, 1, 0]
        ),
        0x02000001
    );
    assert_eq!(
        send(
            &mut engine,
            parent_callback,
            common,
            procedure,
            window,
            [0x40d, 1, 0x12540]
        ),
        0x02000001
    );
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x12540), &mut bytes)
        .unwrap();
    assert_eq!(bytes, [0xe9, 0, 0, 0]);
    assert_eq!(
        send(
            &mut engine,
            parent_callback,
            common,
            procedure,
            window,
            [0x409, 1, 0]
        ),
        0
    );
    assert_eq!(
        send(
            &mut engine,
            parent_callback,
            common,
            procedure,
            window,
            [0x40e, 0, 0]
        ),
        1
    );
    assert_eq!(
        send(
            &mut engine,
            parent_callback,
            common,
            procedure,
            window,
            [0x40c, 0, 0]
        ),
        5
    );
}

#[test]
fn captured_status_rect_and_paint_publish_real_font_metrics_and_pixels() {
    let (mut engine, parent_callback, common, procedure, _, parent) = fixture();
    let window = status(&mut engine, parent_callback, common, procedure, parent);
    assert_eq!(
        send(
            &mut engine,
            parent_callback,
            common,
            procedure,
            window,
            [0x40a, 0, 0x12500]
        ),
        1
    );
    let [left, top, right, bottom] = words::<4>(&engine, 0x12500);
    assert_eq!((left, top, right), (0, 2, 8));
    assert!(bottom > 4);
    assert_eq!(
        send(
            &mut engine,
            parent_callback,
            common,
            procedure,
            window,
            [0x2001, 0, 0x00335577]
        ),
        0xff000000
    );
    assert_eq!(
        send(
            &mut engine,
            parent_callback,
            common,
            procedure,
            window,
            [0x0f, 0, 0]
        ),
        0
    );
    let frame = (0..16)
        .find_map(|index| {
            engine
                .pending_gdi_frame(index)
                .unwrap()
                .filter(|frame| frame.window == window)
        })
        .unwrap();
    assert_eq!((frame.width, frame.height), (8, bottom));
    let pixels = engine
        .read_gdi_frame(frame.id, frame.revision, 0, frame.bytes)
        .unwrap();
    assert_eq!(&pixels[..4], &[0x33, 0x55, 0x77, 0]);
    assert!(
        pixels
            .chunks_exact(4)
            .any(|pixel| pixel[0] < 0x33 && pixel[1] < 0x55 && pixel[2] < 0x77),
        "status text uses genuine realized font glyph pixels"
    );
    assert_eq!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .gdi_update_region(WindowHandle32::try_from(window).unwrap())
            .unwrap()
            .0
            .complexity(),
        1
    );
}
