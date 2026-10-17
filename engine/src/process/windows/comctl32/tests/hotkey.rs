use super::*;

fn hotkey(
    engine: &mut EngineInstance,
    callback: u64,
    common: u64,
    procedure: u32,
    parent: u32,
) -> u32 {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12400),
            &"msctls_hotkey32\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    capture(
        engine,
        User32Api32::CreateWindowExW.id(),
        &[
            0, 0x12400, 0, 0x50000000, 0, 0, 120, 28, parent, 93, 0x400000, 0,
        ],
    );
    callbacks(engine, callback, common, procedure);
    let window = eax(engine);
    assert_ne!(window, 0);
    window
}

#[test]
fn captured_hotkey_registration_packed_messages_and_keyboard_changes_notify_the_real_parent() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x12300), &[8, 0, 0, 0, 0x40, 0, 0, 0])
        .unwrap();
    capture(
        &mut engine,
        CommonControlsApi32::InitCommonControlsEx.id(),
        &[0x12300],
    );
    assert_eq!(eax(&engine), 1);
    let window = hotkey(&mut engine, callback, common, procedure, parent);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x401, 0x641, 0]
        ),
        0
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x402, 0, 0]
        ),
        0x641
    );
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x403, 1, 4],
    );
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[window, 0x100, 66, 0],
    );
    let mut notifications = Vec::new();
    for _ in 0..32 {
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
    assert!(engine.pending_call.is_none());
    assert_eq!(notifications, [(93 | 0x300 << 16, window)]);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x402, 0, 0]
        ),
        0x442
    );
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0xf, 0, 0],
    );
    let frame = (0..16)
        .filter_map(|i| engine.pending_gdi_frame(i).ok().flatten())
        .find(|frame| frame.window == window)
        .unwrap();
    assert_eq!((frame.width, frame.height), (120, 28));
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
    assert!(
        pixels
            .chunks_exact(4)
            .enumerate()
            .any(|(i, p)| i / 120 > 3 && i % 120 > 3 && i % 120 < 100 && p[..3] == [0, 0, 0])
    );
}
