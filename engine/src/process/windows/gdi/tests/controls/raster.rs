use super::super::{call, fixture};
use crate::process::windows::gdi::controls::ControlVisual32;
use crate::windows::{
    controls::{ControlKind32, ControlState32},
    gdi::{GdiApi32, GdiError32, GdiWindow32, Region32},
    user32::Rect32,
};

fn pixels(runtime: &super::super::GdiRuntime32, frame: crate::process::GdiFrame32) -> Vec<u8> {
    let mut bytes = Vec::new();
    for offset in (0..frame.bytes).step_by(4096) {
        bytes.extend_from_slice(
            runtime
                .read_frame(
                    frame.id,
                    frame.revision,
                    offset,
                    (frame.bytes - offset).min(4096),
                )
                .unwrap(),
        );
    }
    bytes
}

#[test]
fn each_standard_control_has_real_pixels_text_and_owned_dc_retirement() {
    for kind in ControlKind32::STANDARD {
        let (mut runtime, mut memory) = fixture();
        let window = GdiWindow32 {
            window: 7,
            thread: 1,
            class_atom: 0xc001,
            class_style: 0,
            background_brush: 0,
            width: 96,
            height: if kind == ControlKind32::ScrollBar {
                16
            } else {
                48
            },
            whole: false,
        };
        let bounds = Rect32 {
            left: 0,
            top: 0,
            right: 96,
            bottom: window.height as i32,
        };
        let dc = runtime
            .begin_control_dc(window, Region32::rectangle(bounds), 0, kind)
            .unwrap();
        let control = ControlState32 {
            check: 1,
            selection: (0, 3),
            items: vec![
                "First".encode_utf16().collect(),
                "Second".encode_utf16().collect(),
            ],
            selected: Some(0),
            scroll: (
                0,
                20,
                if kind == ControlKind32::ScrollBar {
                    7
                } else {
                    0
                },
                5,
            ),
            ..Default::default()
        };
        let label: Vec<_> = "&Hello".encode_utf16().collect();
        let visual = ControlVisual32 {
            kind,
            style: if matches!(kind, ControlKind32::Button | ControlKind32::ComboBox) {
                3
            } else {
                0
            },
            enabled: true,
            focused: true,
            text: &label,
            control: &control,
        };
        runtime.render_control(&mut memory, dc, &visual, 0).unwrap();
        let frame = runtime.frame(0).unwrap();
        let pixels = pixels(&runtime, frame);
        assert!(
            pixels.chunks_exact(4).any(|pixel| pixel[..3] == [0, 0, 0]),
            "{kind:?} must have real ink"
        );
        assert!(
            pixels.chunks_exact(4).any(|pixel| pixel[..3] != [0, 0, 0]),
            "{kind:?} must have real background"
        );
        runtime
            .release_control_dc(dc, window.window, window.thread)
            .unwrap();
        assert_eq!(runtime.model.dc_kind(dc), Err(GdiError32::InvalidHandle));
        assert!(runtime.frame(0).is_some());
    }
}

#[test]
fn pushbutton_ignores_parent_brush_but_static_uses_it_and_owner_draw_is_explicit() {
    for (kind, expected) in [
        (ControlKind32::Button, [0xc0, 0xc0, 0xc0, 0]),
        (ControlKind32::Static, [0x33, 0x22, 0x11, 0]),
    ] {
        let (mut runtime, mut memory) = fixture();
        let window = GdiWindow32 {
            window: 9,
            thread: 1,
            class_atom: 0xc001,
            class_style: 0,
            background_brush: 0,
            width: 96,
            height: 48,
            whole: false,
        };
        let dc = runtime
            .begin_control_dc(
                window,
                Region32::rectangle(Rect32 {
                    left: 0,
                    top: 0,
                    right: 96,
                    bottom: 48,
                }),
                0,
                kind,
            )
            .unwrap();
        let brush = call(
            &mut runtime,
            &mut memory,
            GdiApi32::CreateSolidBrush,
            &[0x00332211],
        )
        .unwrap();
        let control = ControlState32::default();
        let visual = ControlVisual32 {
            kind,
            style: 0,
            enabled: true,
            focused: false,
            text: &[],
            control: &control,
        };
        runtime
            .render_control(&mut memory, dc, &visual, brush)
            .unwrap();
        let frame = runtime.frame(0).unwrap();
        assert_eq!(
            runtime
                .read_frame(frame.id, frame.revision, (40 * 96 + 90) * 4, 4)
                .unwrap(),
            &expected
        );
        let rejected = ControlVisual32 {
            kind: ControlKind32::Button,
            style: 11,
            ..visual
        };
        assert_eq!(
            runtime.render_control(&mut memory, dc, &rejected, brush),
            Err(GdiError32::Unsupported)
        );
        assert_eq!(runtime.frame(0).unwrap().revision, frame.revision);
    }
}

#[test]
fn control_state_changes_have_distinct_pixels_and_update_clip_preserves_other_pixels() {
    for kind in ControlKind32::STANDARD {
        let (mut runtime, mut memory) = fixture();
        let window = GdiWindow32 {
            window: 11,
            thread: 1,
            class_atom: 0xc001,
            class_style: 0,
            background_brush: 0,
            width: 96,
            height: if kind == ControlKind32::ScrollBar {
                16
            } else {
                48
            },
            whole: false,
        };
        let bounds = Rect32 {
            left: 0,
            top: 0,
            right: 96,
            bottom: window.height as i32,
        };
        let dc = runtime
            .begin_control_dc(window, Region32::rectangle(bounds), 0, kind)
            .unwrap();
        let mut control = ControlState32 {
            items: vec![
                "First".encode_utf16().collect(),
                "Second".encode_utf16().collect(),
            ],
            scroll: (0, 20, 0, 5),
            selected: Some(0),
            ..Default::default()
        };
        let label: Vec<_> = "Hello".encode_utf16().collect();
        let style = if matches!(kind, ControlKind32::Button | ControlKind32::ComboBox) {
            3
        } else {
            0
        };
        runtime
            .render_control(
                &mut memory,
                dc,
                &ControlVisual32 {
                    kind,
                    style,
                    enabled: true,
                    focused: true,
                    text: &label,
                    control: &control,
                },
                0,
            )
            .unwrap();
        let first = runtime.frame(0).unwrap();
        let before = pixels(&runtime, first);
        match kind {
            ControlKind32::Button => control.check = 1,
            ControlKind32::Edit => control.selection = (0, 3),
            ControlKind32::ListBox | ControlKind32::ComboBox => control.selected = Some(1),
            ControlKind32::ScrollBar => control.scroll.2 = 20,
            ControlKind32::Static => {}
        }
        let changed: Vec<_> = "World".encode_utf16().collect();
        let label = if kind == ControlKind32::Static {
            &changed
        } else {
            &label
        };
        let mut state = runtime.model.state(dc).unwrap().clone();
        state.clip = Some(Region32::rectangle(Rect32 {
            right: 48,
            ..bounds
        }));
        runtime.model.set_state(dc, state).unwrap();
        runtime
            .render_control(
                &mut memory,
                dc,
                &ControlVisual32 {
                    kind,
                    style,
                    enabled: true,
                    focused: true,
                    text: label,
                    control: &control,
                },
                0,
            )
            .unwrap();
        let second = runtime.frame(0).unwrap();
        let after = pixels(&runtime, second);
        assert_ne!(before, after, "{kind:?} must render its changed state");
        for (before, after) in before.chunks_exact(96 * 4).zip(after.chunks_exact(96 * 4)) {
            assert_eq!(
                &before[48 * 4..],
                &after[48 * 4..],
                "{kind:?} must retain pixels outside the update clip"
            );
        }
        assert_eq!(second.revision, first.revision + 1);
        runtime
            .release_control_dc(dc, window.window, window.thread)
            .unwrap();
    }
}
