use super::*;

fn caller(thread: u32) -> ClipboardOwner32 {
    ClipboardOwner32 {
        thread,
        window: None,
    }
}

#[test]
fn null_window_ownership_and_repeated_open_follow_observed_windows_behavior() {
    let mut state = ClipboardState32::default();
    state.open(caller(1)).unwrap();
    state.open(caller(1)).unwrap();
    assert_eq!(state.open(caller(2)), Err(Error::Win32(5)));
    assert_eq!(state.close(2, 0x409), Err(Error::Win32(1418)));
    let sequence = state.sequence();
    state.commit_empty(caller(1), 0x409);
    state.prepare_set(13).unwrap();
    state.commit_set(13, ClipboardContent32::Delayed);
    assert_eq!(state.owner(), Some(caller(1)));
    assert_eq!(state.sequence().wrapping_sub(sequence), 2);
    assert_eq!(
        state
            .entries()
            .iter()
            .map(|entry| entry.format)
            .collect::<Vec<_>>(),
        [13]
    );
    state.close(1, 0x409).unwrap();
    assert_eq!(state.close(1, 0x409), Err(Error::Win32(1418)));
    assert_eq!(
        state
            .entries()
            .iter()
            .map(|entry| entry.format)
            .collect::<Vec<_>>(),
        [13, 16, 1, 7]
    );
    assert_eq!(
        state.get(1),
        Some(ClipboardContent32::Synthesized {
            source: 13,
            memory: None
        })
    );
}

#[test]
fn registered_names_preserve_first_spelling_without_aliasing_predefined_formats() {
    let mut state = ClipboardState32::default();
    let original: Vec<u16> = "Ring3 delayed data".encode_utf16().collect();
    let format = state.register(&original).unwrap();
    assert_eq!(format, 0xc000);
    assert_eq!(
        state
            .register(&"ring3 DELAYED DATA".encode_utf16().collect::<Vec<_>>())
            .unwrap(),
        format
    );
    assert_eq!(state.registered_name(format), Some(original.as_slice()));
    assert_eq!(state.registered_name(13), None);
    assert_eq!(state.register(&[]), Err(Error::Win32(87)));
    assert_eq!(state.register(&[65; 256]), Err(Error::Win32(87)));
}

#[test]
fn enumeration_priority_and_thread_retirement_preserve_other_owned_formats() {
    let mut state = ClipboardState32::default();
    assert_eq!(state.priority(&[13]), 0);
    assert_eq!(state.next(1, 0), Err(Error::Win32(1418)));
    state.open(caller(1)).unwrap();
    state.commit_empty(caller(1), 0x409);
    let data = ClipboardMemory32 {
        handle: 7,
        pointer: 0x10000,
        allocation: 12,
        bytes: 8,
    };
    state.prepare_set(13).unwrap();
    state.commit_set(13, ClipboardContent32::Memory(data));
    state.prepare_set(0xc000).unwrap();
    state.commit_set(0xc000, ClipboardContent32::Delayed);
    assert_eq!(state.next(1, 0), Ok(Some(13)));
    assert_eq!(state.next(1, 13), Ok(Some(0xc000)));
    assert_eq!(state.next(1, 0xc000), Ok(None));
    assert_eq!(state.next(1, 5), Err(Error::Win32(87)));
    assert_eq!(state.priority(&[8, 13, 0xc000]), 13);
    assert_eq!(state.priority(&[8]), -1);
    state.retire_thread(2);
    assert_eq!(state.opened_by(), Some(caller(1)));
    state.retire_thread(1);
    assert_eq!(state.opened_by(), None);
    assert_eq!(state.owner(), None);
    assert_eq!(state.get(13), Some(ClipboardContent32::Memory(data)));
    assert_eq!(state.get(0xc000), None);
}

#[test]
fn replacing_source_or_locale_retires_cached_conversions_without_losing_their_origin() {
    let mut state = ClipboardState32::default();
    state.open(caller(1)).unwrap();
    state.commit_empty(caller(1), 0x409);
    state.prepare_set(13).unwrap();
    state.commit_set(13, ClipboardContent32::Delayed);
    state.close(1, 0x41f).unwrap();
    assert_eq!(state.locale(), 0x41f);
    let memory = ClipboardMemory32 {
        handle: 7,
        pointer: 0x10000,
        allocation: 1,
        bytes: 8,
    };
    state.commit_synthesized(1, memory);
    assert_eq!(state.replaced_memory(13).collect::<Vec<_>>(), [memory]);
    assert_eq!(state.replaced_memory(16).collect::<Vec<_>>(), [memory]);
    state.prepare_set(16).unwrap();
    state.commit_set(16, ClipboardContent32::Memory(memory));
    assert_eq!(state.get(1), None);
    assert_eq!(state.get(7), None);
    assert_eq!(state.get(13), Some(ClipboardContent32::Delayed));
}
