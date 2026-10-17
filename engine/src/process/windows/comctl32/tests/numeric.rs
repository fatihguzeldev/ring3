use super::*;
fn create(
    engine: &mut EngineInstance,
    callback: u64,
    common: u64,
    procedure: u32,
    parent: u32,
    name: &str,
    style: u32,
    width: u32,
    height: u32,
    id: u32,
) -> u32 {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12400),
            &name
                .encode_utf16()
                .chain([0])
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
            width,
            height,
            parent,
            id,
            0x400000,
            0,
        ],
    );
    callbacks(engine, callback, common, procedure);
    let handle = eax(engine);
    assert_ne!(handle, 0);
    handle
}
fn put(engine: &mut EngineInstance, pointer: u32, values: &[u32]) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(pointer),
            &values
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
}
fn event(
    engine: &mut EngineInstance,
    callback: u64,
    common: u64,
    procedure: u32,
    window: u32,
    message: [u32; 3],
    cancel: bool,
    buddy: bool,
) -> Vec<[u32; 4]> {
    capture(
        engine,
        User32Api32::SendMessageW.id(),
        &[window, message[0], message[1], message[2]],
    );
    let mut notes = Vec::new();
    for _ in 0..128 {
        if engine.pending_windows_callback().is_some() {
            callback_returning(engine, callback, common, procedure, &mut |engine, args| {
                if buddy && args[1] == 0xc {
                    let mut bytes = Vec::new();
                    for index in 0..63 {
                        let mut word = [0; 2];
                        engine
                            .memory()
                            .unwrap()
                            .read(GuestAddress(args[3] + index * 2), &mut word)
                            .unwrap();
                        bytes.extend_from_slice(&word);
                        if word == [0, 0] {
                            break;
                        }
                    }
                    engine
                        .memory
                        .as_mut()
                        .unwrap()
                        .write(GuestAddress(0x12800), &bytes)
                        .unwrap();
                    return Some(1);
                }
                if buddy && args[1] == 0xd {
                    let mut bytes = Vec::new();
                    for index in 0..args[2].saturating_sub(1).min(63) {
                        let mut word = [0; 2];
                        engine
                            .memory()
                            .unwrap()
                            .read(GuestAddress(0x12800 + index * 2), &mut word)
                            .unwrap();
                        if word == [0, 0] {
                            break;
                        }
                        bytes.extend_from_slice(&word);
                    }
                    let count = bytes.len() / 2;
                    bytes.extend_from_slice(&[0, 0]);
                    engine
                        .memory
                        .as_mut()
                        .unwrap()
                        .write(GuestAddress(args[3]), &bytes)
                        .unwrap();
                    return Some(count as u32);
                }
                if args[1] == 0x4e {
                    let note = words::<5>(engine, args[3]);
                    assert_eq!(note[0], window);
                    assert_eq!(note[2], (-722i32) as u32);
                    notes.push([args[1], note[3], note[4], window]);
                    return Some(u32::from(cancel));
                }
                if matches!(args[1], 0x114 | 0x115) {
                    notes.push([args[1], args[2], args[3], 0]);
                    return Some(0);
                }
                None
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
fn pixels(engine: &EngineInstance, window: u32) -> Vec<u8> {
    let frame = (0..16)
        .find_map(|index| {
            engine
                .pending_gdi_frame(index)
                .unwrap()
                .filter(|frame| frame.window == window)
        })
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
    pixels
}
#[test]
fn captured_trackbar_ranges_ticks_geometry_and_pixels_are_real_bounded_state() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = create(
        &mut engine,
        callback,
        common,
        procedure,
        parent,
        "msctls_trackbar32",
        1 | 0x20,
        96,
        24,
        94,
    );
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x406, 1, u32::from((-20i16) as u16) | (80 << 16)],
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x401, 0, 0]
        ),
        (-20i32) as u32
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
        80
    );
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x414, 25, 0],
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x410, 0, 0]
        ),
        5
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x403, 1, 0]
        ),
        30
    );
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x405, 1, 30],
    );
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x40a, 1, 10 | (60 << 16)],
    );
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x419, 0, 0x12500],
    );
    let thumb = words::<4>(&engine, 0x12500);
    assert!(thumb[0] < thumb[2] && thumb[1] < thumb[3]);
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x41a, 0, 0x12520],
    );
    let channel = words::<4>(&engine, 0x12520);
    assert!(channel[0] < channel[2]);
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0xf, 0, 0],
    );
    let pixels = pixels(&engine, window);
    assert!(
        pixels
            .chunks_exact(4)
            .any(|pixel| pixel[..3] != [0xc0, 0xc0, 0xc0]),
        "channel, ticks and thumb publish actual pixels"
    );
}
#[test]
fn captured_trackbar_keyboard_and_thumb_send_real_scroll_and_capture_messages() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = create(
        &mut engine,
        callback,
        common,
        procedure,
        parent,
        "msctls_trackbar32",
        0,
        96,
        24,
        94,
    );
    assert_eq!(
        event(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x100, 39, 0],
            false,
            false
        ),
        [[0x114, 1, window, 0]]
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x400, 0, 0]
        ),
        1
    );
    assert_eq!(
        event(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x101, 39, 0],
            false,
            false
        ),
        [[0x114, 8, window, 0]]
    );
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x419, 0, 0x12500],
    );
    let thumb = words::<4>(&engine, 0x12500);
    let point = (thumb[0] + thumb[2]) / 2 | (((thumb[1] + thumb[3]) / 2) << 16);
    let notes = event(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x201, 1, point],
        false,
        false,
    );
    assert_eq!(notes[0][1] & 0xffff, 5);
    assert_eq!(
        engine.user32_windows().unwrap().capture(),
        Some(WindowHandle32::try_from(window).unwrap())
    );
    event(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x200, 1, 80 | (12 << 16)],
        false,
        false,
    );
    let position = send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x400, 0, 0],
    );
    assert!(position > 1);
    let notes = event(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x202, 0, 80 | (12 << 16)],
        false,
        false,
    );
    assert_eq!(
        notes
            .iter()
            .map(|note| note[1] & 0xffff)
            .collect::<Vec<_>>(),
        [4, 8]
    );
    assert_eq!(engine.user32_windows().unwrap().capture(), None);
}
#[test]
fn captured_updown_cancellable_delta_buddy_text_wrap_and_real_arrow_pixels() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = create(
        &mut engine,
        callback,
        common,
        procedure,
        parent,
        "msctls_updown32",
        1 | 2 | 0x20,
        20,
        40,
        95,
    );
    // the parent is a real application window implementing the buddy's text messages.
    put(&mut engine, 0x12800, &[u32::from(b'0'), 0]);
    event(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x469, parent, 0],
        false,
        true,
    );
    assert_eq!(eax(&engine), 0);
    event(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x46f, 0, 10],
        false,
        true,
    );
    event(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x471, 0, 9],
        false,
        true,
    );
    let notes = event(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x100, 38, 0],
        true,
        true,
    );
    assert_eq!(notes, [[0x4e, 9, 1, window]]);
    event(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x472, 0, 0x12700],
        false,
        true,
    );
    assert_eq!(eax(&engine), 9);
    assert_eq!(words::<1>(&engine, 0x12700), [0]);
    let notes = event(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x100, 38, 0],
        false,
        true,
    );
    assert_eq!(
        notes,
        [[0x4e, 9, 1, window], [0x115, 4 | (10 << 16), window, 0]]
    );
    event(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x100, 38, 0],
        false,
        true,
    );
    event(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x472, 0, 0x12700],
        false,
        true,
    );
    assert_eq!(eax(&engine), 0);
    assert_eq!(words::<1>(&engine, 0x12800)[0] & 0xffff, u32::from(b'0'));
    event(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x201, 1, 4 | (3 << 16)],
        false,
        true,
    );
    assert_eq!(
        engine.user32_windows().unwrap().capture(),
        Some(WindowHandle32::try_from(window).unwrap())
    );
    event(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x202, 0, 4 | (3 << 16)],
        false,
        true,
    );
    assert_eq!(engine.user32_windows().unwrap().capture(), None);
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0xf, 0, 0],
    );
    let pixels = pixels(&engine, window);
    assert!(
        pixels.chunks_exact(4).any(|pixel| pixel[..3] == [0, 0, 0]),
        "the two spin arrows have actual ink"
    );
}
#[test]
fn captured_updown_range_output_fault_is_atomic_and_invalid_acceleration_preserves_live_values() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = create(
        &mut engine,
        callback,
        common,
        procedure,
        parent,
        "msctls_updown32",
        0x20,
        20,
        40,
        95,
    );
    event(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x46f, (-500i32) as u32, 700],
        false,
        false,
    );
    put(&mut engine, 0x12500, &[2, 3, 1, 4]);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x46b, 2, 0x12500]
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
            [0x46c, 0, 0]
        ),
        1
    );
    put(&mut engine, 0x12520, &[0xdeadbeef]);
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[window, 0x470, 0x12520, 0x14000],
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
        Err(HostError::Memory(crate::memory::MemoryError::Fault(
            crate::memory::MemoryFault {
                address: GuestAddress(0x14000),
                access: crate::memory::Access::Write,
                reason: crate::memory::FaultReason::Unmapped
            }
        )))
    ));
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    assert_eq!(engine.pending_call.as_ref().unwrap().frame, frame);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.arena(), arena);
    assert_eq!(words::<1>(&engine, 0x12520), [0xdeadbeef]);
    engine.map(0x14000, 1, 3).unwrap();
    engine
        .complete_active_resident_callback_windows_call(KEY, common, record.token, token)
        .unwrap();
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    assert_eq!(state.registers[0], 0);
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
    assert_eq!(eax(&engine), 0);
    assert_eq!(words::<1>(&engine, 0x12520), [(-500i32) as u32]);
    assert_eq!(words::<1>(&engine, 0x14000), [700]);
}
