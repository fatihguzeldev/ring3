use super::*;

#[test]
fn native_menu_attachment_queries_use_the_window_owner_without_requiring_the_caller_thread() {
    let mut engine = engine();
    call(
        &mut engine,
        crate::windows::thread::ThreadApi32::CreateThread.id(),
        &[0, 0, CODE + 0x100, 0, 0, 0],
    )
    .unwrap();
    write_template(&mut engine);
    begin(&mut engine, true);
    let window = reach_init(&mut engine);
    complete_callback(&mut engine, 0).unwrap();
    for _ in 0..64 {
        let Some(request) = engine.pending_windows_callback() else {
            break;
        };
        complete_callback(&mut engine, u32::from(request.arguments[1] == 0x81)).unwrap();
    }
    assert!(engine.waiting_user32_message().is_some());
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    call(&mut engine, Api::CreateMenu.id(), &[]).unwrap();
    let menu = crate::abi::x86::decode_state(&engine.arena()[..STATE_SIZE])
        .unwrap()
        .registers[0];
    assert_ne!(menu, 0);
    call(&mut engine, Api::SetMenu.id(), &[window, menu]).unwrap();
    let original = {
        let pending = engine.pending_call.as_ref().unwrap();
        (pending.token, pending.frame)
    };
    let mut delivered = Vec::new();
    let mut finished = false;
    let mut owner_running = false;
    fn owner_frame(
        engine: &EngineInstance,
        window: u32,
        index: usize,
    ) -> (crate::cpu::x86::State32, [u32; 5]) {
        assert_eq!(engine.current_windows_thread_id().unwrap(), 1);
        assert!(engine.pending_call.is_none());
        assert!(engine.pending_windows_callback().is_none());
        let state = crate::abi::x86::decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
        let data = engine
            .user32_windows()
            .unwrap()
            .get(WindowHandle32::try_from(window).unwrap())
            .unwrap();
        assert_eq!(data.description.thread_id, 1);
        assert_eq!(state.eip, data.description.procedure.0);
        let frame = crate::process::windows::user32::memory::words::<5>(
            engine.memory.as_ref().unwrap(),
            state.registers[4],
        )
        .unwrap();
        let gate = engine
            .loaded_windows_modules()
            .flat_map(|module| module.gates())
            .find(|gate| gate.id == crate::process::windows::threads::INVOCATION_RETURN_ID)
            .unwrap()
            .entry
            .0;
        assert_eq!(frame[0], gate);
        assert_eq!(frame[1], window);
        assert_eq!(frame[2], [0x46, 0x83, 0x85, 0x47][index]);
        assert_eq!(frame[3], u32::from(matches!(frame[2], 0x83 | 0x85)));
        if frame[2] == 0x85 {
            assert_eq!(frame[4], 0);
        } else {
            assert_ne!(frame[4], 0);
        }
        (state, frame)
    }
    for _ in 0..64 {
        if !owner_running
            && !engine.has_pending_thread_invocation(1)
            && let Some(wait) = engine.waiting_user32_message()
        {
            engine.resume_user32_wait(KEY, wait).unwrap();
        }
        if owner_running || engine.has_pending_thread_invocation(1) {
            if !owner_running {
                assert_eq!(
                    engine.schedule_windows_thread().unwrap(),
                    ThreadSchedule32::Running { thread: 1 }
                );
            }
            owner_running = false;
            let (mut state, frame) = owner_frame(&engine, window, delivered.len());
            delivered.push(frame[2]);
            assert_eq!(
                engine
                    .user32_windows()
                    .unwrap()
                    .get(WindowHandle32::try_from(window).unwrap())
                    .unwrap()
                    .description
                    .thread_id,
                1
            );
            state.eip = frame[0];
            state.registers[0] = 0;
            state.registers[4] += 20;
            for (index, word) in [
                frame[0],
                2,
                frame[0],
                crate::process::windows::threads::INVOCATION_RETURN_ID,
            ]
            .into_iter()
            .enumerate()
            {
                engine.arena_mut().unwrap()
                    [TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
                    .copy_from_slice(&word.to_le_bytes());
            }
            engine.compile_with_gates(1, 1).unwrap();
            encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
            encode_exit_v3(
                &ExecutionExit {
                    retired: 0,
                    reason: ExitReason::Gate {
                        id: crate::process::windows::threads::INVOCATION_RETURN_ID,
                    },
                },
                &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
            )
            .unwrap();
            let returned = engine
                .capture_call(KEY, engine.generation, CallingConvention32::Stdcall, 0)
                .unwrap();
            engine
                .complete_windows_call(KEY, engine.generation, returned.token)
                .unwrap();
            let mut sender = false;
            for _ in 0..16 {
                match engine.schedule_windows_thread().unwrap() {
                    ThreadSchedule32::Running { thread: 2 } => {
                        sender = true;
                        break;
                    }
                    ThreadSchedule32::Running { thread: 1 } => {
                        owner_frame(&engine, window, delivered.len());
                        owner_running = true;
                        sender = true;
                        break;
                    }
                    ThreadSchedule32::WindowsCallback { thread: 1 } => {
                        let request = engine.pending_windows_callback().unwrap();
                        assert_eq!(request.arguments[1], 0x0f);
                        complete_callback(&mut engine, 0).unwrap();
                    }
                    ThreadSchedule32::Waiting { deadline_ns: None } => {
                        assert!(
                            engine.has_pending_thread_invocation(1),
                            "bounded advancement may yield only with the next real owner call queued"
                        );
                        sender = true;
                        break;
                    }
                    other => panic!("unexpected owner callback return: {other:?}"),
                }
            }
            assert!(sender);
        }
        if engine.current_windows_thread_id().unwrap() == 2 {
            if engine.pending_call.is_none() {
                finished = true;
                break;
            }
            let pending = engine.pending_call.as_ref().unwrap();
            assert_eq!((pending.token, pending.frame), original);
        } else {
            assert!(owner_running || engine.has_pending_thread_invocation(1));
        }
    }
    assert!(
        finished,
        "SetMenu completes only after the actual owner callbacks return"
    );
    assert_eq!(delivered, [0x46, 0x83, 0x85, 0x47]);
    assert_eq!(
        crate::abi::x86::decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        1
    );
    call(&mut engine, Api::GetMenu.id(), &[window]).unwrap();
    assert_eq!(
        crate::abi::x86::decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        menu
    );
    assert_eq!(
        engine
            .user32_windows()
            .unwrap()
            .get(WindowHandle32::try_from(window).unwrap())
            .unwrap()
            .description
            .thread_id,
        1
    );
    assert!(!engine.has_pending_thread_invocation(1));
}
