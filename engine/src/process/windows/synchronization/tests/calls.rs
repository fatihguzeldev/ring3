use super::*;

#[test]
fn guest_event_duplicate_rights_and_last_error_use_the_actual_captured_kernel32_path() {
    let mut engine = fixture();
    let handle = call(&mut engine, WindowsApi32::CreateEventA, &[0, 0, 0, 0]);
    assert_ne!(handle, 0);
    assert_eq!(call(&mut engine, WindowsApi32::SetEvent, &[handle]), 1);
    assert_eq!(
        call(&mut engine, WindowsApi32::WaitForSingleObject, &[handle, 0]),
        0
    );
    assert_eq!(
        call(
            &mut engine,
            WindowsApi32::WaitForSingleObjectEx,
            &[handle, 0, 1]
        ),
        WAIT_TIMEOUT
    );
    assert_eq!(
        call(
            &mut engine,
            WindowsApi32::DuplicateHandle,
            &[u32::MAX, handle, u32::MAX, OUTPUT, 0, 1, 2]
        ),
        1
    );
    let mut bytes = [0; 4];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(OUTPUT), &mut bytes)
        .unwrap();
    let duplicate = u32::from_le_bytes(bytes);
    assert_ne!(duplicate, handle);
    assert_eq!(call(&mut engine, WindowsApi32::CloseHandle, &[handle]), 1);
    assert_eq!(call(&mut engine, WindowsApi32::SetEvent, &[handle]), 0);
    assert_eq!(engine.windows_thread.last_error(), 6);
    assert_eq!(call(&mut engine, WindowsApi32::SetEvent, &[duplicate]), 1);
    assert_eq!(
        call(
            &mut engine,
            WindowsApi32::WaitForSingleObject,
            &[duplicate, 0]
        ),
        0
    );
}

#[test]
fn host_wake_preserves_captured_state_until_exact_resume_and_generic_bypasses_remain_busy() {
    let mut engine = fixture();
    let handle = call(&mut engine, WindowsApi32::CreateEventW, &[0, 0, 0, 0]);
    let token = capture(
        &mut engine,
        WindowsApi32::WaitForSingleObject,
        &[handle, INFINITE],
    );
    let before = engine.arena().to_vec();
    complete(&mut engine, token).unwrap();
    let wait = engine.pending_windows_wait().unwrap();
    assert!(!wait.ready);
    assert_eq!(wait.deadline_ns, None);
    assert_eq!(
        engine.resume_windows_wait(KEY, token),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.abandon_call(KEY, token),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.complete_call(KEY, engine.generation, token, 0),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.signal_windows_event(wait.epoch + 1, handle),
        Err(HostError::Synchronization(Error::Stale))
    );
    assert_eq!(engine.arena(), before);
    engine.signal_windows_event(wait.epoch, handle).unwrap();
    engine.reset_windows_event(wait.epoch, handle).unwrap();
    assert!(engine.pending_windows_wait().unwrap().ready);
    assert_eq!(
        engine.resume_windows_wait(KEY, token + 1),
        Err(HostError::Call(CallError::InvalidToken))
    );
    engine.resume_windows_wait(KEY, token).unwrap();
    let state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    assert_eq!(
        (state.registers[0], state.registers[4], state.eip),
        (0, STACK + 12, 0x20900)
    );
    assert!(engine.pending_windows_wait().is_none());
    assert_eq!(
        engine.resume_windows_wait(KEY, token),
        Err(HostError::Call(CallError::InvalidToken))
    );
}

#[test]
fn finite_wait_sleep_and_explicit_cancel_follow_observed_clock_without_spinning() {
    let mut engine = fixture();
    let handle = call(&mut engine, WindowsApi32::CreateEventA, &[0, 1, 0, 0]);
    let token = capture(
        &mut engine,
        WindowsApi32::WaitForSingleObject,
        &[handle, 10],
    );
    complete(&mut engine, token).unwrap();
    assert_eq!(
        engine.pending_windows_wait().unwrap().deadline_ns,
        Some(10_000_000)
    );
    engine.observe_cpu_clock(9_999_999).unwrap();
    assert!(!engine.pending_windows_wait().unwrap().ready);
    engine.observe_cpu_clock(10_000_000).unwrap();
    assert!(engine.pending_windows_wait().unwrap().ready);
    engine.resume_windows_wait(KEY, token).unwrap();
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        WAIT_TIMEOUT
    );
    let token = capture(&mut engine, WindowsApi32::SleepEx, &[5, 1]);
    complete(&mut engine, token).unwrap();
    engine.advance_cpu_clock(5_000_000).unwrap();
    engine.resume_windows_wait(KEY, token).unwrap();
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        0
    );
    let token = capture(
        &mut engine,
        WindowsApi32::WaitForSingleObject,
        &[handle, INFINITE],
    );
    complete(&mut engine, token).unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.cancel_windows_wait(KEY + 1, token),
        Err(HostError::InvalidArtifact)
    );
    assert_eq!(
        engine.cancel_windows_wait(KEY, token + 1),
        Err(HostError::Call(CallError::InvalidToken))
    );
    engine.cancel_windows_wait(KEY, token).unwrap();
    assert_eq!(engine.arena(), before);
    assert!(engine.pending_call.is_none());
    assert!(engine.pending_windows_wait().is_none());
    assert_eq!(
        call(&mut engine, WindowsApi32::WaitForSingleObject, &[handle, 0]),
        WAIT_TIMEOUT
    );
}

#[test]
fn duplicate_output_version_failure_keeps_source_and_proposed_handle_unpublished() {
    let mut engine = fixture();
    let handle = call(&mut engine, WindowsApi32::CreateEventA, &[0, 0, 0, 0]);
    let proposed = engine
        .synchronization()
        .unwrap()
        .prepare_duplicate(handle, EVENT_ALL_ACCESS, false, true, false, true)
        .unwrap()
        .handle();
    let token = capture(
        &mut engine,
        WindowsApi32::DuplicateHandle,
        &[u32::MAX, handle, u32::MAX, OUTPUT, 0, 0, 2],
    );
    engine.memory.as_mut().unwrap().exhaust_versions_for_test();
    let before = engine.arena().to_vec();
    assert_eq!(
        complete(&mut engine, token),
        Err(HostError::Memory(MemoryError::VersionExhausted))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(
        engine.synchronization().unwrap().event_is_signaled(handle),
        Ok(false)
    );
    assert_eq!(
        engine
            .synchronization()
            .unwrap()
            .prepare_duplicate(handle, 0, false, true, false, true)
            .unwrap()
            .handle(),
        proposed
    );
    assert!(engine.pending_call.is_some());
}

#[test]
fn shared_clock_preflight_keeps_cpu_deadline_and_synchronization_atomic_at_exhaustion() {
    let mut engine = fixture();
    let handle = call(&mut engine, WindowsApi32::CreateEventA, &[0, 1, 0, 0]);
    let token = capture(
        &mut engine,
        WindowsApi32::WaitForSingleObject,
        &[handle, 10],
    );
    complete(&mut engine, token).unwrap();
    let deadline = engine.pending_windows_wait().unwrap().deadline_ns;
    engine.observe_cpu_clock(1_000_000).unwrap();
    let before = engine.cpu_clock.monotonic_bounds().unwrap();
    assert_eq!(
        engine.observe_cpu_clock(999_999),
        Err(HostError::Synchronization(Error::ClockBackwards))
    );
    assert_eq!(engine.cpu_clock.monotonic_bounds().unwrap(), before);
    assert_eq!(engine.pending_windows_wait().unwrap().deadline_ns, deadline);
    engine
        .synchronization
        .as_mut()
        .unwrap()
        .context
        .set_revision_for_test(u64::MAX - 1);
    engine.advance_cpu_clock(1_000_000).unwrap();
    assert_eq!(engine.cpu_clock.monotonic_bounds().unwrap().1, 2_000_000);
    assert_eq!(engine.synchronization().unwrap().now_ns(), 2_000_000);
    assert_eq!(engine.synchronization().unwrap().revision(), u64::MAX);
    let before = engine.cpu_clock.monotonic_bounds().unwrap();
    assert_eq!(
        engine.advance_cpu_clock(1),
        Err(HostError::Synchronization(Error::Exhausted))
    );
    assert_eq!(engine.cpu_clock.monotonic_bounds().unwrap(), before);
    assert_eq!(engine.synchronization().unwrap().now_ns(), 2_000_000);
    assert_eq!(engine.pending_windows_wait().unwrap().deadline_ns, deadline);
    assert!(!engine.pending_windows_wait().unwrap().ready);
}

#[test]
fn captured_waitmessage_wakes_once_and_msgwait_inputavailable_observes_old_messages() {
    use crate::{
        memory::PageRange,
        windows::user32::{Message32, User32Api32, User32Limits32},
    };
    let mut engine = fixture();
    engine
        .configure_user32(
            User32Limits32::default(),
            PageRange::new(GuestAddress(0x40000), 4).unwrap(),
        )
        .unwrap();
    let apis = [
        User32Api32::WaitMessage,
        User32Api32::MsgWaitForMultipleObjectsEx,
        User32Api32::PeekMessageW,
    ];
    for index in 0..apis.len() {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(
                GuestAddress(CODE + 0x800 + index as u32 * 16),
                &[0x0f, 0x0b],
            )
            .unwrap();
    }
    let words = [
        CODE + 0x800,
        2,
        CODE + 0x810,
        2,
        CODE + 0x820,
        2,
        CODE + 0x800,
        apis[0].id(),
        CODE + 0x810,
        apis[1].id(),
        CODE + 0x820,
        apis[2].id(),
    ];
    for (index, word) in words.iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    engine.compile_with_gates(3, 3).unwrap();
    let capture_message = |engine: &mut EngineInstance, api: User32Api32, args: &[u32]| {
        for (index, value) in std::iter::once(0x20900)
            .chain(args.iter().copied())
            .enumerate()
        {
            engine
                .memory
                .as_mut()
                .unwrap()
                .write(GuestAddress(STACK + index as u32 * 4), &value.to_le_bytes())
                .unwrap();
        }
        let mut state = State32 {
            eip: CODE + 0x800 + apis.iter().position(|&value| value == api).unwrap() as u32 * 16,
            ..Default::default()
        };
        state.registers[4] = STACK;
        encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
        encode_exit_v3(
            &ExecutionExit {
                retired: 0,
                reason: ExitReason::Gate { id: api.id() },
            },
            &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
        )
        .unwrap();
        engine
            .capture_windows_call(KEY, engine.generation)
            .unwrap()
            .token
    };
    let token = capture_message(&mut engine, apis[0], &[]);
    complete(&mut engine, token).unwrap();
    assert!(!engine.pending_windows_wait().unwrap().ready);
    engine
        .post_user32_message(Message32 {
            thread_id: 1,
            message: 0x400,
            ..Default::default()
        })
        .unwrap();
    assert!(engine.pending_windows_wait().unwrap().ready);
    engine.resume_windows_wait(KEY, token).unwrap();
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        1
    );
    let token = capture_message(&mut engine, apis[0], &[]);
    complete(&mut engine, token).unwrap();
    assert!(!engine.pending_windows_wait().unwrap().ready);
    engine.cancel_windows_wait(KEY, token).unwrap();
    let token = capture_message(&mut engine, apis[1], &[0, 0, 0, 8, 4]);
    complete(&mut engine, token).unwrap();
    assert!(engine.pending_windows_wait().is_none());
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        0
    );
    engine
        .post_user32_message(Message32 {
            thread_id: 1,
            message: 0x401,
            ..Default::default()
        })
        .unwrap();
    let token = capture_message(&mut engine, apis[2], &[OUTPUT, 0, 0x100, 0x109, 0]);
    complete(&mut engine, token).unwrap();
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        0
    );
    let token = capture_message(&mut engine, apis[1], &[0, 0, 0, 0x100, 0]);
    complete(&mut engine, token).unwrap();
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        0
    );
}

#[test]
fn captured_ansi_event_name_matches_unicode_object_under_configured_dbcs() {
    use crate::windows::{nls::NlsProfile32, text::CodePage32};
    let mut engine = fixture();
    engine
        .configure_nls(NlsProfile32 {
            locale: 0x411,
            ansi: CodePage32::from_id(932).unwrap(),
            oem: CodePage32::from_id(932).unwrap(),
            ..Default::default()
        })
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(OUTPUT + 0x100),
            &[0x83, 0x65, 0x83, 0x58, 0x83, 0x67, 0],
        )
        .unwrap();
    let name = "テスト\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(OUTPUT + 0x200), &name)
        .unwrap();
    let original = call(
        &mut engine,
        WindowsApi32::CreateEventA,
        &[0, 0, 0, OUTPUT + 0x100],
    );
    assert_ne!(original, 0);
    let unicode = call(
        &mut engine,
        WindowsApi32::OpenEventW,
        &[EVENT_ALL_ACCESS, 0, OUTPUT + 0x200],
    );
    assert_ne!(unicode, 0);
    assert_ne!(unicode, original);
    assert_eq!(call(&mut engine, WindowsApi32::SetEvent, &[original]), 1);
    assert_eq!(
        call(
            &mut engine,
            WindowsApi32::WaitForSingleObject,
            &[unicode, 0]
        ),
        0
    );
    assert_eq!(
        call(
            &mut engine,
            WindowsApi32::WaitForSingleObject,
            &[original, 0]
        ),
        WAIT_TIMEOUT
    );
    assert_eq!(call(&mut engine, WindowsApi32::CloseHandle, &[unicode]), 1);
    assert_eq!(call(&mut engine, WindowsApi32::CloseHandle, &[original]), 1);
}
