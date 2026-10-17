use super::*;
fn tree(
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
            &"SysTreeView32\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    capture(
        engine,
        User32Api32::CreateWindowExW.id(),
        &[
            0, 0x12400, 0, 0x50000027, 0, 0, 160, 96, parent, 95, 0x400000, 0,
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
                .flat_map(|word| word.to_le_bytes())
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
    parent: u32,
    name: &str,
    data: u32,
) -> u32 {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12600),
            &name
                .encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    put(
        engine,
        &[parent, 0xffff0002, 5, 0, 0, 0, 0x12600, 0, 0, 0, 0, data],
    );
    let id = send(
        engine,
        callback,
        common,
        procedure,
        window,
        [0x1132, 0, 0x12500],
    );
    assert_ne!(id, 0);
    id
}
fn drain(
    engine: &mut EngineInstance,
    callback: u64,
    common: u64,
    procedure: u32,
    veto: bool,
) -> Vec<[u32; 26]> {
    let mut notes = Vec::new();
    for _ in 0..256 {
        if engine.pending_windows_callback().is_some() {
            callback_returning(engine, callback, common, procedure, &mut |engine, args| {
                if args[1] == 0x4e {
                    let payload = words::<26>(engine, args[3]);
                    notes.push(payload);
                    if veto && payload[2] == (-450i32) as u32 {
                        return Some(1);
                    }
                }
                None
            });
        } else if let Some(wait) = engine.waiting_user32_message() {
            engine.resume_user32_wait(KEY, wait).unwrap();
        } else {
            assert!(engine.pending_call.is_none());
            return notes;
        }
    }
    panic!("tree operation must finish its bounded callback chain");
}
#[test]
fn captured_tree_hierarchy_selection_veto_expansion_and_recursive_deletion_use_actual_notifications()
 {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = tree(&mut engine, callback, common, procedure, parent);
    let root = insert(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        0xffff0000,
        "root",
        0xaabbccdd,
    );
    let child = insert(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        root,
        "child",
        0x12345678,
    );
    let other = insert(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        0xffff0000,
        "other",
        0xdeadbeef,
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x110a, 4, root]
        ),
        child
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x110a, 3, child]
        ),
        root
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x110a, 1, root]
        ),
        other
    );
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[window, 0x1102, 2, root],
    );
    let notes = drain(&mut engine, callback, common, procedure, false);
    assert_eq!(
        notes.iter().map(|p| p[2]).collect::<Vec<_>>(),
        [(-454i32) as u32, (-455i32) as u32]
    );
    assert!(
        notes
            .iter()
            .all(|p| p[..4] == [window, 95, p[2], 2] && p[15] == root)
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x110a, 6, root]
        ),
        child
    );
    for (action, result) in [(2, 1), (1, 1), (1, 0), (2, 1)] {
        capture(
            &mut engine,
            User32Api32::SendMessageW.id(),
            &[window, 0x1102, action, root],
        );
        assert!(
            drain(&mut engine, callback, common, procedure, false).is_empty(),
            "subsequent explicit TVM_EXPAND must preserve EXPANDEDONCE notification suppression"
        );
        assert_eq!(eax(&engine), result);
    }
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x1102, 2, other]
        ),
        0,
        "a leaf has no children to expand"
    );
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[window, 0x110b, 9, child],
    );
    let notes = drain(&mut engine, callback, common, procedure, true);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0][15], child);
    assert_eq!(notes[0][23], 0x12345678);
    assert_eq!(eax(&engine), 0);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x110a, 9, 0]
        ),
        0
    );
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[window, 0x110b, 9, child],
    );
    let notes = drain(&mut engine, callback, common, procedure, false);
    assert_eq!(
        notes.iter().map(|p| p[2]).collect::<Vec<_>>(),
        [(-450i32) as u32, (-451i32) as u32]
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x110a, 9, 0]
        ),
        child
    );
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[window, 0x1101, 0, root],
    );
    let notes = drain(&mut engine, callback, common, procedure, false);
    assert_eq!(
        notes
            .iter()
            .map(|p| (p[2], p[5], p[13]))
            .collect::<Vec<_>>(),
        [
            ((-458i32) as u32, child, 0x12345678),
            ((-458i32) as u32, root, 0xaabbccdd)
        ]
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x1105, 0, 0]
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
            [0x110a, 0, 0]
        ),
        other
    );
    let replacement = insert(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        0xffff0000,
        "replacement",
        7,
    );
    assert!(replacement > other);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x110b, 9, root]
        ),
        0
    );
}
#[test]
fn captured_tree_owned_text_keyboard_mouse_and_real_pixels_share_the_same_hierarchy() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = tree(&mut engine, callback, common, procedure, parent);
    let root = insert(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        0,
        "alpha",
        11,
    );
    let child = insert(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        root,
        "beta",
        22,
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x12600), b"scribble")
        .unwrap();
    put(&mut engine, &[5, root, 0, 0, 0x12700, 4, 0, 0, 0, 0]);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x113e, 0, 0x12500]
        ),
        1
    );
    assert_eq!(words::<2>(&engine, 0x12700), [0x006c0061, 0x00000070]);
    assert_eq!(words::<10>(&engine, 0x12500)[9], 11);
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x110b, 9, root],
    );
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x100, 0x27, 0],
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x1127, root, 0x20]
        ),
        0x20
    );
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[window, 0x100, 0x28, 0],
    );
    let notes = drain(&mut engine, callback, common, procedure, false);
    assert_eq!(notes[0][3], 2);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x110a, 9, 0]
        ),
        child
    );
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x100, 0x28, 0],
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x110a, 9, 0]
        ),
        child,
        "Down at the last visible item must not wrap"
    );
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[window, 0x201, 0, 44 | (2 << 16)],
    );
    let notes = drain(&mut engine, callback, common, procedure, false);
    assert_eq!(notes[0][3], 1);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x110a, 9, 0]
        ),
        root
    );
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x0f, 0, 0],
    );
    let frame = (0..16)
        .find_map(|index| {
            engine
                .pending_gdi_frame(index)
                .unwrap()
                .filter(|frame| frame.window == window)
        })
        .expect("TreeView publishes actual glyphs and expansion buttons");
    assert_eq!((frame.width, frame.height), (160, 96));
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
            .any(|pixel| pixel[..3] != [255, 255, 255])
    );
    capture(
        &mut engine,
        crate::windows::gdi::GdiApi32::GetDC.id(),
        &[window],
    );
    let dc = eax(&engine);
    assert_ne!(dc, 0);
    capture(
        &mut engine,
        crate::windows::gdi::GdiApi32::SetViewportOrgEx.id(),
        &[dc, 11, 7, 0],
    );
    assert_eq!(eax(&engine), 1);
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x318, dc, 0],
    );
    let translated = (0..16)
        .find_map(|index| {
            engine
                .pending_gdi_frame(index)
                .unwrap()
                .filter(|frame| frame.window == window)
        })
        .unwrap();
    let mut after = Vec::new();
    for offset in (0..translated.bytes).step_by(4096) {
        after.extend_from_slice(
            engine
                .read_gdi_frame(
                    translated.id,
                    translated.revision,
                    offset,
                    (translated.bytes - offset).min(4096),
                )
                .unwrap(),
        );
    }
    for y in 7usize..96 {
        for x in 11usize..160 {
            let actual = (y * 160 + x) * 4;
            let original = ((y - 7) * 160 + x - 11) * 4;
            assert_eq!(
                &after[actual..actual + 4],
                &pixels[original..original + 4],
                "borrowed DC mapping at ({x},{y}) must transform every pixel once"
            );
        }
    }
    capture(
        &mut engine,
        crate::windows::gdi::GdiApi32::ReleaseDC.id(),
        &[window, dc],
    );
    assert_eq!(eax(&engine), 1);
}
#[test]
fn captured_tree_getitem_bad_text_span_is_neutral_and_retries_exact_call() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = tree(&mut engine, callback, common, procedure, parent);
    let root = insert(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        0,
        "alpha",
        0xabcdef01,
    );
    let before = [5, root, 0, 0, 0x14000, 16, 0, 0, 0, 0xdeadbeef];
    put(&mut engine, &before);
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[window, 0x113e, 0, 0x12500],
    );
    let return_esp = engine
        .pending_call
        .as_ref()
        .unwrap()
        .frame
        .state()
        .registers[4];
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
    let pending = engine.pending_call.as_ref().unwrap().frame;
    let version = engine.memory().unwrap().transaction_version();
    let arena = engine.arena().to_vec();
    assert!(matches!(
        engine.complete_active_resident_callback_windows_call(KEY, common, record.token, token),
        Err(HostError::User32(
            crate::windows::user32::User32Error32::Memory(_)
        ))
    ));
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    assert_eq!(engine.pending_call.as_ref().unwrap().frame, pending);
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(words::<10>(&engine, 0x12500), before);
    engine.map(0x14000, 1, 3).unwrap();
    engine
        .complete_active_resident_callback_windows_call(KEY, common, record.token, token)
        .unwrap();
    assert_eq!(words::<10>(&engine, 0x12500)[9], 0xabcdef01);
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = COMMON_RETURN;
    state.registers[4] = return_esp;
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
}

#[test]
fn captured_tree_hidden_selection_and_ensure_visible_expand_then_scroll_only_if_needed() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = tree(&mut engine, callback, common, procedure, parent);
    let root = insert(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        0,
        "root",
        10,
    );
    let child = insert(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        root,
        "child",
        20,
    );
    let grandchild = insert(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        child,
        "grandchild",
        30,
    );
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[window, 0x110b, 9, grandchild],
    );
    let notes = drain(&mut engine, callback, common, procedure, false);
    assert_eq!(
        notes
            .iter()
            .map(|note| (note[2] as i32, note[15]))
            .collect::<Vec<_>>(),
        [
            (-454, root),
            (-455, root),
            (-454, child),
            (-455, child),
            (-450, grandchild),
            (-451, grandchild)
        ]
    );
    assert_eq!(eax(&engine), 1);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x110a, 6, root]
        ),
        child
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x110a, 6, child]
        ),
        grandchild
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x1114, 0, grandchild]
        ),
        0,
        "already visible items neither expand nor scroll"
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x110a, 5, 0]
        ),
        root
    );
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x1102, 1, root],
    );
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[window, 0x1114, 0, grandchild],
    );
    let notes = drain(&mut engine, callback, common, procedure, false);
    assert_eq!(
        notes.iter().map(|note| note[2] as i32).collect::<Vec<_>>(),
        [-454, -455]
    );
    assert_eq!(
        eax(&engine),
        0,
        "expansion makes EnsureVisible return zero even when it reveals the target"
    );
    let mut last = 0;
    for index in 0..10 {
        last = insert(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            0,
            &format!("row{index}"),
            100 + index,
        );
    }
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x1114, 0, last]
        ),
        1,
        "scrolling with no expansion is the nonzero case"
    );
    let first = send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x110a, 5, 0],
    );
    assert_ne!(first, root);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x1114, 0, last]
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
            [0x110a, 5, 0]
        ),
        first
    );
}
