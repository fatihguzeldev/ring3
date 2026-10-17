use super::*;
fn list(
    engine: &mut EngineInstance,
    callback: u64,
    common: u64,
    procedure: u32,
    parent: u32,
) -> u32 {
    list_with_style(engine, callback, common, procedure, parent, 0x50000009)
}
fn list_with_style(
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
            &"SysListView32\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    capture(
        engine,
        User32Api32::CreateWindowExW.id(),
        &[0, 0x12400, 0, style, 0, 0, 160, 96, parent, 94, 0x400000, 0],
    );
    callbacks(engine, callback, common, procedure);
    let window = eax(engine);
    assert_ne!(window, 0);
    window
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
fn text(engine: &mut EngineInstance, value: &str) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12600),
            &value
                .encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
}
fn column(
    engine: &mut EngineInstance,
    callback: u64,
    common: u64,
    procedure: u32,
    window: u32,
    index: u32,
    name: &str,
) {
    text(engine, name);
    put(engine, &[15, 0, 70, 0x12600, 0, index]);
    assert_eq!(
        send(
            engine,
            callback,
            common,
            procedure,
            window,
            [0x1061, index, 0x12500]
        ),
        index
    );
}
fn row(
    engine: &mut EngineInstance,
    callback: u64,
    common: u64,
    procedure: u32,
    window: u32,
    index: u32,
    name: &str,
) {
    text(engine, name);
    put(
        engine,
        &[5, index, 0, 0, 0, 0x12600, 0, u32::MAX, index + 10],
    );
    assert_eq!(
        send(
            engine,
            callback,
            common,
            procedure,
            window,
            [0x104d, 0, 0x12500]
        ),
        index
    );
}
fn notes(
    engine: &mut EngineInstance,
    callback: u64,
    common: u64,
    procedure: u32,
    window: u32,
    message: [u32; 3],
    veto: bool,
) -> Vec<[u32; 11]> {
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
                let note = words::<11>(engine, args[3]);
                if note[0] == window {
                    notes.push(note);
                    Some(u32::from(veto && note[2] == (-100i32) as u32))
                } else {
                    None
                }
            });
        } else if let Some(wait) = engine.waiting_user32_message() {
            engine.resume_user32_wait(KEY, wait).unwrap();
        } else {
            assert!(engine.pending_call.is_none());
            return notes;
        }
    }
    panic!("bounded list-view notification chain");
}
#[test]
fn captured_report_rows_subitems_and_real_header_share_one_canonical_state() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = list(&mut engine, callback, common, procedure, parent);
    let header = send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x101f, 0, 0],
    );
    assert_ne!(header, 0);
    capture(
        &mut engine,
        User32Api32::GetWindowLongW.id(),
        &[header, (-8i32) as u32],
    );
    assert_eq!(eax(&engine), window);
    column(&mut engine, callback, common, procedure, window, 0, "name");
    column(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        1,
        "detail",
    );
    row(&mut engine, callback, common, procedure, window, 0, "alpha");
    text(&mut engine, "日本");
    put(&mut engine, &[0, 0, 1, 0, 0, 0x12600]);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x1074, 0, 0x12500]
        ),
        1
    );
    put(&mut engine, &[0, 0, 1, 0, 0, 0x12700, 8]);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x1073, 0, 0x12500]
        ),
        2
    );
    assert_eq!(words::<2>(&engine, 0x12700), [0x672c65e5, 0]);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            header,
            [0x1200, 0, 0]
        ),
        2
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
        .find_map(|index| {
            engine
                .pending_gdi_frame(index)
                .unwrap()
                .filter(|frame| frame.window == window)
        })
        .expect("real ListView pixels");
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
    let header_height = engine
        .user32_windows()
        .unwrap()
        .get(WindowHandle32::try_from(header).unwrap())
        .unwrap()
        .description
        .extent
        .height as usize;
    assert!(
        (header_height + 2..(header_height + 18).min(frame.height as usize))
            .any(|y| (5..60).any(|x| pixels
                [(y * frame.width as usize + x) * 4..(y * frame.width as usize + x) * 4 + 3]
                != [255, 255, 255])),
        "the row label must have actual font ink below its separate Header child"
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            header,
            [0x1202, 1, 0]
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
            [0x101d, 1, 0]
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
            [0x1004, 0, 0]
        ),
        1
    );
}
#[test]
fn actual_parent_can_veto_item_change_before_state_and_owned_text_publication() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = list(&mut engine, callback, common, procedure, parent);
    row(&mut engine, callback, common, procedure, window, 0, "old");
    text(&mut engine, "new");
    put(&mut engine, &[9, 0, 0, 3, 3, 0x12600, 0, u32::MAX, 10]);
    let refused = notes(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x104c, 0, 0x12500],
        true,
    );
    assert_eq!(
        refused.iter().map(|row| row[2] as i32).collect::<Vec<_>>(),
        [-100]
    );
    assert_eq!(eax(&engine), 0);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x102c, 0, 3]
        ),
        0
    );
    let accepted = notes(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x104c, 0, 0x12500],
        false,
    );
    assert_eq!(
        accepted.iter().map(|row| row[2] as i32).collect::<Vec<_>>(),
        [-100, -101]
    );
    assert_eq!(&accepted[0][3..8], &[0, 0, 3, 0, 9]);
    assert_eq!(&accepted[1][3..8], &[0, 0, 3, 0, 9]);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x102c, 0, 3]
        ),
        3
    );
    put(&mut engine, &[0, 0, 0, 0, 0, 0x12700, 8]);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x1073, 0, 0x12500]
        ),
        3
    );
    assert_eq!(words::<2>(&engine, 0x12700), [0x0065006e, 0x00000077]);
}
#[test]
fn listview_output_failure_is_atomic_and_repair_reads_the_same_row_and_header() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = list(&mut engine, callback, common, procedure, parent);
    column(&mut engine, callback, common, procedure, window, 0, "name");
    row(&mut engine, callback, common, procedure, window, 0, "owned");
    put(&mut engine, &[13, 0, 0, 0, 3, 0x12700, 8, 0, 0]);
    let range = crate::memory::PageRange::new(GuestAddress(0x12000), 1).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(range, crate::memory::Permissions::READ)
        .unwrap();
    let version = engine.memory.as_ref().unwrap().transaction_version();
    let result = engine.execute_common_control_procedure(&[window, 0x104b, 0, 0x12500], true);
    assert!(matches!(
        result,
        Err(HostError::User32(
            crate::windows::user32::User32Error32::Memory(_)
        ))
    ));
    assert_eq!(
        engine.memory.as_ref().unwrap().transaction_version(),
        version
    );
    assert_eq!(
        words::<9>(&engine, 0x12500),
        [13, 0, 0, 0, 3, 0x12700, 8, 0, 0]
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(range, crate::memory::Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x104b, 0, 0x12500]
        ),
        1
    );
    assert_eq!(words::<9>(&engine, 0x12500)[8], 10);
    assert_eq!(
        words::<3>(&engine, 0x12700),
        [0x0077006f, 0x0065006e, 0x00000064]
    );
}
#[test]
fn mouse_and_keyboard_selection_use_actual_focus_capture_and_parent_change_notifications() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = list(&mut engine, callback, common, procedure, parent);
    column(&mut engine, callback, common, procedure, window, 0, "name");
    for (index, name) in ["first", "second", "third"].into_iter().enumerate() {
        row(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            index as u32,
            name,
        );
    }
    let header = send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x101f, 0, 0],
    );
    let header_height = engine
        .user32_windows()
        .unwrap()
        .get(WindowHandle32::try_from(header).unwrap())
        .unwrap()
        .description
        .extent
        .height;
    let point = 5 | ((header_height + 5) << 16);
    let changes = notes(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x201, 0, point],
        false,
    );
    assert!(changes.iter().any(|note| note[2] == (-101i32) as u32));
    assert_eq!(
        engine
            .user32_windows()
            .unwrap()
            .focus()
            .map(WindowHandle32::get),
        Some(window)
    );
    assert_eq!(
        engine
            .user32_windows()
            .unwrap()
            .capture()
            .map(WindowHandle32::get),
        Some(window)
    );
    let released = notes(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x202, 0, point],
        false,
    );
    assert!(released.iter().any(|note| note[2] == (-2i32) as u32));
    assert_eq!(engine.user32_windows().unwrap().capture(), None);
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
            [0x100c, u32::MAX, 3]
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
            [0x1032, 0, 0]
        ),
        1
    );
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x100, 0x24, 0],
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x100c, u32::MAX, 1]
        ),
        0,
        "multi-selection keyboard Home must move the sole focused item backwards"
    );
}

#[test]
fn listview_erase_publishes_the_configured_background_to_the_real_borrowed_dc() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = list(&mut engine, callback, common, procedure, parent);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x14, 0, 0]
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
            [0x1001, 0, 0x00563412]
        ),
        1
    );
    capture(&mut engine, GdiApi32::CreateCompatibleDC.id(), &[0]);
    let dc = eax(&engine);
    assert_ne!(dc, 0);
    capture(&mut engine, GdiApi32::CreateBitmap.id(), &[4, 4, 1, 32, 0]);
    let bitmap = eax(&engine);
    assert_ne!(bitmap, 0);
    capture(&mut engine, GdiApi32::SelectObject.id(), &[dc, bitmap]);
    let old = eax(&engine);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x14, dc, 0]
        ),
        1
    );
    capture(&mut engine, GdiApi32::GetPixel.id(), &[dc, 1, 1]);
    assert_eq!(eax(&engine), 0x00563412);
    capture(&mut engine, GdiApi32::SelectObject.id(), &[dc, old]);
    assert_eq!(eax(&engine), bitmap);
    capture(&mut engine, GdiApi32::DeleteObject.id(), &[bitmap]);
    assert_eq!(eax(&engine), 1);
    capture(&mut engine, GdiApi32::DeleteDC.id(), &[dc]);
    assert_eq!(eax(&engine), 1);
}

#[test]
fn captured_single_selection_programmatic_set_and_initial_selected_insert_send_real_deselections() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = list_with_style(&mut engine, callback, common, procedure, parent, 0x5000000d);
    row(&mut engine, callback, common, procedure, window, 0, "first");
    row(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        1,
        "second",
    );
    put(&mut engine, &[8, 0, 0, 2, 2]);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x102b, 0, 0x12500]
        ),
        1
    );
    put(&mut engine, &[8, 0, 0, 2, 2]);
    let changed = notes(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x102b, 1, 0x12500],
        false,
    );
    assert_eq!(
        changed
            .iter()
            .map(|note| (note[2] as i32, note[3], note[5], note[6]))
            .collect::<Vec<_>>(),
        [
            (-100, 0, 0, 2),
            (-101, 0, 0, 2),
            (-100, 1, 2, 0),
            (-101, 1, 2, 0)
        ]
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x1032, 0, 0]
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
            [0x102c, 0, 2]
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
            [0x102c, 1, 2]
        ),
        2
    );
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x100, 0x24, 0],
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x102c, 0, 2]
        ),
        2,
        "keyboard selection toward an earlier row must deselect the later row first"
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x1032, 0, 0]
        ),
        1
    );
    put(&mut engine, &[8, 0, 0, 2, 2]);
    send(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x102b, 1, 0x12500],
    );
    text(&mut engine, "selected insertion");
    put(&mut engine, &[9, 2, 0, 2, 2, 0x12600, 0, 0, 0]);
    let inserted = notes(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x104d, 0, 0x12500],
        false,
    );
    assert_eq!(eax(&engine), 2);
    assert_eq!(
        inserted
            .iter()
            .map(|note| (note[2] as i32, note[3]))
            .collect::<Vec<_>>(),
        [(-100, 1), (-101, 1), (-102, 2)]
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x1032, 0, 0]
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
            [0x102c, 1, 2]
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
            [0x102c, 2, 2]
        ),
        2
    );
    put(&mut engine, &[8, 0, 0, 2, 2]);
    notes(
        &mut engine,
        callback,
        common,
        procedure,
        window,
        [0x102b, 0, 0x12500],
        true,
    );
    assert_eq!(eax(&engine), 0);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x1032, 0, 0]
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
            [0x102c, 2, 2]
        ),
        2
    );
}
