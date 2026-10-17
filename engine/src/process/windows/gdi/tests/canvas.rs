use super::{call, fixture, memory};
use crate::{
    memory::GuestAddress,
    process::windows::gdi::CanvasText32,
    windows::{
        gdi::{GdiApi32, GdiError32, GdiWindow32},
        user32::Rect32,
    },
};

#[test]
fn shared_canvas_draws_real_glyphs_with_one_revision_and_preserves_client_surface_identity() {
    let (mut runtime, mut ram) = fixture();
    let whole = GdiWindow32 {
        window: 99,
        thread: 1,
        class_atom: 0xc001,
        class_style: 0,
        background_brush: 0,
        width: 64,
        height: 32,
        whole: true,
    };
    let dc = runtime.acquire_dc(whole, 1).unwrap();
    let client = runtime
        .acquire_dc(
            GdiWindow32 {
                whole: false,
                ..whole
            },
            1,
        )
        .unwrap();
    let client_index = runtime.dc_surface(client).unwrap();
    let client_bytes = runtime.surfaces[client_index]
        .storage
        .read(&ram, 64 * 32 * 4)
        .unwrap();
    let index = runtime.dc_surface(dc).unwrap();
    let revision = runtime.surfaces[index].revision;
    let mut canvas = runtime.begin_canvas(&ram, dc).unwrap();
    let bounds = Rect32 {
        left: 0,
        top: 0,
        right: 64,
        bottom: 32,
    };
    canvas.fill(&runtime, bounds, 0x00ff00);
    canvas
        .text(
            &runtime,
            &ram,
            CanvasText32 {
                units: &[77, 101, 110, 117],
                bounds,
                color: 0,
                background: Some(0x00ff00),
                horizontal: 0,
                middle: false,
                mnemonic: true,
                multiline: false,
            },
        )
        .unwrap();
    runtime.publish_canvas(&mut ram, canvas).unwrap();
    assert_eq!(runtime.surfaces[index].revision, revision + 1);
    let bytes = runtime.surfaces[index]
        .storage
        .read(&ram, 64 * 32 * 4)
        .unwrap();
    assert!(
        bytes.chunks_exact(4).any(|pixel| pixel[..3] == [0, 0, 0]),
        "configured font produces real glyph ink"
    );
    assert!(bytes.chunks_exact(4).any(|pixel| pixel[..3] == [0, 255, 0]));
    assert_eq!(
        runtime.surfaces[client_index]
            .storage
            .read(&ram, 64 * 32 * 4)
            .unwrap(),
        client_bytes
    );
    let frame = (0..4)
        .filter_map(|slot| runtime.frame(slot))
        .find(|frame| frame.window == 99 && frame.window_dc)
        .unwrap();
    assert_eq!(frame.revision, revision + 1);
    assert!(!runtime.release_window_dc(99, dc, 2).unwrap());
    assert!(!runtime.release_window_dc(100, dc, 1).unwrap());
    assert!(runtime.model.dc_kind(dc).is_ok());
    assert!(runtime.release_window_dc(99, dc, 1).unwrap());
    assert!(runtime.release_window_dc(99, client, 1).unwrap());
    assert_eq!(
        runtime.release_window_dc(99, dc, 1),
        Err(GdiError32::InvalidHandle)
    );
    assert_eq!(
        runtime.surfaces[index]
            .storage
            .read(&ram, 64 * 32 * 4)
            .unwrap(),
        bytes
    );
}

#[test]
fn stale_canvas_rejects_ram_dc_and_palette_changes_without_overwriting_current_pixels() {
    for mutation in 0..4 {
        let (mut runtime, mut ram) = fixture();
        memory::write_words(
            &mut ram,
            0x10000,
            &[40, 8, u32::MAX, 1 | 1 << 16, 0, 4, 0, 0, 2, 0, 0, 0xffffff],
        )
        .unwrap();
        let bitmap = call(
            &mut runtime,
            &mut ram,
            GdiApi32::CreateDIBSection,
            &[0, 0x10000, 0, 0x10080, 0, 0],
        )
        .unwrap();
        let [bits] = memory::words(&ram, 0x10080).unwrap();
        let dc = call(&mut runtime, &mut ram, GdiApi32::CreateCompatibleDC, &[0]).unwrap();
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::SelectObject,
            &[dc, bitmap],
        )
        .unwrap();
        ram.write(
            GuestAddress(0x10100),
            &[0, 3, 2, 0, 0, 0, 0, 0, 255, 255, 255, 0],
        )
        .unwrap();
        let palette = call(&mut runtime, &mut ram, GdiApi32::CreatePalette, &[0x10100]).unwrap();
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::SelectPalette,
            &[dc, palette, 0],
        )
        .unwrap();
        ram.write(GuestAddress(0x10200), &[0, 0, 255, 0]).unwrap();
        let mut canvas = runtime.begin_canvas(&ram, dc).unwrap();
        canvas.fill(
            &runtime,
            Rect32 {
                left: 0,
                top: 0,
                right: 8,
                bottom: 1,
            },
            0xffffff,
        );
        match mutation {
            0 => ram.write(GuestAddress(bits), &[0x80]).unwrap(),
            1 => {
                call(&mut runtime, &mut ram, GdiApi32::SetBkColor, &[dc, 0xff00]).unwrap();
            }
            2 => {
                call(
                    &mut runtime,
                    &mut ram,
                    GdiApi32::SetDIBColorTable,
                    &[dc, 0, 1, 0x10200],
                )
                .unwrap();
            }
            3 => {
                call(
                    &mut runtime,
                    &mut ram,
                    GdiApi32::SetPaletteEntries,
                    &[palette, 0, 1, 0x10200],
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let version = ram.transaction_version();
        let index = runtime.dc_surface(dc).unwrap();
        let revision = runtime.surfaces[index].revision;
        let mut bytes = [0; 4];
        ram.read(GuestAddress(bits), &mut bytes).unwrap();
        assert_eq!(
            runtime.publish_canvas(&mut ram, canvas),
            Err(GdiError32::InvalidRequest),
            "mutation {mutation}"
        );
        assert_eq!(ram.transaction_version(), version);
        assert_eq!(runtime.surfaces[index].revision, revision);
        let mut current = [0; 4];
        ram.read(GuestAddress(bits), &mut current).unwrap();
        assert_eq!(current, bytes);
    }
}

#[path = "canvas/style.rs"]
mod style;
