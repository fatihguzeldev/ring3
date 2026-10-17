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

#[test]
fn captured_status_ownerdraw_uses_parent_callback_real_dc_and_owned_drawitem_scratch() {
    let (mut engine, parent_callback, common, procedure, _, parent) = fixture();
    let window = status(&mut engine, parent_callback, common, procedure, parent);
    let mut observed = Vec::new();
    let mut scratch = 0;
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[window, 0x40b, 0x1000, 0xdeadbeef],
    );
    for _ in 0..64 {
        if engine.pending_windows_callback().is_some() {
            callback_observing(
                &mut engine,
                parent_callback,
                common,
                procedure,
                &mut |engine, arguments| {
                    if arguments[1] != 0x2b {
                        return;
                    }
                    assert_eq!(&arguments[..3], &[parent, 0x2b, 91]);
                    let output = words::<12>(engine, arguments[3]);
                    assert_eq!(&output[..6], &[0, 91, 0, 0, 0, window]);
                    assert_eq!(output[7..10], [0, 2, 8]);
                    assert!(output[10] > 4);
                    assert_eq!(output[11], 0xdeadbeef);
                    engine
                        .gdi
                        .as_ref()
                        .unwrap()
                        .check_control_dc(output[6], window, 1)
                        .unwrap();
                    scratch = arguments[3];
                    observed.push(output);
                },
            );
        } else if let Some(wait) = engine.waiting_user32_message() {
            engine.resume_user32_wait(KEY, wait).unwrap();
        } else {
            break;
        }
    }
    assert!(engine.pending_call.is_none());
    assert_eq!(eax(&engine), 1);
    assert_eq!(
        observed.len(),
        1,
        "setting ownerdraw sends exactly one accepted native parent call"
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(scratch), &mut [0; 48])
            .is_err()
    );
    assert!(
        engine
            .gdi
            .as_ref()
            .unwrap()
            .check_control_dc(observed[0][6], window, 1)
            .is_err()
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
        0x10000000
    );
    assert_eq!(
        send(
            &mut engine,
            parent_callback,
            common,
            procedure,
            window,
            [0x40d, 0, 0]
        ),
        0xdeadbeef
    );
}

fn gdi_call(engine: &mut EngineInstance, api: GdiApi32, args: &[u32]) -> u32 {
    capture(engine, api.id(), args);
    eax(engine)
}
#[test]
fn captured_status_print_uses_its_font_and_caller_mapping_without_changing_borrowed_dc() {
    let (mut engine, parent_callback, common, procedure, _, parent) = fixture();
    let window = status(&mut engine, parent_callback, common, procedure, parent);
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
    let dc = gdi_call(&mut engine, GdiApi32::CreateCompatibleDC, &[0]);
    assert_ne!(dc, 0);
    let bitmap = gdi_call(&mut engine, GdiApi32::CreateBitmap, &[40, 40, 1, 32, 0]);
    assert_ne!(bitmap, 0);
    let old = gdi_call(&mut engine, GdiApi32::SelectObject, &[dc, bitmap]);
    assert_eq!(
        gdi_call(&mut engine, GdiApi32::SetViewportOrgEx, &[dc, 3, 4, 0]),
        1
    );
    let font = gdi_call(&mut engine, GdiApi32::GetCurrentObject, &[dc, 6]);
    let before = engine
        .user32
        .as_ref()
        .unwrap()
        .gdi_update_region(WindowHandle32::try_from(window).unwrap())
        .unwrap();
    assert_eq!(
        send(
            &mut engine,
            parent_callback,
            common,
            procedure,
            window,
            [0x318, dc, 4]
        ),
        0
    );
    assert_eq!(
        gdi_call(&mut engine, GdiApi32::GetPixel, &[dc, 6, 0]),
        0x00335577
    );
    assert_eq!(
        gdi_call(&mut engine, GdiApi32::GetCurrentObject, &[dc, 6]),
        font
    );
    assert_eq!(
        gdi_call(&mut engine, GdiApi32::GetViewportOrgEx, &[dc, 0x12500]),
        1
    );
    assert_eq!(words::<2>(&engine, 0x12500), [3, 4]);
    assert_eq!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .gdi_update_region(WindowHandle32::try_from(window).unwrap())
            .unwrap(),
        before
    );
    assert_eq!(
        gdi_call(&mut engine, GdiApi32::SelectObject, &[dc, old]),
        bitmap
    );
    assert_eq!(gdi_call(&mut engine, GdiApi32::DeleteObject, &[bitmap]), 1);
    assert_eq!(gdi_call(&mut engine, GdiApi32::DeleteDC, &[dc]), 1);
}
#[test]
fn captured_status_icons_use_real_image_planes_and_remain_borrowed_after_window_destruction() {
    let (mut engine, parent_callback, common, procedure, _, parent) = fixture();
    let window = status(&mut engine, parent_callback, common, procedure, parent);
    capture(&mut engine, User32Api32::LoadIconW.id(), &[0, 32512]);
    let icon = eax(&engine);
    assert_ne!(icon, 0);
    assert_eq!(
        send(
            &mut engine,
            parent_callback,
            common,
            procedure,
            window,
            [0x40b, 0x100, 0]
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
            [0x40f, 0, icon]
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
            [0x414, 0, 0]
        ),
        icon
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
    let pixels = engine
        .read_gdi_frame(frame.id, frame.revision, 0, frame.bytes)
        .unwrap();
    assert!(
        pixels
            .chunks_exact(4)
            .any(|pixel| pixel[0] != pixel[1] || pixel[1] != pixel[2]),
        "the real authored application icon contributes colored pixels"
    );
    capture(&mut engine, User32Api32::DestroyWindow.id(), &[window]);
    callbacks(&mut engine, parent_callback, common, procedure);
    assert_eq!(eax(&engine), 1);
    assert!(engine.gdi.as_ref().unwrap().image_info(icon).is_ok());
}
#[test]
fn captured_status_rtl_text_is_retained_and_painted_with_the_actual_canvas_direction() {
    let (mut engine, parent_callback, common, procedure, _, parent) = fixture();
    let window = status(&mut engine, parent_callback, common, procedure, parent);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12400),
            &[0x0645u16, 0x0631, 0x062d, 0]
                .into_iter()
                .flat_map(u16::to_le_bytes)
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
            [0x40b, 0x400, 0x12400]
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
        0x04000003
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
}
