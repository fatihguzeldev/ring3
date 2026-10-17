use super::*;
fn toolbar(
    engine: &mut EngineInstance,
    callback: u64,
    common: u64,
    procedure: u32,
    parent: u32,
) -> u32 {
    toolbar_with_style(engine, callback, common, procedure, parent, 0)
}
fn toolbar_with_style(
    engine: &mut EngineInstance,
    callback: u64,
    common: u64,
    procedure: u32,
    parent: u32,
    style: u32,
) -> u32 {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12400),
            &"ToolbarWindow32\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    capture(
        engine,
        User32Api32::CreateWindowExW.id(),
        &[
            0,
            0x12400,
            0,
            0x50000000 | style,
            0,
            0,
            96,
            24,
            parent,
            93,
            0x400000,
            0,
        ],
    );
    callbacks(engine, callback, common, procedure);
    let window = eax(engine);
    assert_ne!(window, 0);
    window
}
fn put(engine: &mut EngineInstance, words: &[u32]) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12500),
            &words
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
}
#[test]
fn captured_toolbar_owns_unicode_button_text_and_publishes_real_command_check_and_pixels() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let bar = toolbar(&mut engine, callback, common, procedure, parent);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x41e, 20, 0]
        ),
        0
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x41f, 0, 48 | (22 << 16)]
        ),
        1
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12600),
            &"Play\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    put(
        &mut engine,
        &[(-2i32) as u32, 77, 4 | (2 << 8), 0x12345678, 0x12600],
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x444, 1, 0x12500]
        ),
        1
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x12600), &[0; 10])
        .unwrap();
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x417, 0, 0x12500]
        ),
        1
    );
    assert_eq!(
        words::<5>(&engine, 0x12500),
        [(-2i32) as u32, 77, 4 | (2 << 8), 0x12345678, 0]
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x44b, 77, 0x12600]
        ),
        4
    );
    let mut text = [0; 10];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x12600), &mut text)
        .unwrap();
    assert_eq!(text, [80, 0, 108, 0, 97, 0, 121, 0, 0, 0]);
    send(
        &mut engine,
        callback,
        common,
        procedure,
        bar,
        [0x201, 1, 4 | (4 << 16)],
    );
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[bar, 0x202, 0, 4 | (4 << 16)],
    );
    let mut notifications = Vec::new();
    for _ in 0..64 {
        if engine.pending_windows_callback().is_some() {
            callback_observing(&mut engine, callback, common, procedure, &mut |_, args| {
                if args[0] == parent && args[1] == 0x111 {
                    notifications.push((args[2], args[3]));
                }
            });
        } else if let Some(wait) = engine.waiting_user32_message() {
            engine.resume_user32_wait(KEY, wait).unwrap();
        } else {
            break;
        }
    }
    assert_eq!(notifications, [(77, bar)]);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x412, 77, 0]
        ),
        5
    );
    send(&mut engine, callback, common, procedure, bar, [0xf, 0, 0]);
    let frame = (0..16)
        .filter_map(|i| engine.pending_gdi_frame(i).ok().flatten())
        .find(|f| f.window == bar)
        .unwrap();
    let mut pixels = Vec::new();
    for offset in (0..frame.bytes).step_by(4096) {
        pixels.extend_from_slice(
            engine
                .read_gdi_frame(
                    frame.id,
                    frame.revision,
                    offset,
                    (frame.bytes - offset).min(4096),
                )
                .unwrap(),
        );
    }
    assert!(pixels.chunks_exact(4).enumerate().any(|(i, p)| i / 96 > 4
        && i / 96 < 18
        && i % 96 > 4
        && i % 96 < 44
        && p[..3] == [0, 0, 0]));
}
#[test]
fn captured_toolbar_ansi_pool_and_deleted_pressed_button_do_not_dispatch_replacement() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let bar = toolbar(&mut engine, callback, common, procedure, parent);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x12600), b"One\0Two\0\0")
        .unwrap();
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x41c, 0, 0x12600]
        ),
        0
    );
    put(&mut engine, &[(-2i32) as u32, 81, 4, 0, 1]);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x414, 1, 0x12500]
        ),
        1
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x42d, 81, 0x12600]
        ),
        3
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x45c, 4 | (1 << 16), 0x12600]
        ),
        3
    );
    let mut text = [0; 4];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x12600), &mut text)
        .unwrap();
    assert_eq!(&text, b"Two\0");
    send(
        &mut engine,
        callback,
        common,
        procedure,
        bar,
        [0x201, 1, 2 | (2 << 16)],
    );
    assert_eq!(
        send(&mut engine, callback, common, procedure, bar, [0x416, 0, 0]),
        1
    );
    put(&mut engine, &[(-2i32) as u32, 82, 4, 0, 0]);
    send(
        &mut engine,
        callback,
        common,
        procedure,
        bar,
        [0x414, 1, 0x12500],
    );
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[bar, 0x202, 0, 2 | (2 << 16)],
    );
    for _ in 0..64 {
        if engine.pending_windows_callback().is_some() {
            callback_observing(&mut engine, callback, common, procedure, &mut |_, args| {
                assert!(!(args[0] == parent && args[1] == 0x111));
            });
        } else if let Some(wait) = engine.waiting_user32_message() {
            engine.resume_user32_wait(KEY, wait).unwrap();
        } else {
            break;
        }
    }
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x412, 82, 0]
        ),
        4
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x41d, 0, 0x12600]
        ),
        1
    );
    assert_eq!(words::<4>(&engine, 0x12600), [0, 0, 24, 22]);
}

#[test]
fn captured_toolbar_autosize_distinguishes_bottom_nomovey_and_parent_alignment() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let bottom = toolbar_with_style(&mut engine, callback, common, procedure, parent, 3);
    send(
        &mut engine,
        callback,
        common,
        procedure,
        bottom,
        [0x421, 0, 0],
    );
    capture(
        &mut engine,
        User32Api32::GetWindowRect.id(),
        &[bottom, 0x12580],
    );
    assert_eq!(words::<4>(&engine, 0x12580), [0, (-16i32) as u32, 8, 6]);
    capture(
        &mut engine,
        User32Api32::SetWindowPos.id(),
        &[parent, 0, 0, 0, 120, 80, 0x14],
    );
    callbacks(&mut engine, callback, common, procedure);
    assert_eq!(eax(&engine), 1);
    send(
        &mut engine,
        callback,
        common,
        procedure,
        bottom,
        [0x421, 0, 0],
    );
    capture(
        &mut engine,
        User32Api32::GetWindowRect.id(),
        &[bottom, 0x12580],
    );
    assert_eq!(words::<4>(&engine, 0x12580), [0, 58, 120, 80]);
    for style in [2, 9] {
        let bar = toolbar_with_style(&mut engine, callback, common, procedure, parent, style);
        capture(
            &mut engine,
            User32Api32::SetWindowPos.id(),
            &[bar, 0, 11, 7, 75, 18, 0x14],
        );
        callbacks(&mut engine, callback, common, procedure);
        assert_eq!(eax(&engine), 1);
        send(&mut engine, callback, common, procedure, bar, [0x421, 0, 0]);
        capture(
            &mut engine,
            User32Api32::GetWindowRect.id(),
            &[bar, 0x12580],
        );
        let expected = if style == 2 {
            [0, 7, 120, 29]
        } else {
            [11, 7, 86, 29]
        };
        assert_eq!(words::<4>(&engine, 0x12580), expected);
        capture(&mut engine, User32Api32::DestroyWindow.id(), &[bar]);
        callbacks(&mut engine, callback, common, procedure);
        assert_eq!(eax(&engine), 1);
    }
}

#[test]
fn captured_toolbar_physical_input_preserves_unrelated_programmatic_pressed_state() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let bar = toolbar(&mut engine, callback, common, procedure, parent);
    put(
        &mut engine,
        &[
            (-2i32) as u32,
            81,
            4,
            0,
            u32::MAX,
            (-2i32) as u32,
            82,
            4,
            0,
            u32::MAX,
        ],
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x444, 2, 0x12500]
        ),
        1
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x403, 82, 1]
        ),
        1
    );
    send(
        &mut engine,
        callback,
        common,
        procedure,
        bar,
        [0x200, 0, 2 | (2 << 16)],
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x412, 82, 0]
        ),
        6
    );
    send(
        &mut engine,
        callback,
        common,
        procedure,
        bar,
        [0x201, 1, 2 | (2 << 16)],
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x412, 81, 0]
        ),
        6
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x412, 82, 0]
        ),
        6
    );
    send(
        &mut engine,
        callback,
        common,
        procedure,
        bar,
        [0x200, 1, 90 | (20 << 16)],
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x412, 81, 0]
        ),
        4
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x412, 82, 0]
        ),
        6
    );
    send(
        &mut engine,
        callback,
        common,
        procedure,
        bar,
        [0x202, 0, 90 | (20 << 16)],
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x412, 82, 0]
        ),
        6
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x403, 82, 0]
        ),
        1
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x412, 82, 0]
        ),
        4
    );
}

#[test]
fn captured_toolbar_indeterminate_enabled_button_has_real_gray_glyph_pixels() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let bar = toolbar(&mut engine, callback, common, procedure, parent);
    send(
        &mut engine,
        callback,
        common,
        procedure,
        bar,
        [0x41f, 0, 48 | (22 << 16)],
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12600),
            &"Play\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    put(&mut engine, &[(-2i32) as u32, 77, 4, 0, 0x12600]);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            bar,
            [0x444, 1, 0x12500]
        ),
        1
    );
    for indeterminate in [false, true] {
        assert_eq!(
            send(
                &mut engine,
                callback,
                common,
                procedure,
                bar,
                [0x405, 77, u32::from(indeterminate)]
            ),
            1
        );
        send(&mut engine, callback, common, procedure, bar, [0xf, 0, 0]);
        let frame = (0..16)
            .filter_map(|i| engine.pending_gdi_frame(i).ok().flatten())
            .find(|f| f.window == bar)
            .unwrap();
        let mut pixels = Vec::new();
        for offset in (0..frame.bytes).step_by(4096) {
            pixels.extend_from_slice(
                engine
                    .read_gdi_frame(
                        frame.id,
                        frame.revision,
                        offset,
                        (frame.bytes - offset).min(4096),
                    )
                    .unwrap(),
            );
        }
        let interior: Vec<_> = pixels
            .chunks_exact(4)
            .enumerate()
            .filter(|(i, _)| i / 96 > 4 && i / 96 < 18 && i % 96 > 4 && i % 96 < 44)
            .map(|(_, p)| p)
            .collect();
        if indeterminate {
            assert!(interior.iter().any(|p| p[..3] == [128, 128, 128]));
            assert!(!interior.iter().any(|p| p[..3] == [0, 0, 0]));
        } else {
            assert!(interior.iter().any(|p| p[..3] == [0, 0, 0]));
        }
    }
}
