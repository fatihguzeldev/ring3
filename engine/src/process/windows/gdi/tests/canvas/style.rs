use super::super::{GdiApi32 as Api, call, fixture};
use crate::{
    process::windows::gdi::CanvasText32,
    windows::{gdi::GdiError32, user32::Rect32},
};
fn target(runtime: &mut super::super::GdiRuntime32, ram: &mut crate::memory::AddressSpace) -> u32 {
    let dc = call(runtime, ram, Api::CreateCompatibleDC, &[0]).unwrap();
    let bitmap = call(runtime, ram, Api::CreateBitmap, &[64, 32, 1, 32, 0]).unwrap();
    call(runtime, ram, Api::SelectObject, &[dc, bitmap]).unwrap();
    dc
}
#[test]
fn canvas_scoped_font_rtl_and_device_clip_render_without_mutating_the_borrowed_dc() {
    let (mut runtime, mut ram) = fixture();
    let dc = target(&mut runtime, &mut ram);
    let font = call(
        &mut runtime,
        &mut ram,
        Api::CreateFontW,
        &[(-24i32) as u32, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 0, 0, 0],
    )
    .unwrap();
    let state = runtime.model.state(dc).unwrap().clone();
    let clip = Rect32 {
        left: 7,
        top: 3,
        right: 27,
        bottom: 25,
    };
    let mut canvas = runtime.begin_canvas(&ram, dc).unwrap();
    canvas
        .with_text_style(&runtime, font, true, Some(clip), |canvas| {
            canvas.fill(
                &runtime,
                Rect32 {
                    left: 0,
                    top: 0,
                    right: 64,
                    bottom: 32,
                },
                0x102030,
            );
            canvas.text(
                &runtime,
                &ram,
                CanvasText32 {
                    units: &[0x5d0, 0x5d1],
                    bounds: Rect32 {
                        left: 0,
                        top: 0,
                        right: 64,
                        bottom: 32,
                    },
                    color: 0xffffff,
                    background: None,
                    horizontal: 0,
                    middle: false,
                    mnemonic: false,
                    multiline: false,
                },
            )
        })
        .unwrap();
    runtime.publish_canvas(&mut ram, canvas).unwrap();
    assert_eq!(runtime.model.state(dc).unwrap(), &state);
    let index = runtime.dc_surface(dc).unwrap();
    let surface = &runtime.surfaces[index];
    let pixels = surface
        .storage
        .read(&ram, surface.layout.byte_len().unwrap())
        .unwrap();
    let mut ink = false;
    for y in 0..32 {
        for x in 0..64 {
            let color = surface.layout.read_pixel(&pixels, &surface.palette, x, y);
            if (7..27).contains(&x) && (3..25).contains(&y) {
                ink |= color != 0x102030;
            } else {
                assert_eq!(color, 0, "outside scoped device clip at {x},{y}");
            }
        }
    }
    assert!(
        ink,
        "configured font produces actual clipped glyph coverage"
    );
    call(&mut runtime, &mut ram, Api::DeleteObject, &[font]).unwrap();
}
#[test]
fn retired_scoped_font_rejects_canvas_publication_without_changing_pixels() {
    let (mut runtime, mut ram) = fixture();
    let dc = target(&mut runtime, &mut ram);
    let font = call(
        &mut runtime,
        &mut ram,
        Api::CreateFontW,
        &[(-20i32) as u32, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 0, 0, 0],
    )
    .unwrap();
    let mut canvas = runtime.begin_canvas(&ram, dc).unwrap();
    canvas
        .with_text_style(&runtime, font, false, None, |canvas| {
            canvas.fill(
                &runtime,
                Rect32 {
                    left: 0,
                    top: 0,
                    right: 64,
                    bottom: 32,
                },
                0xff,
            );
            Ok(())
        })
        .unwrap();
    let index = runtime.dc_surface(dc).unwrap();
    let before = runtime.surfaces[index].storage.read(&ram, 8192).unwrap();
    let revision = runtime.surfaces[index].revision;
    let version = ram.transaction_version();
    call(&mut runtime, &mut ram, Api::DeleteObject, &[font]).unwrap();
    assert_eq!(
        runtime.publish_canvas(&mut ram, canvas),
        Err(GdiError32::InvalidHandle)
    );
    assert_eq!(
        runtime.surfaces[index].storage.read(&ram, 8192).unwrap(),
        before
    );
    assert_eq!(runtime.surfaces[index].revision, revision);
    assert_eq!(ram.transaction_version(), version);
}
