use super::*;
fn automatic(
    engine: &mut EngineInstance,
    callback: u64,
    common: u64,
    procedure: u32,
    parent: u32,
) -> u32 {
    let window = tooltip(engine, callback, common, procedure);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x12600), b"automatic\0")
        .unwrap();
    put_info(engine, parent, 0x12600);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x12504), &0x10u32.to_le_bytes())
        .unwrap();
    assert_eq!(
        send(
            engine,
            callback,
            common,
            procedure,
            window,
            [0x404, 0, 0x12500]
        ),
        1
    );
    window
}
fn dispatch_timer(
    engine: &mut EngineInstance,
    callback: u64,
    common: u64,
    procedure: u32,
    window: u32,
    log: &mut Vec<(u32, u32, [u32; 3])>,
) {
    capture(
        engine,
        User32Api32::PeekMessageW.id(),
        &[0x12900, window, 0x113, 0x113, 1],
    );
    assert_eq!(eax(engine), 1);
    assert_eq!(words::<4>(engine, 0x12900), [window, 0x113, 1, 0]);
    capture(engine, User32Api32::DispatchMessageW.id(), &[0x12900]);
    drain_observing(engine, callback, common, procedure, log);
}
#[test]
fn captured_subclass_mouse_runs_original_once_and_real_timer_shows_then_hides_tooltip() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = automatic(&mut engine, callback, common, procedure, parent);
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[parent, 0x200, 0, 2 | (2 << 16)],
    );
    let mut calls = 0;
    for _ in 0..64 {
        if engine.pending_windows_callback().is_some() {
            callback_returning(&mut engine, callback, common, procedure, &mut |_, args| {
                if args[0] == parent && args[1] == 0x200 {
                    calls += 1;
                    assert_eq!(&args[2..], [0, 2 | (2 << 16)]);
                    Some(0x2345)
                } else {
                    None
                }
            });
        } else if let Some(wait) = engine.waiting_user32_message() {
            engine.resume_user32_wait(KEY, wait).unwrap();
        } else {
            assert!(engine.pending_call.is_none());
            break;
        }
    }
    assert_eq!(calls, 1);
    assert_eq!(eax(&engine), 0x2345);
    let mut log = Vec::new();
    engine.advance_cpu_clock(499_000_000).unwrap();
    capture(
        &mut engine,
        User32Api32::PeekMessageW.id(),
        &[0x12900, window, 0x113, 0x113, 1],
    );
    assert_eq!(eax(&engine), 0);
    engine.advance_cpu_clock(1_000_000).unwrap();
    dispatch_timer(&mut engine, callback, common, procedure, window, &mut log);
    assert_eq!(log, [(parent, 77, [window, 77, (-521i32) as u32])]);
    assert!(
        engine
            .user32_windows()
            .unwrap()
            .get(WindowHandle32::try_from(window).unwrap())
            .unwrap()
            .description
            .visible
    );
    engine.advance_cpu_clock(5_000_000_000).unwrap();
    dispatch_timer(&mut engine, callback, common, procedure, window, &mut log);
    assert_eq!(log.len(), 2);
    assert_eq!(log[1], (parent, 77, [window, 77, (-522i32) as u32]));
    assert!(
        !engine
            .user32_windows()
            .unwrap()
            .get(WindowHandle32::try_from(window).unwrap())
            .unwrap()
            .description
            .visible
    );
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[parent, 0x200, 0, 3 | (2 << 16)],
    );
    callbacks(&mut engine, callback, common, procedure);
    engine.advance_cpu_clock(10_000_000_000).unwrap();
    capture(
        &mut engine,
        User32Api32::PeekMessageW.id(),
        &[0x12900, window, 0x113, 0x113, 1],
    );
    assert_eq!(
        eax(&engine),
        0,
        "autopop suppresses the same hover until leave/reenter"
    );
}
#[test]
fn captured_subclass_timer_capacity_rejects_before_original_callback_or_state_publication() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = automatic(&mut engine, callback, common, procedure, parent);
    let (owner, _) = capture(
        &mut engine,
        User32Api32::SetTimer.id(),
        &[parent, 100, 10, 0],
    );
    let pc = decode_state(&engine.arena()[..STATE_SIZE]).unwrap().eip - 2;
    for id in 101..116u32 {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(
                GuestAddress(STACK),
                &[pc + 2, parent, id, 10, 0]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        stop(&mut engine, pc, User32Api32::SetTimer.id(), 0);
        let token = engine
            .capture_resident_call(KEY, owner, CallingConvention32::Stdcall, 4)
            .unwrap()
            .token;
        engine
            .complete_resident_windows_call(KEY, owner, token)
            .unwrap();
    }
    let before = engine
        .user32_windows()
        .unwrap()
        .get(WindowHandle32::try_from(window).unwrap())
        .unwrap();
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[parent, 0x200, 0, 2 | (2 << 16)],
    );
    assert!(engine.pending_call.is_none());
    assert!(engine.pending_windows_callback().is_none());
    assert_eq!(eax(&engine), 0);
    assert_eq!(
        engine
            .user32_windows()
            .unwrap()
            .get(WindowHandle32::try_from(window).unwrap())
            .unwrap(),
        before
    );
    capture(
        &mut engine,
        crate::windows::WindowsApi32::GetLastError.id(),
        &[],
    );
    assert_eq!(eax(&engine), 1450);
}
#[test]
fn private_tooltip_relay_gate_rejects_uncaptured_original_procedure_authority() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    automatic(&mut engine, callback, common, procedure, parent);
    let id = crate::windows::comctl32::TOOLTIP_RELAY_PROC;
    let gate = engine
        .loaded_windows_modules()
        .flat_map(|module| module.gates())
        .find(|gate| gate.id == id)
        .unwrap()
        .entry
        .0;
    let owner = unit(&mut engine, &[gate, 2, gate, id], 1, 1);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(STACK),
            &[PC + 2, parent, 0x200, 0, 0, PROC]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    stop(&mut engine, gate, id, 0);
    let token = engine
        .capture_resident_call(KEY, owner, CallingConvention32::Stdcall, 5)
        .unwrap()
        .token;
    let frame = engine.pending_call.as_ref().unwrap().frame;
    let arena = engine.arena().to_vec();
    let version = engine.memory().unwrap().transaction_version();
    assert!(matches!(
        engine.complete_resident_windows_call(KEY, owner, token),
        Err(HostError::InvalidRequest)
    ));
    assert_eq!(engine.pending_call.as_ref().unwrap().frame, frame);
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert!(engine.pending_windows_callback().is_none());
}

#[test]
fn foreign_subclass_invocation_preserves_sender_frame_and_calls_actual_owner_procedure_once() {
    use crate::windows::thread::{ThreadApi32, ThreadSchedule32};
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    engine
        .configure_synchronization(Default::default())
        .unwrap();
    automatic(&mut engine, callback, common, procedure, parent);
    engine.load_library_module("kernel32.dll", None).unwrap();
    assert!(
        engine
            .module_gates()
            .unwrap()
            .iter()
            .any(|gate| gate.id == crate::process::windows::threads::THREAD_RETURN_ID)
    );
    capture(
        &mut engine,
        ThreadApi32::CreateThread.id(),
        &[0, 0, PROC, 0, 0, 0],
    );
    assert_ne!(eax(&engine), 0);
    capture(
        &mut engine,
        User32Api32::GetMessageW.id(),
        &[0x12a00, 0, 0x8000, 0x8000],
    );
    assert!(engine.waiting_user32_message().is_some());
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 2 }
    );
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[parent, 0x200, 0, 2 | (2 << 16)],
    );
    let sender = {
        let pending = engine.pending_call.as_ref().unwrap();
        (pending.token, pending.frame)
    };
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    assert!(engine.pending_call.is_none());
    let id = crate::windows::comctl32::TOOLTIP_RELAY_PROC;
    let gate = engine
        .loaded_windows_modules()
        .flat_map(|module| module.gates())
        .find(|gate| gate.id == id)
        .unwrap()
        .entry
        .0;
    let state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    assert_eq!(state.eip, gate);
    let frame = words::<6>(&engine, state.registers[4]);
    assert_eq!(&frame[1..], &[parent, 0x200, 0, 2 | (2 << 16), PROC]);
    let invocation_return = frame[0];
    let owner = unit(&mut engine, &[gate, 2, gate, id], 1, 1);
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = engine
        .capture_resident_call(KEY, owner, CallingConvention32::Stdcall, 5)
        .unwrap()
        .token;
    engine
        .complete_resident_windows_call(KEY, owner, token)
        .unwrap();
    let mut calls = 0;
    for _ in 0..64 {
        if engine.pending_windows_callback().is_some() {
            callback_returning(
                &mut engine,
                callback,
                common,
                procedure,
                &mut |engine, args| {
                    if args[0] == parent && args[1] == 0x200 {
                        assert_eq!(engine.current_windows_thread_id().unwrap(), 1);
                        calls += 1;
                        Some(0x3456)
                    } else {
                        None
                    }
                },
            );
        } else if let Some(wait) = engine.waiting_user32_message() {
            engine.resume_user32_wait(KEY, wait).unwrap();
        } else {
            assert!(engine.pending_call.is_none());
            break;
        }
    }
    assert_eq!(calls, 1);
    let state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    assert_eq!(state.eip, invocation_return);
    assert_eq!(state.registers[0], 0x3456);
    let return_id = crate::process::windows::threads::INVOCATION_RETURN_ID;
    let returned_owner = unit(
        &mut engine,
        &[invocation_return, 2, invocation_return, return_id],
        1,
        1,
    );
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: return_id },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let returned = engine
        .capture_resident_call(KEY, returned_owner, CallingConvention32::Stdcall, 0)
        .unwrap();
    engine
        .complete_resident_windows_call(KEY, returned_owner, returned.token)
        .unwrap();
    let mut completed = false;
    for _ in 0..16 {
        if engine.current_windows_thread_id().unwrap() == 2 {
            if engine.pending_call.is_none() {
                completed = true;
                break;
            }
            let pending = engine.pending_call.as_ref().unwrap();
            assert_eq!((pending.token, pending.frame), sender);
        }
        match engine.schedule_windows_thread().unwrap() {
            ThreadSchedule32::Running { thread: 2 } => {}
            ThreadSchedule32::WindowsCallback { thread: 1 } => {
                let request = engine.pending_windows_callback().unwrap();
                assert_eq!(request.arguments[1], 0xf);
                super::super::callback(&mut engine, callback, common, procedure);
            }
            other => panic!("unexpected foreign-tooltip completion: {other:?}"),
        }
    }
    assert!(completed);
    assert_eq!(eax(&engine), 0x3456);
    assert_eq!(calls, 1);
}
