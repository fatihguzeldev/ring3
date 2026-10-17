use super::{call, dib, fixture, memory};
use crate::{
    memory::{GuestAddress, PageRange, Permissions},
    windows::gdi::{GdiApi32 as Api, GdiError32},
};

fn target(runtime: &mut super::super::GdiRuntime32, ram: &mut crate::memory::AddressSpace) -> u32 {
    let dc = call(runtime, ram, Api::CreateCompatibleDC, &[0]).unwrap();
    let bitmap = call(runtime, ram, Api::CreateBitmap, &[12, 8, 1, 32, 0]).unwrap();
    call(runtime, ram, Api::SelectObject, &[dc, bitmap]).unwrap();
    dc
}
fn pixel(
    runtime: &mut super::super::GdiRuntime32,
    ram: &mut crate::memory::AddressSpace,
    dc: u32,
    x: u32,
    y: u32,
) -> u32 {
    call(runtime, ram, Api::GetPixel, &[dc, x, y]).unwrap()
}

#[test]
fn extended_pen_keeps_literal_extlogpen_metadata_and_real_user_dash_pixels() {
    let (mut runtime, mut ram) = fixture();
    let dc = target(&mut runtime, &mut ram);
    memory::write_words(&mut ram, 0x10100, &[0, 0x123456, 0]).unwrap();
    memory::write_words(&mut ram, 0x10200, &[2, 2]).unwrap();
    let pen = call(
        &mut runtime,
        &mut ram,
        Api::ExtCreatePen,
        &[7, 1, 0x10100, 2, 0x10200],
    )
    .unwrap();
    assert_eq!(
        call(&mut runtime, &mut ram, Api::GetObjectType, &[pen]),
        Ok(11)
    );
    assert_eq!(
        call(&mut runtime, &mut ram, Api::GetObjectW, &[pen, 0, 0]),
        Ok(32)
    );
    assert_eq!(
        call(&mut runtime, &mut ram, Api::GetObjectW, &[pen, 32, 0x10300]),
        Ok(32)
    );
    assert_eq!(
        memory::words::<8>(&ram, 0x10300).unwrap(),
        [7, 1, 0, 0x123456, 0, 2, 2, 2]
    );
    let previous = call(&mut runtime, &mut ram, Api::SelectObject, &[dc, pen]).unwrap();
    call(&mut runtime, &mut ram, Api::LineTo, &[dc, 10, 0]).unwrap();
    for x in 0..10 {
        assert_eq!(
            pixel(&mut runtime, &mut ram, dc, x, 0),
            if x % 4 < 2 { 0x123456 } else { 0 }
        );
    }
    assert_eq!(runtime.model.state(dc).unwrap().current_position.x, 10);
    assert_eq!(
        call(&mut runtime, &mut ram, Api::DeleteObject, &[pen]),
        Err(GdiError32::SelectedObject)
    );
    call(&mut runtime, &mut ram, Api::SelectObject, &[dc, previous]).unwrap();
    call(&mut runtime, &mut ram, Api::DeleteObject, &[pen]).unwrap();
}

#[test]
fn geometric_flat_and_round_caps_have_real_width_and_distinct_endpoint_coverage() {
    for (cap, before_start) in [(0x200, 0), (0, 0xabcdef)] {
        let (mut runtime, mut ram) = fixture();
        let dc = target(&mut runtime, &mut ram);
        memory::write_words(&mut ram, 0x10100, &[0, 0xabcdef, 0]).unwrap();
        let pen = call(
            &mut runtime,
            &mut ram,
            Api::ExtCreatePen,
            &[0x10000 | cap, 3, 0x10100, 0, 0],
        )
        .unwrap();
        call(&mut runtime, &mut ram, Api::SelectObject, &[dc, pen]).unwrap();
        call(&mut runtime, &mut ram, Api::MoveToEx, &[dc, 3, 4, 0]).unwrap();
        call(&mut runtime, &mut ram, Api::LineTo, &[dc, 9, 4]).unwrap();
        assert_eq!(pixel(&mut runtime, &mut ram, dc, 2, 4), before_start);
        for y in [3, 4, 5] {
            assert_eq!(pixel(&mut runtime, &mut ram, dc, 5, y), 0xabcdef);
        }
        assert_eq!(pixel(&mut runtime, &mut ram, dc, 5, 2), 0);
        assert_eq!(pixel(&mut runtime, &mut ram, dc, 5, 6), 0);
    }
}

#[test]
fn extended_pattern_pen_owns_ddb_pattern_and_rejects_dib_pattern_without_publication() {
    let (mut runtime, mut ram) = fixture();
    let (dib, _) = dib(&mut runtime, &mut ram, -2);
    memory::write_words(&mut ram, 0x10100, &[3, 0, dib]).unwrap();
    let count = runtime.model.object_count();
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            Api::ExtCreatePen,
            &[0x10200, 3, 0x10100, 0, 0]
        ),
        Err(GdiError32::InvalidRequest)
    );
    assert_eq!(runtime.model.object_count(), count);
    ram.write(GuestAddress(0x10200), &[0x80, 0]).unwrap();
    let bitmap = call(
        &mut runtime,
        &mut ram,
        Api::CreateBitmap,
        &[2, 1, 1, 1, 0x10200],
    )
    .unwrap();
    memory::write_words(&mut ram, 0x10100, &[3, 0, bitmap]).unwrap();
    let pen = call(
        &mut runtime,
        &mut ram,
        Api::ExtCreatePen,
        &[0x10200, 3, 0x10100, 0, 0],
    )
    .unwrap();
    call(&mut runtime, &mut ram, Api::DeleteObject, &[bitmap]).unwrap();
    let dc = target(&mut runtime, &mut ram);
    call(&mut runtime, &mut ram, Api::SelectObject, &[dc, pen]).unwrap();
    call(&mut runtime, &mut ram, Api::SetTextColor, &[dc, 0x112233]).unwrap();
    call(&mut runtime, &mut ram, Api::SetBkColor, &[dc, 0x778899]).unwrap();
    call(&mut runtime, &mut ram, Api::MoveToEx, &[dc, 1, 4, 0]).unwrap();
    call(&mut runtime, &mut ram, Api::LineTo, &[dc, 10, 4]).unwrap();
    assert_eq!(pixel(&mut runtime, &mut ram, dc, 4, 4), 0x778899);
    assert_eq!(pixel(&mut runtime, &mut ram, dc, 5, 4), 0x112233);
}

#[test]
fn malformed_extended_pen_and_line_output_fault_leave_issuer_pixels_and_current_position_neutral() {
    let (mut runtime, mut ram) = fixture();
    memory::write_words(&mut ram, 0x10100, &[0, 0x123456, 0]).unwrap();
    memory::write_words(&mut ram, 0x10200, &[0; 16]).unwrap();
    let count = runtime.model.object_count();
    for args in [
        [7, 1, 0x10100, 0, 0],
        [7, 1, 0x10100, 17, 0x10200],
        [7, 1, 0x10100, 2, 0x10200],
        [0, 2, 0x10100, 0, 0],
        [0x10008, 3, 0x10100, 0, 0],
        [7, 1, 0x10100, 2, 0x1ffff],
    ] {
        assert!(call(&mut runtime, &mut ram, Api::ExtCreatePen, &args).is_err());
        assert_eq!(runtime.model.object_count(), count);
    }
    let pen = call(
        &mut runtime,
        &mut ram,
        Api::ExtCreatePen,
        &[0, 1, 0x10100, 0, 0],
    )
    .unwrap();
    let dc = call(&mut runtime, &mut ram, Api::CreateCompatibleDC, &[0]).unwrap();
    let (bitmap, pointer) = dib(&mut runtime, &mut ram, -2);
    call(&mut runtime, &mut ram, Api::SelectObject, &[dc, bitmap]).unwrap();
    call(&mut runtime, &mut ram, Api::SelectObject, &[dc, pen]).unwrap();
    let range = PageRange::new(GuestAddress(pointer), 1).unwrap();
    ram.protect(range, Permissions::READ).unwrap();
    let version = ram.transaction_version();
    assert!(matches!(
        call(&mut runtime, &mut ram, Api::LineTo, &[dc, 2, 0]),
        Err(GdiError32::Memory(_))
    ));
    assert_eq!(runtime.model.state(dc).unwrap().current_position.x, 0);
    assert_eq!(ram.transaction_version(), version);
    ram.protect(range, Permissions::READ_WRITE).unwrap();
    call(&mut runtime, &mut ram, Api::LineTo, &[dc, 2, 0]).unwrap();
    assert_eq!(runtime.model.state(dc).unwrap().current_position.x, 2);
    assert_eq!(pixel(&mut runtime, &mut ram, dc, 0, 0), 0x123456);
}
