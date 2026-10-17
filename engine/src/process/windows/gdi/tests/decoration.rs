use super::{call, fixture, memory};
use crate::{
    memory::{GuestAddress, PageRange, Permissions},
    windows::gdi::{GdiApi32 as Api, GdiError32},
};
fn target(runtime: &mut super::super::GdiRuntime32, ram: &mut crate::memory::AddressSpace) -> u32 {
    let dc = call(runtime, ram, Api::CreateCompatibleDC, &[0]).unwrap();
    let bitmap = call(runtime, ram, Api::CreateBitmap, &[16, 16, 1, 32, 0]).unwrap();
    call(runtime, ram, Api::SelectObject, &[dc, bitmap]).unwrap();
    dc
}

#[test]
fn drawedge_publishes_actual_border_pixels_and_adjusted_rect_in_one_checked_transaction() {
    let (mut runtime, mut ram) = fixture();
    let dc = target(&mut runtime, &mut ram);
    memory::write_words(&mut ram, 0x10100, &[1, 1, 14, 14]).unwrap();
    let index = runtime.dc_surface(dc).unwrap();
    let revision = runtime.surfaces[index].revision;
    let version = ram.transaction_version();
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            Api::DrawEdge,
            &[dc, 0x10100, 5, 15 | 0x800 | 0x2000]
        ),
        Ok(1)
    );
    assert_eq!(memory::words::<4>(&ram, 0x10100).unwrap(), [3, 3, 12, 12]);
    assert_eq!(ram.transaction_version(), version + 1);
    assert_eq!(runtime.surfaces[index].revision, revision + 1);
    let light = runtime.system_color(20);
    let dark = runtime.system_color(21);
    let face = runtime.system_color(15);
    assert_eq!(
        call(&mut runtime, &mut ram, Api::GetPixel, &[dc, 3, 1]),
        Ok(light)
    );
    assert_eq!(
        call(&mut runtime, &mut ram, Api::GetPixel, &[dc, 13, 3]),
        Ok(dark)
    );
    assert_eq!(
        call(&mut runtime, &mut ram, Api::GetPixel, &[dc, 5, 5]),
        Ok(face)
    );
    assert_eq!(
        call(&mut runtime, &mut ram, Api::GetPixel, &[dc, 0, 0]),
        Ok(0)
    );
}

#[test]
fn framecontrol_checkbox_is_a_real_black_white_mask_and_pushbutton_adjusts_its_edge() {
    let (mut runtime, mut ram) = fixture();
    let dc = target(&mut runtime, &mut ram);
    memory::write_words(&mut ram, 0x10100, &[0, 0, 14, 14]).unwrap();
    call(
        &mut runtime,
        &mut ram,
        Api::DrawFrameControl,
        &[dc, 0x10100, 4, 0],
    )
    .unwrap();
    let index = runtime.dc_surface(dc).unwrap();
    let before = runtime.surfaces[index]
        .storage
        .read(&ram, 16 * 16 * 4)
        .unwrap();
    call(
        &mut runtime,
        &mut ram,
        Api::DrawFrameControl,
        &[dc, 0x10100, 4, 0x400],
    )
    .unwrap();
    let after = runtime.surfaces[index]
        .storage
        .read(&ram, 16 * 16 * 4)
        .unwrap();
    assert_ne!(before, after, "checked mask has real ink");
    assert!(
        after
            .chunks_exact(4)
            .all(|pixel| pixel[..3] == [0, 0, 0] || pixel[..3] == [255, 255, 255])
    );
    assert_eq!(memory::words::<4>(&ram, 0x10100).unwrap(), [0, 0, 14, 14]);
    call(
        &mut runtime,
        &mut ram,
        Api::DrawFrameControl,
        &[dc, 0x10100, 4, 16 | 0x2000],
    )
    .unwrap();
    assert_eq!(memory::words::<4>(&ram, 0x10100).unwrap(), [2, 2, 12, 12]);
}

#[test]
fn decoration_output_rights_and_invalid_style_failures_preserve_pixels_dc_and_revision() {
    let (mut runtime, mut ram) = fixture();
    let dc = target(&mut runtime, &mut ram);
    memory::write_words(&mut ram, 0x10100, &[1, 1, 14, 14]).unwrap();
    let page = PageRange::new(GuestAddress(0x10000), 1).unwrap();
    ram.protect(page, Permissions::READ).unwrap();
    let index = runtime.dc_surface(dc).unwrap();
    let before = runtime.surfaces[index]
        .storage
        .read(&ram, 16 * 16 * 4)
        .unwrap();
    let state = runtime.model.state(dc).unwrap().clone();
    let revision = runtime.surfaces[index].revision;
    let version = ram.transaction_version();
    assert!(matches!(
        call(
            &mut runtime,
            &mut ram,
            Api::DrawEdge,
            &[dc, 0x10100, 5, 15 | 0x2000]
        ),
        Err(GdiError32::Memory(_))
    ));
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            Api::DrawFrameControl,
            &[dc, 0x10100, 99, 0]
        ),
        Err(GdiError32::InvalidRequest)
    );
    assert_eq!(runtime.model.state(dc).unwrap(), &state);
    assert_eq!(
        runtime.surfaces[index]
            .storage
            .read(&ram, 16 * 16 * 4)
            .unwrap(),
        before
    );
    assert_eq!(runtime.surfaces[index].revision, revision);
    assert_eq!(ram.transaction_version(), version);
    ram.protect(page, Permissions::READ_WRITE).unwrap();
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            Api::DrawEdge,
            &[dc, 0x10100, 5, 15 | 0x2000]
        ),
        Ok(1)
    );
    assert_eq!(memory::words::<4>(&ram, 0x10100).unwrap(), [3, 3, 12, 12]);
}
