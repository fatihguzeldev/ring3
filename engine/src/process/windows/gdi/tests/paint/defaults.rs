use super::*;

#[test]
fn captured_default_paint_erases_with_the_real_class_brush_and_releases_its_private_dc() {
    let (mut engine, callback, window) =
        fixture_configured(true, Some(User32Api32::DefWindowProcA), 65_536);
    capture(
        &mut engine,
        User32Api32::DefWindowProcA.id(),
        &[window, 0x0f, 0, 0],
    );
    let request = engine
        .pending_windows_callback()
        .expect("default paint calls guest erase procedure");
    assert_eq!(request.procedure.0, PROC);
    assert_eq!(request.arguments[..2], [window, 0x14]);
    let dc = request.arguments[2];
    let handle = WindowHandle32::try_from(window).unwrap();
    assert_eq!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .gdi_update_region(handle)
            .unwrap()
            .0
            .complexity(),
        1
    );
    let record = engine
        .begin_windows_resident_callback(KEY, callback, RETURN, RETURN_ID)
        .unwrap();
    engine
        .authorize_resident_callback(KEY, callback, record.token)
        .unwrap();
    let bytes: Vec<u8> = std::iter::once(PROC)
        .chain([window, 0x14, dc, 0])
        .flat_map(u32::to_le_bytes)
        .collect();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(STACK), &bytes)
        .unwrap();
    stop(&mut engine, 0x10030, User32Api32::DefWindowProcA.id(), 0);
    let inner = engine
        .capture_resident_callback_call(
            KEY,
            callback,
            record.token,
            CallingConvention32::Stdcall,
            4,
        )
        .unwrap()
        .token;
    engine
        .complete_active_resident_callback_windows_call(KEY, callback, record.token, inner)
        .unwrap();
    assert_eq!(eax(&engine), 1);
    stop(&mut engine, RETURN, RETURN_ID, 1);
    engine
        .finish_windows_resident_callback(KEY, callback, record.token)
        .unwrap();
    assert_eq!(eax(&engine), 0);
    assert!(engine.pending_windows_callback().is_none());
    assert!(engine.gdi.as_ref().unwrap().paints.is_empty());
    assert!(engine.gdi.as_ref().unwrap().model.dc_kind(dc).is_err());
    let frame = engine
        .pending_gdi_frame(0)
        .unwrap()
        .expect("class brush publishes real pixels");
    assert_eq!(frame.window, window);
    let color = engine.gdi.as_ref().unwrap().system_color(15);
    let pixel = [(color >> 16) as u8, (color >> 8) as u8, color as u8, 0];
    assert_eq!(
        engine
            .read_gdi_frame(frame.id, frame.revision, 0, 4)
            .unwrap(),
        pixel
    );
}
