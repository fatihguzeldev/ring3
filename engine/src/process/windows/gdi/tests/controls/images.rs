use super::super::{call, fixture};
use crate::{
    process::windows::gdi::{
        GdiDibSource32, GdiImageKind32, GdiImageRequest32, ResourceBitmapOptions32,
        controls::ControlVisual32,
    },
    windows::{
        controls::{ControlImage32, ControlKind32, ControlState32},
        gdi::{GdiApi32, GdiWindow32, Region32},
        user32::{Point32, Rect32, images::ImageKind32},
    },
};
fn dib(height: i32, pixels: &[u8]) -> Vec<u8> {
    let mut bytes = [
        40,
        2,
        height as u32,
        1 | 32 << 16,
        0,
        pixels.len() as u32,
        0,
        0,
        0,
        0,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect::<Vec<_>>();
    bytes.extend_from_slice(pixels);
    bytes
}
fn dc(runtime: &mut super::super::GdiRuntime32) -> u32 {
    runtime
        .begin_control_dc(
            GdiWindow32 {
                window: 7,
                thread: 1,
                class_atom: 0xc001,
                class_style: 0,
                background_brush: 0,
                width: 4,
                height: 4,
                whole: false,
            },
            Region32::rectangle(Rect32 {
                left: 0,
                top: 0,
                right: 4,
                bottom: 4,
            }),
            0,
            ControlKind32::Static,
        )
        .unwrap()
}
#[test]
fn static_bitmaps_use_owned_pixels_center_clip_and_independent_axis_scaling() {
    for style in [14, 14 | 0x200, 14 | 0x40] {
        let (mut runtime, mut memory) = fixture();
        let data = dib(
            -2,
            &[0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0],
        );
        let prepared = runtime
            .prepare_resource_bitmap(
                &mut memory,
                GdiDibSource32 {
                    info: &data,
                    pixels: None,
                },
                ResourceBitmapOptions32::default(),
                None,
            )
            .unwrap();
        let image = runtime
            .commit_resource_bitmap(&mut memory, prepared)
            .unwrap();
        let dc = dc(&mut runtime);
        let control = ControlState32 {
            image: Some(ControlImage32 {
                kind: ImageKind32::Bitmap,
                handle: image,
            }),
            ..Default::default()
        };
        runtime
            .render_control(
                &mut memory,
                dc,
                &ControlVisual32 {
                    kind: ControlKind32::Static,
                    style,
                    enabled: true,
                    focused: false,
                    text: &[],
                    control: &control,
                },
                0,
            )
            .unwrap();
        let frame = runtime.frame(0).unwrap();
        let bytes = runtime.read_frame(frame.id, frame.revision, 0, 64).unwrap();
        let pixel = |x: usize, y: usize| &bytes[(y * 4 + x) * 4..(y * 4 + x + 1) * 4];
        if style & 0x200 != 0 {
            assert_eq!(pixel(0, 0), &[0, 0, 255, 0]);
            assert_eq!(pixel(1, 1), &[0, 0, 255, 0]);
            assert_eq!(pixel(2, 1), &[0, 255, 0, 0]);
            assert_eq!(pixel(1, 2), &[255, 0, 0, 0]);
            assert_eq!(pixel(2, 2), &[255, 255, 255, 0]);
        } else if style & 0x40 != 0 {
            assert_eq!(pixel(1, 1), &[0, 0, 255, 0]);
            assert_eq!(pixel(2, 1), &[0, 255, 0, 0]);
            assert_eq!(pixel(1, 2), &[255, 0, 0, 0]);
            assert_eq!(pixel(3, 3), &[255, 255, 255, 0]);
        } else {
            assert_eq!(pixel(0, 0), &[0, 0, 255, 0]);
            assert_eq!(pixel(1, 0), &[0, 255, 0, 0]);
            assert_eq!(pixel(0, 1), &[255, 0, 0, 0]);
            assert_eq!(pixel(1, 1), &[255, 255, 255, 0]);
        }
    }
}
#[test]
fn static_icons_use_existing_mask_and_color_composition() {
    let (mut runtime, mut memory) = fixture();
    let data = dib(
        4,
        &[
            0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    );
    let prepared = runtime
        .prepare_image_group(
            &mut memory,
            GdiImageRequest32 {
                kind: GdiImageKind32::Icon,
                dib: GdiDibSource32 {
                    info: &data,
                    pixels: None,
                },
                hotspot: Point32::default(),
                width: 0,
                height: 0,
                monochrome: false,
                transparent: false,
                map3d: false,
                vga: false,
            },
            None,
        )
        .unwrap();
    let image = runtime.commit_image_group(&mut memory, prepared).unwrap();
    let dc = dc(&mut runtime);
    let control = ControlState32 {
        image: Some(ControlImage32 {
            kind: ImageKind32::Icon,
            handle: image,
        }),
        ..Default::default()
    };
    runtime
        .render_control(
            &mut memory,
            dc,
            &ControlVisual32 {
                kind: ControlKind32::Static,
                style: 3 | 0x200,
                enabled: true,
                focused: false,
                text: &[],
                control: &control,
            },
            0,
        )
        .unwrap();
    let frame = runtime.frame(0).unwrap();
    let bytes = runtime.read_frame(frame.id, frame.revision, 0, 64).unwrap();
    assert_eq!(&bytes[20..24], &[255, 0, 0, 0]);
    assert_eq!(&bytes[24..28], &[255, 255, 255, 0]);
}
#[test]
fn bitmap_canvas_rejects_retired_source_without_publishing_pending_pixels() {
    let (mut runtime, mut memory) = fixture();
    let data = dib(-2, &[0; 16]);
    let prepared = runtime
        .prepare_resource_bitmap(
            &mut memory,
            GdiDibSource32 {
                info: &data,
                pixels: None,
            },
            ResourceBitmapOptions32::default(),
            None,
        )
        .unwrap();
    let image = runtime
        .commit_resource_bitmap(&mut memory, prepared)
        .unwrap();
    let dc = dc(&mut runtime);
    let mut canvas = runtime.begin_canvas(&memory, dc).unwrap();
    canvas
        .bitmap(
            &runtime,
            &memory,
            image,
            Rect32 {
                left: 0,
                top: 0,
                right: 2,
                bottom: 2,
            },
            false,
        )
        .unwrap();
    call(&mut runtime, &mut memory, GdiApi32::DeleteObject, &[image]).unwrap();
    let before = runtime.frame(0);
    assert!(runtime.publish_canvas(&mut memory, canvas).is_err());
    assert_eq!(
        runtime.frame(0).map(|frame| frame.revision),
        before.map(|frame| frame.revision)
    );
}
