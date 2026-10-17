use super::*;
fn header(
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
            &"SysHeader32\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    capture(
        engine,
        User32Api32::CreateWindowExW.id(),
        &[
            0, 0x12400, 0, 0x50000002, 0, 0, 96, 24, parent, 93, 0x400000, 0,
        ],
    );
    callbacks(engine, callback, common, procedure);
    let handle = eax(engine);
    assert_ne!(handle, 0);
    handle
}
fn put(engine: &mut EngineInstance, values: &[u32]) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12500),
            &values
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
}
fn insert(
    engine: &mut EngineInstance,
    callback: u64,
    common: u64,
    procedure: u32,
    window: u32,
    index: u32,
    width: u32,
    text: &str,
) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12600),
            &text
                .encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    put(engine, &[15, width, 0x12600, 0, 0, 0x4000, index + 10]);
    assert_eq!(
        send(
            engine,
            callback,
            common,
            procedure,
            window,
            [0x120a, index, 0x12500]
        ),
        index
    );
}
fn notifying(
    engine: &mut EngineInstance,
    callback: u64,
    common: u64,
    procedure: u32,
    window: u32,
    message: [u32; 3],
    cancel: i32,
) -> Vec<[u32; 6]> {
    capture(
        engine,
        User32Api32::SendMessageW.id(),
        &[window, message[0], message[1], message[2]],
    );
    let mut notes = Vec::new();
    for _ in 0..128 {
        if engine.pending_windows_callback().is_some() {
            callback_returning(engine, callback, common, procedure, &mut |engine, args| {
                if args[1] != 0x4e {
                    return None;
                }
                assert_eq!(args[2], 93);
                let note = words::<6>(engine, args[3]);
                assert_eq!(note[0], window);
                notes.push(note);
                Some(u32::from(note[2] == cancel as u32))
            });
        } else if let Some(wait) = engine.waiting_user32_message() {
            engine.resume_user32_wait(KEY, wait).unwrap();
        } else {
            break;
        }
    }
    assert!(engine.pending_call.is_none());
    notes
}
#[test]
fn captured_header_items_layout_order_and_font_pixels_use_the_named_canonical_class() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = header(&mut engine, callback, common, procedure, parent);
    insert(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        0,
        40,
        "first",
    );
    insert(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        1,
        45,
        "second",
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x1200, 0, 0]
        ),
        2
    );
    put(&mut engine, &[15, 0, 0x12700, 0, 16, 0, 0]);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x120b, 0, 0x12500]
        ),
        1
    );
    assert_eq!(
        words::<7>(&engine, 0x12500),
        [15, 40, 0x12700, 0, 16, 0x4000, 10]
    );
    assert_eq!(
        words::<3>(&engine, 0x12700),
        [0x00690066, 0x00730072, 0x00000074]
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12700),
            &[1u32, 0]
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
            window,
            [0x1212, 2, 0x12700]
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
            [0x120f, 0, 0]
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
            [0x1207, 0, 0x12800]
        ),
        1
    );
    assert_eq!(words::<4>(&engine, 0x12800), [45, 0, 85, 24]);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12700),
            &[0u32, 0, 96, 80]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    put(&mut engine, &[0x12700, 0x12800]);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x1205, 0, 0x12500]
        ),
        1
    );
    let position = words::<7>(&engine, 0x12800);
    assert_eq!(position[0], window);
    assert_eq!(
        [position[2], position[3], position[4], position[6]],
        [0, 0, 96, 0x14]
    );
    assert!(position[5] > 4);
    assert_eq!(words::<4>(&engine, 0x12700), [0, position[5], 96, 80]);
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0xf, 0, 0],
    );
    let frame = (0..16)
        .find_map(|index| {
            engine
                .pending_gdi_frame(index)
                .unwrap()
                .filter(|frame| frame.window == window)
        })
        .expect("header publishes actual pixels");
    let mut pixels = Vec::new();
    for offset in (0..frame.bytes).step_by(4096) {
        pixels.extend_from_slice(engine.read_gdi_frame(frame.id, frame.revision, offset, (frame.bytes - offset).min(4096)).unwrap());
    }
    let background = [0xc0, 0xc0, 0xc0, 0];
    assert_eq!((frame.width, frame.height), (96, 24));
    assert!(
        (3..21).any(|y| (5..35).any(|x| {
            let offset = (y * 96 + x) * 4;
            pixels[offset..offset + 3] != background[..3]
        })),
        "header interior text has real ink independently of its cell frame"
    );
    assert_eq!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .gdi_update_region(WindowHandle32::try_from(window).unwrap())
            .unwrap()
            .0
            .complexity(),
        1
    );
}
#[test]
fn captured_header_change_cancellation_and_mouse_tracking_use_exact_parent_notifications() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = header(&mut engine, callback, common, procedure, parent);
    insert(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        0,
        40,
        "first",
    );
    put(&mut engine, &[1, 55]);
    let notes = notifying(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x120c, 0, 0x12500],
        -320,
    );
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0][2], (-320i32) as u32);
    assert_eq!(eax(&engine), 0);
    put(&mut engine, &[1, 0]);
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x120b, 0, 0x12500],
    );
    assert_eq!(words::<2>(&engine, 0x12500), [1, 40]);
    put(&mut engine, &[1, 55]);
    let notes = notifying(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x120c, 0, 0x12500],
        0,
    );
    assert_eq!(
        notes.iter().map(|note| note[2]).collect::<Vec<_>>(),
        [(-320i32) as u32, (-321i32) as u32]
    );
    assert_eq!(eax(&engine), 1);
    let down = 55 | (3 << 16);
    let notes = notifying(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x201, 1, down],
        -326,
    );
    assert_eq!(notes[0][2], (-326i32) as u32);
    assert_eq!(engine.user32_windows().unwrap().capture(), None);
    notifying(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x201, 1, down],
        0,
    );
    assert_eq!(
        engine.user32_windows().unwrap().capture(),
        Some(WindowHandle32::try_from(window).unwrap())
    );
    let notes = notifying(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x200, 1, 65 | (3 << 16)],
        0,
    );
    assert_eq!(notes[0][2], (-328i32) as u32);
    let notes = notifying(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x202, 0, 65 | (3 << 16)],
        0,
    );
    assert_eq!(
        notes.iter().map(|note| note[2]).collect::<Vec<_>>(),
        [(-327i32) as u32, (-320i32) as u32, (-321i32) as u32]
    );
    assert_eq!(engine.user32_windows().unwrap().capture(), None);
    put(&mut engine, &[1, 0]);
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x120b, 0, 0x12500],
    );
    assert_eq!(words::<2>(&engine, 0x12500), [1, 65]);
    assert!(
        notifying(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x201, 1, 8 | (3 << 16)],
            0
        )
        .is_empty()
    );
    let notes = notifying(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x202, 0, 8 | (3 << 16)],
        0,
    );
    assert_eq!(notes[0][2], (-322i32) as u32);
    assert_eq!(notes[0][5], 0, "HDN_ITEMCLICK has no HDITEM payload");
    assert_eq!(engine.user32_windows().unwrap().capture(), None);
}

#[test]
fn captured_header_getitem_fault_preserves_all_outputs_and_retries_the_same_owned_frame() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = header(&mut engine, callback, common, procedure, parent);
    insert(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        0,
        40,
        "first",
    );
    let before = [15, 0xdeadbeef, 0x14000, 0, 16, 0x1234, 0x5678];
    put(&mut engine, &before);
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[window, 0x120b, 0, 0x12500],
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
        [15, 40, 0x14000, 0, 16, 0x4000, 10]
    );
    assert_eq!(
        words::<3>(&engine, 0x14000),
        [0x00690066, 0x00730072, 0x00000074]
    );
}
