use super::{call, dib, fixture, memory};
use crate::{
    memory::{GuestAddress, PageRange, Permissions},
    windows::gdi::{GdiApi32 as Api, GdiError32},
};

#[test]
fn color_pattern_owns_snapshot_after_source_bitmap_mutation_and_deletion() {
    let (mut runtime, mut ram) = fixture();
    let (bitmap, pointer) = dib(&mut runtime, &mut ram, -2);
    memory::write_words(
        &mut ram,
        pointer,
        &[
            0x000000ff, 0x0000ff00, 0x00ff0000, 0x00112233, 0x00445566, 0x00778899,
        ],
    )
    .unwrap();
    let brush = call(&mut runtime, &mut ram, Api::CreatePatternBrush, &[bitmap]).unwrap();
    memory::write_words(&mut ram, pointer, &[0; 6]).unwrap();
    assert_eq!(
        call(&mut runtime, &mut ram, Api::DeleteObject, &[bitmap]),
        Ok(1)
    );
    let dc = call(&mut runtime, &mut ram, Api::CreateCompatibleDC, &[0]).unwrap();
    let (target, _) = dib(&mut runtime, &mut ram, -2);
    call(&mut runtime, &mut ram, Api::SelectObject, &[dc, target]).unwrap();
    let previous = call(&mut runtime, &mut ram, Api::SelectObject, &[dc, brush]).unwrap();
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            Api::PatBlt,
            &[dc, 0, 0, 3, 2, 0x00f00021]
        ),
        Ok(1)
    );
    for (x, y, color) in [
        (0, 0, 0x00ff0000),
        (1, 0, 0x0000ff00),
        (2, 0, 0x000000ff),
        (0, 1, 0x00332211),
    ] {
        assert_eq!(
            call(&mut runtime, &mut ram, Api::GetPixel, &[dc, x, y]),
            Ok(color)
        );
    }
    assert_eq!(
        call(&mut runtime, &mut ram, Api::DeleteObject, &[brush]),
        Err(GdiError32::SelectedObject)
    );
    call(&mut runtime, &mut ram, Api::SelectObject, &[dc, previous]).unwrap();
    assert_eq!(
        call(&mut runtime, &mut ram, Api::DeleteObject, &[brush]),
        Ok(1)
    );
}

#[test]
fn monochrome_pattern_uses_target_dc_text_background_colors_and_device_brush_origin() {
    let (mut runtime, mut ram) = fixture();
    ram.write(GuestAddress(0x10100), &[0x80, 0]).unwrap();
    let bitmap = call(
        &mut runtime,
        &mut ram,
        Api::CreateBitmap,
        &[2, 1, 1, 1, 0x10100],
    )
    .unwrap();
    memory::write_words(&mut ram, 0x10200, &[3, 0xabcdef, bitmap]).unwrap();
    let brush = call(&mut runtime, &mut ram, Api::CreateBrushIndirect, &[0x10200]).unwrap();
    call(&mut runtime, &mut ram, Api::DeleteObject, &[bitmap]).unwrap();
    let dc = call(&mut runtime, &mut ram, Api::CreateCompatibleDC, &[0]).unwrap();
    let (bitmap, _) = dib(&mut runtime, &mut ram, -2);
    call(&mut runtime, &mut ram, Api::SelectObject, &[dc, bitmap]).unwrap();
    call(&mut runtime, &mut ram, Api::SelectObject, &[dc, brush]).unwrap();
    call(&mut runtime, &mut ram, Api::SetTextColor, &[dc, 0x112233]).unwrap();
    call(&mut runtime, &mut ram, Api::SetBkColor, &[dc, 0x778899]).unwrap();
    call(&mut runtime, &mut ram, Api::SetBrushOrgEx, &[dc, 1, 0, 0]).unwrap();
    call(
        &mut runtime,
        &mut ram,
        Api::PatBlt,
        &[dc, 0, 0, 3, 2, 0x00f00021],
    )
    .unwrap();
    for (x, color) in [(0, 0x112233), (1, 0x778899), (2, 0x112233)] {
        assert_eq!(
            call(&mut runtime, &mut ram, Api::GetPixel, &[dc, x, 0]),
            Ok(color)
        );
    }
    call(&mut runtime, &mut ram, Api::SetTextColor, &[dc, 0x010203]).unwrap();
    call(
        &mut runtime,
        &mut ram,
        Api::PatBlt,
        &[dc, 0, 0, 3, 2, 0x00f00021],
    )
    .unwrap();
    assert_eq!(
        call(&mut runtime, &mut ram, Api::GetPixel, &[dc, 0, 0]),
        Ok(0x010203)
    );
}

#[test]
fn pattern_source_fault_and_capacity_failure_publish_no_handle_or_surface_then_deletion_reclaims_budget()
 {
    let (mut runtime, mut ram) = fixture();
    let (bitmap, pointer) = dib(&mut runtime, &mut ram, -2);
    let count = runtime.model.object_count();
    let bytes = runtime.bytes;
    let surfaces = runtime.surfaces.len();
    let range = PageRange::new(GuestAddress(pointer), 1).unwrap();
    ram.protect(range, Permissions::NONE).unwrap();
    let version = ram.transaction_version();
    assert!(matches!(
        call(&mut runtime, &mut ram, Api::CreatePatternBrush, &[bitmap]),
        Err(GdiError32::Memory(_))
    ));
    assert_eq!(runtime.model.object_count(), count);
    assert_eq!(runtime.surfaces.len(), surfaces);
    assert_eq!(runtime.bytes, bytes);
    assert_eq!(ram.transaction_version(), version);
    ram.protect(range, Permissions::READ_WRITE).unwrap();
    runtime.config.bitmap_bytes = 256;
    let filler = call(
        &mut runtime,
        &mut ram,
        Api::CreateBitmap,
        &[58, 1, 1, 32, 0],
    )
    .unwrap();
    let full_count = runtime.model.object_count();
    let full_bytes = runtime.bytes;
    let full_surfaces = runtime.surfaces.len();
    assert_eq!(full_bytes, 256);
    assert_eq!(
        call(&mut runtime, &mut ram, Api::CreatePatternBrush, &[bitmap]),
        Err(GdiError32::Capacity)
    );
    assert_eq!(runtime.model.object_count(), full_count);
    assert_eq!(runtime.surfaces.len(), full_surfaces);
    assert_eq!(runtime.bytes, full_bytes);
    call(&mut runtime, &mut ram, Api::DeleteObject, &[filler]).unwrap();
    for _ in 0..40 {
        let brush = call(&mut runtime, &mut ram, Api::CreatePatternBrush, &[bitmap]).unwrap();
        assert_eq!(runtime.bytes, bytes + 24);
        call(&mut runtime, &mut ram, Api::DeleteObject, &[brush]).unwrap();
        assert_eq!(runtime.bytes, bytes);
        assert_eq!(runtime.model.object_count(), count);
        assert_eq!(runtime.surfaces.len(), surfaces);
    }
    assert!(runtime.model.object(bitmap).is_ok());
}
