use super::*;
fn tab(
    engine: &mut EngineInstance,
    parent_callback: u64,
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
            0, 0x12400, 0, 0x50000000, 0, 0, 80, 40, parent, 92, 0x400000, 0,
        ],
    );
    callbacks(engine, parent_callback, common, procedure);
    let window = eax(engine);
    assert_ne!(window, 0);
    window
}
fn put_item(engine: &mut EngineInstance, words: [u32; 7]) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12500),
            &words
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
}
#[test]
fn captured_tab_named_class_items_preserve_owned_text_data_and_real_font_geometry() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = tab(&mut engine, callback, common, procedure, parent);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x12600), b"first\0")
        .unwrap();
    put_item(&mut engine, [0xb, 0, 0, 0x12600, 0, 2, 0xaabbccdd]);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x1307, 0, 0x12500]
        ),
        0
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x12600), b"second\0")
        .unwrap();
    put_item(&mut engine, [0xb, 0, 0, 0x12600, 0, u32::MAX, 0x11223344]);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x1307, 1, 0x12500]
        ),
        1
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x1304, 0, 0]
        ),
        2
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x130b, 0, 0]
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
            [0x130c, 1, 0]
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
            [0x130b, 0, 0]
        ),
        1
    );
    put_item(&mut engine, [0xb, 0, 0, 0x12700, 16, 0, 0]);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x133c, 0, 0x12500]
        ),
        1
    );
    assert_eq!(
        words::<7>(&engine, 0x12500),
        [0xb, 0, 0, 0x12700, 16, 2, 0xaabbccdd]
    );
    assert_eq!(
        words::<3>(&engine, 0x12700),
        [0x00690066, 0x00730072, 0x00000074]
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x130a, 0, 0x12800]
        ),
        1
    );
    let [left, top, right, bottom] = words::<4>(&engine, 0x12800);
    assert!(right > left + 12);
    assert!(bottom > top + 6);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x1308, 0, 0]
        ),
        1
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x130b, 0, 0]
        ),
        0
    );
}
#[test]
fn captured_tab_getitem_guarded_text_output_does_not_publish_updated_structure() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = tab(&mut engine, callback, common, procedure, parent);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x12600), b"first\0")
        .unwrap();
    put_item(&mut engine, [0xb, 0, 0, 0x12600, 0, 2, 0xaabbccdd]);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x1307, 0, 0x12500]
        ),
        0
    );
    let before = [0x1b, 0xdeadbeef, u32::MAX, 0x14000, 16, 0x1234, 0x5678];
    put_item(&mut engine, before);
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[window, 0x133c, 0, 0x12500],
    );
    let outer = engine.pending_call.as_ref().unwrap().frame;
    let record = engine
        .begin_windows_resident_callback(KEY, common, COMMON_RETURN, RETURN_ID)
        .unwrap();
    engine
        .authorize_resident_callback(KEY, common, record.token)
        .unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: COMMON_PROC_W },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = engine
        .capture_resident_callback_call(KEY, common, record.token, CallingConvention32::Stdcall, 4)
        .unwrap()
        .token;
    let frame = engine.pending_call.as_ref().unwrap().frame;
    let arena = engine.arena().to_vec();
    let version = engine.memory().unwrap().transaction_version();
    assert!(matches!(
        engine.complete_active_resident_callback_windows_call(KEY, common, record.token, token),
        Err(HostError::User32(
            crate::windows::user32::User32Error32::Memory(crate::memory::MemoryError::Fault(
                crate::memory::MemoryFault {
                    address: GuestAddress(0x14000),
                    access: crate::memory::Access::Write,
                    reason: crate::memory::FaultReason::Unmapped
                }
            ))
        ))
    ));
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    assert_eq!(engine.pending_call.as_ref().unwrap().frame, frame);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.arena(), arena);
    assert_eq!(words::<7>(&engine, 0x12500), before);
    engine.map(0x14000, 1, 3).unwrap();
    engine
        .complete_active_resident_callback_windows_call(KEY, common, record.token, token)
        .unwrap();
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    assert_eq!(state.registers[0], 1);
    assert_eq!(state.registers[4], outer.state().registers[4]);
    state.eip = COMMON_RETURN;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: RETURN_ID },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    engine
        .finish_windows_resident_callback(KEY, common, record.token)
        .unwrap();
    assert!(engine.pending_call.is_none());
    assert_eq!(eax(&engine), 1);
    assert_eq!(
        words::<7>(&engine, 0x12500),
        [0x1b, 0, u32::MAX, 0x14000, 16, 2, 0xaabbccdd]
    );
    assert_eq!(
        words::<3>(&engine, 0x14000),
        [0x00690066, 0x00730072, 0x00000074]
    );
}

fn input(
    engine: &mut EngineInstance,
    callback: u64,
    common: u64,
    procedure: u32,
    window: u32,
    message: [u32; 3],
    cancel: bool,
) -> Vec<[u32; 3]> {
    capture(
        engine,
        User32Api32::SendMessageW.id(),
        &[window, message[0], message[1], message[2]],
    );
    let mut notes = Vec::new();
    for _ in 0..64 {
        if engine.pending_windows_callback().is_some() {
            callback_returning(engine, callback, common, procedure, &mut |engine, args| {
                if args[1] != 0x4e {
                    return None;
                }
                let note = words::<3>(engine, args[3]);
                assert_eq!(args[2], 92);
                notes.push(note);
                Some(u32::from(cancel && note[2] == (-552i32) as u32))
            });
        } else if let Some(wait) = engine.waiting_user32_message() {
            engine.resume_user32_wait(KEY, wait).unwrap();
        } else {
            break;
        }
    }
    assert!(engine.pending_call.is_none());
    assert_eq!(eax(engine), 0);
    notes
}
#[test]
fn captured_tab_mouse_keyboard_and_focus_messages_send_real_cancellable_parent_notifications() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = tab(&mut engine, callback, common, procedure, parent);
    for (index, text) in [(0, b"first\0".as_slice()), (1, b"second\0".as_slice())] {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0x12600), text)
            .unwrap();
        put_item(&mut engine, [9, 0, 0, 0x12600, 0, 0, index + 10]);
        assert_eq!(
            send(
                &mut engine,
                callback,
                common,
                procedure,
                window,
                [0x1307, index, 0x12500]
            ),
            index
        );
    }
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x130a, 1, 0x12800]
        ),
        1
    );
    let [left, top, _, _] = words::<4>(&engine, 0x12800);
    let point = (left + 1) | ((top + 1) << 16);
    assert_eq!(
        input(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x201, 1, point],
            false
        ),
        [
            [window, 92, (-552i32) as u32],
            [window, 92, (-551i32) as u32]
        ]
    );
    assert_eq!(
        engine.user32_windows().unwrap().focus(),
        Some(WindowHandle32::try_from(window).unwrap())
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x130b, 0, 0]
        ),
        1
    );
    assert_eq!(
        input(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x100, 37, 0],
            true
        ),
        [[window, 92, (-552i32) as u32]]
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x130b, 0, 0]
        ),
        1
    );
    assert_eq!(
        input(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x100, 37, 0],
            false
        ),
        [
            [window, 92, (-552i32) as u32],
            [window, 92, (-551i32) as u32]
        ]
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x130b, 0, 0]
        ),
        0
    );
    assert_eq!(
        input(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x1330, 1, 0],
            false
        ),
        [
            [window, 92, (-552i32) as u32],
            [window, 92, (-551i32) as u32]
        ]
    );
    assert!(
        input(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x201, 1, point],
            false
        )
        .is_empty()
    );
}
