use super::*;
use crate::windows::thread::{ThreadApi32, ThreadSchedule32};

fn on_thread(engine: &mut EngineInstance, id: u32, args: &[u32]) -> (u64, u32) {
    let pc = PC + 0x400 + engine.resident.as_ref().unwrap().usage().units as u32 * 4;
    let owner = unit(engine, &[pc, 2, pc, id], 1, 1);
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = pc;
    state.registers[4] -= 64;
    let bytes = std::iter::once(pc + 2)
        .chain(args.iter().copied())
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(state.registers[4]), &bytes)
        .unwrap();
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = engine
        .capture_resident_call(KEY, owner, CallingConvention32::Stdcall, args.len() as u32)
        .unwrap()
        .token;
    engine
        .complete_resident_windows_call(KEY, owner, token)
        .unwrap();
    (owner, token)
}
fn child_fixture() -> (EngineInstance, u64, u32) {
    let (mut engine, callback, window) = fixture_with_startup(true);
    engine
        .configure_synchronization(Default::default())
        .unwrap();
    engine.load_library_module("kernel32.dll", None).unwrap();
    capture(
        &mut engine,
        ThreadApi32::CreateThread.id(),
        &[0, 0, PROC, 0, 4, 0x12400],
    );
    let child = eax(&engine);
    assert_ne!(child, 0);
    assert_eq!(words::<1>(&engine, 0x12400), [2]);
    capture(&mut engine, ThreadApi32::ResumeThread.id(), &[child]);
    assert_eq!(eax(&engine), 1);
    capture(&mut engine, GdiApi32::ValidateRect.id(), &[window, 0]);
    capture(
        &mut engine,
        User32Api32::GetMessageW.id(),
        &[0x12500, 0, 0, 0],
    );
    assert!(engine.waiting_user32_message().is_some());
    select(&mut engine, 2);
    on_thread(&mut engine, GdiApi32::InvalidateRect.id(), &[window, 0, 1]);
    (engine, callback, window)
}
fn select(engine: &mut EngineInstance, thread: u32) {
    for _ in 0..8 {
        match engine.schedule_windows_thread().unwrap() {
            ThreadSchedule32::Running { thread: current } if current == thread => return,
            ThreadSchedule32::Running { .. } => {}
            other => panic!("expected ready thread {thread}, got {other:?}"),
        }
    }
    panic!("ready thread {thread} must be selected within the bounded two-thread cycle");
}
fn owner_call(engine: &mut EngineInstance, window: u32, message: u32, dc: Option<u32>) -> u32 {
    select(engine, 1);
    assert_eq!(engine.current_windows_thread_id().unwrap(), 1);
    let state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    assert_eq!(state.eip, PROC);
    let [returned, hwnd, msg, wparam, lparam] = words::<5>(engine, state.registers[4]);
    let gate = engine
        .module_gates()
        .unwrap()
        .into_iter()
        .find(|gate| gate.id == crate::process::windows::threads::INVOCATION_RETURN_ID)
        .unwrap();
    assert_eq!(returned, gate.entry.0);
    assert_eq!([hwnd, msg, lparam], [window, message, 0]);
    if let Some(dc) = dc {
        assert_eq!(wparam, dc);
    } else {
        assert_eq!(wparam, 0);
    }
    state.registers[4] + 20
}
fn return_owner_call(engine: &mut EngineInstance, esp: u32, result: u32) {
    let id = crate::process::windows::threads::INVOCATION_RETURN_ID;
    let pc = engine
        .module_gates()
        .unwrap()
        .into_iter()
        .find(|gate| gate.id == id)
        .unwrap()
        .entry
        .0;
    let owner = unit(engine, &[pc, 2, pc, id], 1, 1);
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.eip = pc;
    state.registers[0] = result;
    state.registers[4] = esp;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = engine
        .capture_resident_call(KEY, owner, CallingConvention32::Stdcall, 0)
        .unwrap()
        .token;
    engine
        .complete_resident_windows_call(KEY, owner, token)
        .unwrap();
}

#[test]
fn foreign_beginpaint_sends_erase_to_actual_window_owner_and_restores_caller_result() {
    let (mut engine, _, window) = child_fixture();
    on_thread(
        &mut engine,
        GdiApi32::GetUpdateRect.id(),
        &[window, 0x12300, 1],
    );
    assert_eq!(eax(&engine), 1);
    assert!(engine.pending_windows_callback().is_none());
    assert!(engine.waiting_user32_message().is_none());
    on_thread(&mut engine, GdiApi32::BeginPaint.id(), &[window, PAINT]);
    let [dc, erase, left, top, right, bottom] = words::<6>(&engine, PAINT);
    assert_ne!(dc, 0);
    assert_eq!([erase, left, top, right, bottom], [1, 0, 0, 8, 6]);
    let parent = engine
        .waiting_user32_message()
        .expect("foreign paint must retain its parent wait");
    let frame = engine.pending_call.as_ref().unwrap().frame;
    engine.resume_user32_wait(KEY, parent).unwrap();
    let esp = owner_call(&mut engine, window, 0x14, Some(dc));
    on_thread(&mut engine, GdiApi32::ReleaseDC.id(), &[window, dc]);
    assert_eq!(
        eax(&engine),
        0,
        "receiver can draw through the loan but cannot release caller's DC"
    );
    assert!(engine.gdi.as_ref().unwrap().model.dc_kind(dc).is_ok());
    on_thread(
        &mut engine,
        User32Api32::DefWindowProcA.id(),
        &[window, 0x14, dc, 0],
    );
    assert_eq!(eax(&engine), 1);
    let paint_frame = engine.pending_gdi_frame(0).unwrap().unwrap();
    assert_eq!(
        (paint_frame.window, paint_frame.width, paint_frame.height),
        (window, 8, 6)
    );
    let pixels = engine
        .read_gdi_frame(paint_frame.id, paint_frame.revision, 0, paint_frame.bytes)
        .unwrap();
    assert!(
        pixels
            .chunks_exact(4)
            .all(|pixel| pixel == [0xc0, 0xc0, 0xc0, 0])
    );
    return_owner_call(&mut engine, esp, 1);
    select(&mut engine, 2);
    assert!(engine.pending_call.is_none());
    assert!(engine.waiting_user32_message().is_none());
    assert!(engine.pending_windows_callback().is_none());
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE]).unwrap(),
        frame.complete(dc)
    );
    assert_eq!(words::<1>(&engine, PAINT + 4), [0]);
    on_thread(&mut engine, GdiApi32::EndPaint.id(), &[window, PAINT]);
    assert_eq!(eax(&engine), 1);
    assert!(engine.gdi.as_ref().unwrap().model.dc_kind(dc).is_err());
}

#[test]
fn foreign_updatewindow_runs_real_owner_paint_with_nested_owner_erase() {
    let (mut engine, callback, window) = child_fixture();
    on_thread(&mut engine, GdiApi32::UpdateWindow.id(), &[window]);
    let parent = engine.waiting_user32_message().unwrap();
    let frame = engine.pending_call.as_ref().unwrap().frame;
    engine.resume_user32_wait(KEY, parent).unwrap();
    let esp = owner_call(&mut engine, window, 0x0f, None);
    on_thread(&mut engine, GdiApi32::BeginPaint.id(), &[window, PAINT]);
    let request = engine.pending_windows_callback().unwrap();
    assert_eq!(engine.current_windows_thread_id().unwrap(), 1);
    assert_eq!(&request.arguments[..2], &[window, 0x14]);
    finish(&mut engine, callback, 1);
    assert_eq!(words::<1>(&engine, PAINT + 4), [0]);
    on_thread(&mut engine, GdiApi32::EndPaint.id(), &[window, PAINT]);
    return_owner_call(&mut engine, esp, 0);
    select(&mut engine, 2);
    assert!(engine.pending_call.is_none());
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE]).unwrap(),
        frame.complete(1)
    );
    on_thread(
        &mut engine,
        GdiApi32::GetUpdateRect.id(),
        &[window, 0x12300, 0],
    );
    assert_eq!(eax(&engine), 0);
    assert_eq!(words::<4>(&engine, 0x12300), [0; 4]);
}

#[test]
fn destroyed_window_retires_unreachable_beginpaint_leases_without_endpaint() {
    let (mut engine, callback, window) = fixture();
    capture(&mut engine, GdiApi32::BeginPaint.id(), &[window, PAINT]);
    finish(&mut engine, callback, 1);
    let dc = words::<1>(&engine, PAINT)[0];
    assert_eq!(engine.gdi.as_ref().unwrap().paints.len(), 1);
    capture(&mut engine, User32Api32::DestroyWindow.id(), &[window]);
    while engine.pending_windows_callback().is_some() {
        finish(&mut engine, callback, 0);
    }
    assert_eq!(eax(&engine), 1);
    assert!(engine.gdi.as_ref().unwrap().model.dc_kind(dc).is_err());
    assert!(
        engine.gdi.as_ref().unwrap().paints.is_empty(),
        "dead HWND makes its outstanding paint DC lease unreachable"
    );
}
