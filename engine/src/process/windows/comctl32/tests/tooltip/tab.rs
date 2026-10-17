use super::*;
fn tab_window(
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
            &"SysTabControl32\0"
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
            80,
            40,
            parent,
            92,
            0x400000,
            0,
        ],
    );
    callbacks(engine, callback, common, procedure);
    let window = eax(engine);
    assert_ne!(window, 0);
    window
}
fn tab_item(engine: &mut EngineInstance, callback: u64, common: u64, procedure: u32, window: u32) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12600),
            &"label is not the description\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12500),
            &[1u32, 0, 0, 0x12600, 0, u32::MAX, 0]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(
        send(
            engine,
            callback,
            common,
            procedure,
            window,
            [0x133e, 0, 0x12500]
        ),
        0
    );
}
#[test]
fn captured_tab_owned_tooltip_requests_parent_description_and_retires_only_its_own_window() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = tab_window(&mut engine, callback, common, procedure, parent, 0x4000);
    let tooltip = send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x132d, 0, 0],
    );
    assert_ne!(tooltip, 0);
    assert_ne!(tooltip, window);
    tab_item(&mut engine, callback, common, procedure, window);
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x200, 0, 3 | (2 << 16)],
    );
    engine.advance_cpu_clock(500_000_000).unwrap();
    capture(
        &mut engine,
        User32Api32::PeekMessageW.id(),
        &[0x12900, tooltip, 0x113, 0x113, 1],
    );
    assert_eq!(eax(&engine), 1);
    capture(&mut engine, User32Api32::DispatchMessageW.id(), &[0x12900]);
    let mut descriptions = 0;
    for _ in 0..128 {
        if engine.pending_windows_callback().is_some() {
            callback_returning(
                &mut engine,
                callback,
                common,
                procedure,
                &mut |engine, args| {
                    if args[0] == parent
                        && args[1] == 0x4e
                        && words::<3>(engine, args[3])[2] == (-520i32) as u32
                    {
                        assert_eq!(
                            decode_state(&engine.arena()[..STATE_SIZE]).unwrap().eip,
                            PROC
                        );
                        assert_eq!(words::<3>(engine, args[3]), [tooltip, 0, (-520i32) as u32]);
                        descriptions += 1;
                        engine
                            .memory
                            .as_mut()
                            .unwrap()
                            .write(GuestAddress(0x12800), b"actual parent description\0")
                            .unwrap();
                        engine
                            .memory
                            .as_mut()
                            .unwrap()
                            .write(GuestAddress(args[3] + 12), &0x12800u32.to_le_bytes())
                            .unwrap();
                        engine
                            .memory
                            .as_mut()
                            .unwrap()
                            .write(GuestAddress(args[3] + 100), &0x8010u32.to_le_bytes())
                            .unwrap();
                    }
                    None
                },
            );
        } else if let Some(wait) = engine.waiting_user32_message() {
            engine.resume_user32_wait(KEY, wait).unwrap();
        } else {
            assert!(engine.pending_call.is_none());
            break;
        }
    }
    assert_eq!(descriptions, 1);
    assert!(
        engine
            .user32_windows()
            .unwrap()
            .get(WindowHandle32::try_from(tooltip).unwrap())
            .unwrap()
            .description
            .visible
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12800),
            &"replaced source memory\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12500),
            &[44u32, 0, window, 0, 0, 0, 0, 0, 0, 0x13000, 0]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            tooltip,
            [0x435, 0, 0x12500]
        ),
        1
    );
    let expected = "actual parent description\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let mut bytes = vec![0; expected.len()];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x13000), &mut bytes)
        .unwrap();
    assert_eq!(bytes, expected);
    capture(&mut engine, User32Api32::DestroyWindow.id(), &[window]);
    callbacks(&mut engine, callback, common, procedure);
    assert_eq!(eax(&engine), 1);
    assert!(
        engine
            .user32_windows()
            .unwrap()
            .get(WindowHandle32::try_from(tooltip).unwrap())
            .is_err()
    );
    assert!(
        engine
            .user32_windows()
            .unwrap()
            .get(WindowHandle32::try_from(parent).unwrap())
            .is_ok()
    );
}
#[test]
fn captured_tab_explicit_tooltip_remains_borrowed_when_tab_is_destroyed() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = tab_window(&mut engine, callback, common, procedure, parent, 0);
    let tooltip = tooltip(&mut engine, callback, common, procedure);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x132d, 0, 0]
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
            [0x132e, tooltip, 0]
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
            [0x132d, 0, 0]
        ),
        tooltip
    );
    capture(&mut engine, User32Api32::DestroyWindow.id(), &[window]);
    callbacks(&mut engine, callback, common, procedure);
    assert_eq!(eax(&engine), 1);
    assert!(
        engine
            .user32_windows()
            .unwrap()
            .get(WindowHandle32::try_from(tooltip).unwrap())
            .is_ok()
    );
}

#[test]
fn tab_tooltip_callback_cannot_publish_for_a_deleted_and_same_index_replaced_tab() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = tab_window(&mut engine, callback, common, procedure, parent, 0x4000);
    let tooltip = send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x132d, 0, 0],
    );
    tab_item(&mut engine, callback, common, procedure, window);
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x200, 0, 3 | (2 << 16)],
    );
    engine.advance_cpu_clock(500_000_000).unwrap();
    capture(
        &mut engine,
        User32Api32::PeekMessageW.id(),
        &[0x12900, tooltip, 0x113, 0x113, 1],
    );
    assert_eq!(eax(&engine), 1);
    capture(&mut engine, User32Api32::DispatchMessageW.id(), &[0x12900]);
    let mut requests = 0;
    let mut shows = 0;
    for _ in 0..128 {
        if engine.pending_windows_callback().is_some() {
            callback_returning(
                &mut engine,
                callback,
                common,
                procedure,
                &mut |engine, args| {
                    if args[0] == parent && args[1] == 0x4e {
                        let note = words::<3>(engine, args[3]);
                        if note[2] == (-520i32) as u32 {
                            assert_eq!(note, [tooltip, 0, (-520i32) as u32]);
                            requests += 1;
                            assert_eq!(
                                engine
                                    .execute_common_control_procedure(&[window, 0x1308, 0, 0], true)
                                    .unwrap(),
                                crate::process::windows::user32::User32Outcome32::Return(1)
                            );
                            assert_eq!(
                                engine
                                    .execute_common_control_procedure(
                                        &[window, 0x133e, 0, 0x12500],
                                        true
                                    )
                                    .unwrap(),
                                crate::process::windows::user32::User32Outcome32::Return(0)
                            );
                            engine
                                .memory
                                .as_mut()
                                .unwrap()
                                .write(GuestAddress(args[3] + 16), b"deleted tab description\0")
                                .unwrap();
                            engine
                                .memory
                                .as_mut()
                                .unwrap()
                                .write(GuestAddress(args[3] + 100), &0x8010u32.to_le_bytes())
                                .unwrap();
                        } else if note[2] == (-521i32) as u32 {
                            shows += 1;
                        }
                    }
                    None
                },
            );
        } else if let Some(wait) = engine.waiting_user32_message() {
            engine.resume_user32_wait(KEY, wait).unwrap();
        } else {
            assert!(engine.pending_call.is_none());
            break;
        }
    }
    assert_eq!(requests, 1);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x1304, 0, 0]
        ),
        1
    );
    assert_eq!(
        shows, 0,
        "the returned text belongs to the deleted tab key, not its new same-index replacement"
    );
    assert!(
        !engine
            .user32_windows()
            .unwrap()
            .get(WindowHandle32::try_from(tooltip).unwrap())
            .unwrap()
            .description
            .visible
    );
}
