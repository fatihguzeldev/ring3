use super::*;

fn tooltip(engine: &mut EngineInstance, parent: u64, common: u64, procedure: u32) -> u32 {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12400),
            &"tooltips_class32\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    capture(
        engine,
        User32Api32::CreateWindowExW.id(),
        &[8, 0x12400, 0, 0x80000002, 0, 0, 0, 0, 0, 0, 0x400000, 0],
    );
    callbacks(engine, parent, common, procedure);
    let window = eax(engine);
    assert_ne!(window, 0);
    assert!(
        !engine
            .user32_windows()
            .unwrap()
            .get(WindowHandle32::try_from(window).unwrap())
            .unwrap()
            .description
            .visible
    );
    window
}
fn put_info(engine: &mut EngineInstance, owner: u32, text: u32) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12500),
            &[44u32, 0xa0, owner, 77, 0, 0, 8, 6, 0, text, 0xaabbccdd]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
}
fn drain_observing(
    engine: &mut EngineInstance,
    parent: u64,
    common: u64,
    procedure: u32,
    notifications: &mut Vec<(u32, u32, [u32; 3])>,
) {
    for _ in 0..128 {
        if engine.pending_windows_callback().is_some() {
            callback_returning(engine, parent, common, procedure, &mut |engine, args| {
                if args[1] == 0x4e {
                    notifications.push((args[0], args[2], words::<3>(engine, args[3])));
                }
                None
            });
        } else if let Some(wait) = engine.waiting_user32_message() {
            engine.resume_user32_wait(KEY, wait).unwrap();
        } else {
            assert!(engine.pending_call.is_none());
            return;
        }
    }
    panic!("tooltip must finish through its bounded real window and notify callbacks");
}
#[test]
fn captured_tooltip_static_tracking_emits_real_notifications_and_paints_owned_text() {
    let (mut engine, parent_callback, common, procedure, _, parent) = fixture();
    let window = tooltip(&mut engine, parent_callback, common, procedure);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x12600), b"tracking text\0")
        .unwrap();
    put_info(&mut engine, parent, 0x12600);
    assert_eq!(
        send(
            &mut engine,
            parent_callback,
            common,
            procedure,
            window,
            [0x404, 0, 0x12500]
        ),
        1
    );
    assert_eq!(
        send(
            &mut engine,
            parent_callback,
            common,
            procedure,
            window,
            [0x404, 0, 0x12500]
        ),
        0
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x12600), b"changed bytes\0")
        .unwrap();
    put_info(&mut engine, parent, 0x12700);
    assert_eq!(
        send(
            &mut engine,
            parent_callback,
            common,
            procedure,
            window,
            [0x435, 0, 0x12500]
        ),
        1
    );
    let expected: Vec<u8> = "tracking text\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut output = vec![0; expected.len()];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x12700), &mut output)
        .unwrap();
    assert_eq!(output, expected);
    assert_eq!(
        words::<11>(&engine, 0x12500),
        [44, 0xa0, parent, 77, 0, 0, 8, 6, 0, 0x12700, 0xaabbccdd]
    );
    send(
        &mut engine,
        parent_callback,
        common,
        procedure,
        window,
        [0x412, 0, 20 | (30 << 16)],
    );
    let mut notifications = Vec::new();
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[window, 0x411, 1, 0x12500],
    );
    drain_observing(
        &mut engine,
        parent_callback,
        common,
        procedure,
        &mut notifications,
    );
    assert_eq!(
        notifications,
        [(parent, 77, [window, 77, (-521i32) as u32])]
    );
    let record = engine
        .user32_windows()
        .unwrap()
        .get(WindowHandle32::try_from(window).unwrap())
        .unwrap();
    assert!(record.description.visible);
    assert!(record.description.extent.width > 30);
    assert!(record.description.extent.height > 8);
    capture(
        &mut engine,
        User32Api32::GetWindowRect.id(),
        &[window, 0x12800],
    );
    let [left, top, right, bottom] = words::<4>(&engine, 0x12800);
    assert_eq!((left, top), (20, 30));
    assert!(right > left + 30 && bottom > top + 8);
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
    assert!(
        pixels
            .chunks_exact(4)
            .any(|pixel| pixel[0] != pixel[1] || pixel[1] != pixel[2])
    );
    let background = engine.gdi.as_ref().unwrap().system_color(24);
    let background = [
        (background >> 16) as u8,
        (background >> 8) as u8,
        background as u8,
        255,
    ];
    assert!(pixels.chunks_exact(4).any(|pixel| pixel != background));
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[window, 0x412, 0, 40 | (50 << 16)],
    );
    drain_observing(
        &mut engine,
        parent_callback,
        common,
        procedure,
        &mut notifications,
    );
    assert_eq!(
        notifications.len(),
        1,
        "position updates must not replay TTN_SHOW"
    );
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[window, 0x411, 0, 0x12500],
    );
    drain_observing(
        &mut engine,
        parent_callback,
        common,
        procedure,
        &mut notifications,
    );
    assert_eq!(
        notifications,
        [
            (parent, 77, [window, 77, (-521i32) as u32]),
            (parent, 77, [window, 77, (-522i32) as u32])
        ]
    );
    assert!(
        !engine
            .user32_windows()
            .unwrap()
            .get(WindowHandle32::try_from(window).unwrap())
            .unwrap()
            .description
            .visible
    );
    assert_eq!(
        send(
            &mut engine,
            parent_callback,
            common,
            procedure,
            window,
            [0x433, 0, 0x12500]
        ),
        0
    );
    assert_eq!(
        send(
            &mut engine,
            parent_callback,
            common,
            procedure,
            window,
            [0x40d, 0, 0]
        ),
        0
    );
}
#[test]
fn captured_tooltip_getinfo_unmapped_text_preserves_header_and_retry_token() {
    let (mut engine, parent_callback, common, procedure, _, parent) = fixture();
    let window = tooltip(&mut engine, parent_callback, common, procedure);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x12600), b"owned\0")
        .unwrap();
    put_info(&mut engine, parent, 0x12600);
    assert_eq!(
        send(
            &mut engine,
            parent_callback,
            common,
            procedure,
            window,
            [0x404, 0, 0x12500]
        ),
        1
    );
    put_info(&mut engine, parent, 0x14000);
    let before = words::<11>(&engine, 0x12500);
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[window, 0x435, 0, 0x12500],
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
    assert_eq!(words::<11>(&engine, 0x12500), before);
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
    assert_eq!(
        words::<3>(&engine, 0x14000),
        [0x0077006f, 0x0065006e, 0x00000064]
    );
}

#[path = "tooltip/automatic.rs"]
mod automatic;

#[path = "tooltip/tab.rs"]
mod tab;
