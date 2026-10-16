use super::super::*;

#[test]
fn getclipbox_returns_logical_coordinates_and_output_fault_preserves_device_clip() {
    let (mut runtime, mut memory) = fixture();
    let dc = call(
        &mut runtime,
        &mut memory,
        GdiApi32::CreateCompatibleDC,
        &[0],
    )
    .unwrap();
    let (bitmap, _) = dib(&mut runtime, &mut memory, -2);
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SelectObject,
        &[dc, bitmap],
    )
    .unwrap();
    call(&mut runtime, &mut memory, GdiApi32::SetMapMode, &[dc, 8]).unwrap();
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SetWindowExtEx,
        &[dc, 2, 2, 0],
    )
    .unwrap();
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SetViewportExtEx,
        &[dc, 4, 4, 0],
    )
    .unwrap();
    assert_eq!(
        call(
            &mut runtime,
            &mut memory,
            GdiApi32::GetClipBox,
            &[dc, 0x10100]
        ),
        Ok(2)
    );
    assert_eq!(memory::words::<4>(&memory, 0x10100).unwrap(), [0, 0, 2, 1]);
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::IntersectClipRect,
        &[dc, 1, 0, 2, 1],
    )
    .unwrap();
    let state = runtime.model.state(dc).unwrap().clone();
    let version = memory.transaction_version();
    assert!(
        call(
            &mut runtime,
            &mut memory,
            GdiApi32::GetClipBox,
            &[dc, 0x12000]
        )
        .is_err()
    );
    assert_eq!(runtime.model.state(dc).unwrap(), &state);
    assert_eq!(memory.transaction_version(), version);
    assert_eq!(
        call(
            &mut runtime,
            &mut memory,
            GdiApi32::GetClipBox,
            &[dc, 0x10100]
        ),
        Ok(2)
    );
    assert_eq!(memory::words::<4>(&memory, 0x10100).unwrap(), [1, 0, 2, 1]);
    assert_eq!(runtime.model.state(dc).unwrap(), &state);
}

#[test]
fn rectangle_applies_current_binary_rop_to_both_pen_and_brush_and_failed_write_is_neutral() {
    let (mut runtime, mut memory) = fixture();
    let dc = call(
        &mut runtime,
        &mut memory,
        GdiApi32::CreateCompatibleDC,
        &[0],
    )
    .unwrap();
    let (bitmap, bits) = dib(&mut runtime, &mut memory, -3);
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SelectObject,
        &[dc, bitmap],
    )
    .unwrap();
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SelectObject,
        &[
            dc,
            crate::windows::gdi::GdiContext32::stock_object(6).unwrap(),
        ],
    )
    .unwrap();
    let brush = call(
        &mut runtime,
        &mut memory,
        GdiApi32::CreateSolidBrush,
        &[0x00010203],
    )
    .unwrap();
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SelectObject,
        &[dc, brush],
    )
    .unwrap();
    call(&mut runtime, &mut memory, GdiApi32::SetROP2, &[dc, 7]).unwrap();
    let seed = [0x33, 0x22, 0x11, 0].repeat(9);
    memory.write(GuestAddress(bits), &seed).unwrap();
    let index = runtime.dc_surface(dc).unwrap();
    let revision = runtime.surfaces[index].revision;
    let range = PageRange::new(GuestAddress(bits), 1).unwrap();
    memory.protect(range, Permissions::READ).unwrap();
    let version = memory.transaction_version();
    assert!(
        call(
            &mut runtime,
            &mut memory,
            GdiApi32::Rectangle,
            &[dc, 0, 0, 3, 3]
        )
        .is_err()
    );
    assert_eq!(memory.transaction_version(), version);
    assert_eq!(runtime.surfaces[index].revision, revision);
    assert_eq!(
        runtime.surfaces[index].storage.read(&memory, 36).unwrap(),
        seed
    );
    memory.protect(range, Permissions::READ_WRITE).unwrap();
    assert_eq!(
        call(
            &mut runtime,
            &mut memory,
            GdiApi32::Rectangle,
            &[dc, 0, 0, 3, 3]
        ),
        Ok(1)
    );
    assert_eq!(
        call(&mut runtime, &mut memory, GdiApi32::GetPixel, &[dc, 0, 0]),
        Ok(0x00ccddee)
    );
    assert_eq!(
        call(&mut runtime, &mut memory, GdiApi32::GetPixel, &[dc, 1, 1]),
        Ok(0x00322012)
    );
}
