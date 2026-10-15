use super::*;
use crate::{
    abi::{
        arena::{EXIT_OFFSET, STATE_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, decode_state, encode_exit_v3, encode_state_current},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    input::{InputKind, InputLimits, MAX_BATCH_EVENTS, MIN_QUEUE_EVENTS, TimedInputEvent},
    process::{
        CallError, EngineInstance, HostError, MediaConfig, MediaError, calls::call::PendingOwner,
    },
};

const KEY: u64 = 42;
const CALL_PC: u32 = 0x70000;
const CALLBACK_PC: u32 = 0x70010;
const RETURN_PC: u32 = 0x70020;
const RETURN_ID: u32 = 0x200ff;
const STACK: u32 = 0x11f00;

fn stop(engine: &mut EngineInstance, pc: u32, id: u32, esp: u32, result: u32) {
    let mut state = State32 {
        eip: pc,
        ..Default::default()
    };
    state.registers[4] = esp;
    state.registers[0] = result;
    encode_state_current(
        &state,
        &mut engine.arena_mut().unwrap()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE],
    )
    .unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
}

fn unit(engine: &mut EngineInstance, values: &[u32], blocks: u32, gates: u32) -> u64 {
    for (index, value) in values.iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    engine
        .compile_resident_with_gates(blocks, gates)
        .unwrap()
        .get()
}

fn engine() -> (EngineInstance, u64, u64, u32) {
    let (mut context, mut memory, mut call) = fixture();
    call.thread_id = 1;
    let atom = register(&mut context, &mut memory, call);
    let window = create(&mut context, &mut memory, call, atom);
    context
        .state
        .windows_mut()
        .update_description(
            WindowHandle32::try_from(window).unwrap(),
            crate::windows::window::WindowDescription32 {
                thread_id: 1,
                procedure: GuestAddress(CALLBACK_PC),
                extent: crate::windows::window::WindowExtent32 {
                    width: 640,
                    height: 480,
                },
                visible: false,
                enabled: true,
            },
        )
        .unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(CALL_PC), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(CALL_PC), &[0x0f, 0x0b]).unwrap();
    memory.write(GuestAddress(CALLBACK_PC), &[0x90]).unwrap();
    memory
        .write(GuestAddress(CALL_PC + 0x30), &[0x0f, 0x0b])
        .unwrap();
    memory
        .write(GuestAddress(CALL_PC + 0x40), &[0x0f, 0x0b])
        .unwrap();
    memory
        .write(GuestAddress(RETURN_PC), &[0x0f, 0x0b])
        .unwrap();
    let mut engine = EngineInstance::new(16, KEY).unwrap();
    engine.memory = Some(memory);
    engine.user32 = Some(context);
    engine.configure_cpu_clock(1_000_000, 0).unwrap();
    engine
        .configure_media(MediaConfig {
            input: InputLimits {
                queue_events: MIN_QUEUE_EVENTS,
                batch_events: MAX_BATCH_EVENTS,
            },
            ..Default::default()
        })
        .unwrap();
    let outer = unit(
        &mut engine,
        &[CALL_PC, 2, CALL_PC, User32Api32::SetFocus.id()],
        1,
        1,
    );
    let callback = unit(
        &mut engine,
        &[CALLBACK_PC, 1, RETURN_PC, 2, RETURN_PC, RETURN_ID],
        2,
        1,
    );
    (engine, outer, callback, window)
}

fn capture(engine: &mut EngineInstance, outer: u64, window: u32) -> u32 {
    let mut stack = [0; 8];
    stack[4..].copy_from_slice(&window.to_le_bytes());
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(STACK), &stack)
        .unwrap();
    stop(engine, CALL_PC, User32Api32::SetFocus.id(), STACK, 0);
    engine
        .capture_resident_call(KEY, outer, crate::windows::CallingConvention32::Stdcall, 1)
        .unwrap()
        .token
}

#[test]
fn shared_bridge_parks_real_guest_wndproc_then_finishes_original_stdcall_once() {
    let (mut engine, outer, callback, window) = engine();
    let token = capture(&mut engine, outer, window);
    engine
        .complete_resident_windows_call(KEY, outer, token)
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.abandon_call(KEY, token),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.abandon_call(KEY, token + 100),
        Err(HostError::Call(CallError::InvalidToken))
    );
    assert_eq!(engine.arena(), before);
    let request = engine.pending_windows_callback().unwrap();
    assert_eq!(request.procedure, GuestAddress(CALLBACK_PC));
    assert_eq!(request.count, 4);
    assert_eq!(&request.arguments[..4], &[window, 7, 0, 0]);
    assert_eq!(
        engine.complete_resident_call(KEY, outer, token, 99),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.begin_resident_callback(
            KEY,
            outer,
            callback,
            token,
            CALLBACK_PC,
            RETURN_PC,
            RETURN_ID,
            &request.arguments[..4]
        ),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    let record = engine
        .begin_windows_resident_callback(KEY, callback, RETURN_PC, RETURN_ID)
        .unwrap();
    engine
        .authorize_resident_callback(KEY, callback, record.token)
        .unwrap();
    stop(&mut engine, RETURN_PC, RETURN_ID, STACK, 0xdeadbeef);
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.finish_resident_callback(KEY, callback, record.token),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.abort_callback(KEY, record.token),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.abort_callback(KEY, record.token + 100),
        Err(HostError::Call(CallError::InvalidToken))
    );
    assert_eq!(engine.arena(), before);
    engine
        .finish_windows_resident_callback(KEY, callback, record.token)
        .unwrap();
    let state = decode_state(&engine.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]).unwrap();
    assert_eq!(state.registers[0], 0);
    assert_eq!(state.registers[4], STACK + 8);
    assert_eq!(state.eip, 0);
    assert!(engine.pending_call.is_none());
    assert!(engine.windows_replies.is_empty());
    assert_eq!(engine.pending_windows_callback(), None);
    assert_eq!(
        engine.finish_windows_resident_callback(KEY, callback, record.token),
        Err(HostError::Call(CallError::InvalidToken))
    );
}

#[test]
fn input_queue_full_keeps_hwnd_focus_and_captured_guest_frame_unchanged() {
    let (mut engine, outer, _, window) = engine();
    engine
        .user32
        .as_mut()
        .unwrap()
        .state
        .windows_mut()
        .set_focus(1, Some(WindowHandle32::try_from(window).unwrap()))
        .unwrap();
    engine
        .user32
        .as_mut()
        .unwrap()
        .state
        .windows_mut()
        .pop_message();
    let prepared = engine.prepare_windows_input_flags(true, false).unwrap();
    engine.commit_windows_input_flags(prepared);
    let mut events = vec![TimedInputEvent {
        timestamp_ns: 0,
        kind: InputKind::Focus { focused: true },
    }];
    for code in 0..crate::input::KEY_COUNT {
        events.push(TimedInputEvent {
            timestamp_ns: 0,
            kind: InputKind::Key {
                code: code as u16,
                pressed: true,
            },
        });
    }
    for button in 0..crate::input::BUTTON_COUNT {
        events.push(TimedInputEvent {
            timestamp_ns: 0,
            kind: InputKind::Button {
                button: button as u8,
                pressed: true,
            },
        });
    }
    let epoch = engine.media_epoch().unwrap();
    engine.submit_input(epoch, 1, &events).unwrap();
    let token = capture(&mut engine, outer, 0);
    let before = engine.arena().to_vec();
    let input = *engine.input().unwrap().state();
    assert_eq!(
        engine.complete_resident_windows_call(KEY, outer, token),
        Err(HostError::Media(MediaError::Input(
            crate::input::InputError::QueueFull
        )))
    );
    assert_eq!(
        engine.user32_windows().unwrap().focus().unwrap().get(),
        window
    );
    assert_eq!(*engine.input().unwrap().state(), input);
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    assert!(engine.windows_replies.is_empty());
    assert_eq!(
        engine.validate_windows_provider_owner(PendingOwner::Resident(outer)),
        Ok(())
    );
}

#[test]
fn waiting_getmessage_cannot_be_abandoned_and_close_tears_down_the_accepted_owner() {
    let (mut engine, _, _, _) = engine();
    let pc = 0x70030;
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(pc), &[0x0f, 0x0b])
        .unwrap();
    let unit = unit(
        &mut engine,
        &[pc, 2, pc, User32Api32::GetMessageA.id()],
        1,
        1,
    );
    let mut stack = [0; 20];
    stack[4..8].copy_from_slice(&0x10200u32.to_le_bytes());
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(STACK), &stack)
        .unwrap();
    stop(&mut engine, pc, User32Api32::GetMessageA.id(), STACK, 0);
    let token = engine
        .capture_resident_call(KEY, unit, crate::windows::CallingConvention32::Stdcall, 4)
        .unwrap()
        .token;
    engine
        .complete_resident_windows_call(KEY, unit, token)
        .unwrap();
    assert!(engine.waiting_user32_message().is_some());
    assert_eq!(
        engine.abandon_call(KEY, token),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.abandon_call(KEY, token + 100),
        Err(HostError::Call(CallError::InvalidToken))
    );
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    engine.close();
    assert!(engine.user32.is_none());
    assert!(engine.windows_replies.is_empty());
    assert!(engine.pending_call.is_none());
    assert!(engine.callback.is_none());
    assert!(!engine.is_open());
}

#[test]
fn user32_continuation_capacity_failure_drops_projected_input_receipt_without_fake_focus() {
    let (mut engine, outer, _, window) = engine();
    let epoch = engine.media_epoch().unwrap();
    engine
        .submit_input(
            epoch,
            1,
            &[TimedInputEvent {
                timestamp_ns: 0,
                kind: InputKind::Focus { focused: true },
            }],
        )
        .unwrap();
    assert!(!engine.input().unwrap().state().focused);
    engine.user32.as_mut().unwrap().continuation_limit = 0;
    let before = *engine.input().unwrap().state();
    let queued = engine.input().unwrap().queued_events();
    let token = capture(&mut engine, outer, window);
    engine
        .complete_resident_windows_call(KEY, outer, token)
        .unwrap();
    assert_eq!(engine.user32_windows().unwrap().focus(), None);
    assert_eq!(*engine.input().unwrap().state(), before);
    assert_eq!(engine.input().unwrap().queued_events(), queued);
    assert_eq!(engine.windows_thread.last_error(), 8);
}

#[test]
fn replacement_bridge_keeps_low_level_callback_paths_from_consuming_provider_ownership() {
    let (mut engine, _, _, window) = engine();
    for (index, value) in [
        CALL_PC,
        2,
        CALLBACK_PC,
        1,
        RETURN_PC,
        2,
        CALL_PC,
        User32Api32::SetFocus.id(),
        RETURN_PC,
        RETURN_ID,
    ]
    .iter()
    .enumerate()
    {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    let generation = engine.compile_with_gates(3, 2).unwrap();
    let mut stack = [0; 8];
    stack[4..].copy_from_slice(&window.to_le_bytes());
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(STACK), &stack)
        .unwrap();
    stop(&mut engine, CALL_PC, User32Api32::SetFocus.id(), STACK, 0);
    let token = engine
        .capture_call(
            KEY,
            generation,
            crate::windows::CallingConvention32::Stdcall,
            1,
        )
        .unwrap()
        .token;
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
    let request = engine.pending_windows_callback().unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.complete_call(KEY, generation, token, 99),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.begin_callback(
            KEY,
            generation,
            token,
            CALLBACK_PC,
            RETURN_PC,
            RETURN_ID,
            &request.arguments[..4]
        ),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    let record = engine
        .begin_windows_callback(KEY, generation, RETURN_PC, RETURN_ID)
        .unwrap();
    stop(&mut engine, RETURN_PC, RETURN_ID, STACK, 99);
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.finish_callback(KEY, generation, record.token),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.abort_callback(KEY, record.token),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    engine
        .finish_windows_callback(KEY, generation, record.token)
        .unwrap();
    assert!(engine.pending_call.is_none());
    assert!(engine.callback.is_none());
    assert!(engine.windows_replies.is_empty());
}

#[test]
fn hidden_show_resident_callbacks_publish_logical_focus_without_granting_physical_focus_or_capture()
{
    for host_focused in [false, true] {
        let (mut engine, _, callback_unit, window) = engine();
        let epoch = engine.media_epoch().unwrap();
        engine
            .submit_input(
                epoch,
                1,
                &[TimedInputEvent {
                    timestamp_ns: 0,
                    kind: InputKind::Focus {
                        focused: host_focused,
                    },
                }],
            )
            .unwrap();
        assert!(!engine.input().unwrap().state().focused);
        let pc = 0x70030;
        let outer = unit(
            &mut engine,
            &[pc, 2, pc, User32Api32::ShowWindow.id()],
            1,
            1,
        );
        let mut stack = [0; 12];
        stack[4..8].copy_from_slice(&window.to_le_bytes());
        stack[8..12].copy_from_slice(&5u32.to_le_bytes());
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(STACK), &stack)
            .unwrap();
        stop(&mut engine, pc, User32Api32::ShowWindow.id(), STACK, 0);
        let token = engine
            .capture_resident_call(KEY, outer, crate::windows::CallingConvention32::Stdcall, 2)
            .unwrap()
            .token;
        engine
            .complete_resident_windows_call(KEY, outer, token)
            .unwrap();
        for message in [0x18, 0x86, 6, 7] {
            let request = engine.pending_windows_callback().unwrap();
            assert_eq!(request.arguments[1], message);
            assert_eq!(
                engine.input().unwrap().state().focused,
                host_focused && message == 7
            );
            assert!(!engine.input().unwrap().state().captured);
            let record = engine
                .begin_windows_resident_callback(KEY, callback_unit, RETURN_PC, RETURN_ID)
                .unwrap();
            engine
                .authorize_resident_callback(KEY, callback_unit, record.token)
                .unwrap();
            stop(&mut engine, RETURN_PC, RETURN_ID, STACK, 0);
            engine
                .finish_windows_resident_callback(KEY, callback_unit, record.token)
                .unwrap();
        }
        assert_eq!(
            engine.user32_windows().unwrap().active().unwrap().get(),
            window
        );
        assert_eq!(
            engine.user32_windows().unwrap().focus().unwrap().get(),
            window
        );
        assert_eq!(engine.input().unwrap().state().focused, host_focused);
        assert!(!engine.input().unwrap().state().captured);
        assert!(engine.pending_call.is_none());
        assert!(engine.windows_replies.is_empty());
        let state = decode_state(&engine.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]).unwrap();
        assert_eq!(state.registers[0], 0);
        assert_eq!(state.registers[4], STACK + 12);
    }
}

#[test]
fn visible_creation_reuses_activation_callbacks_and_focus_receipt_after_scratch_reclamation() {
    let (mut engine, _, callback_unit, _) = engine();
    let epoch = engine.media_epoch().unwrap();
    engine
        .submit_input(
            epoch,
            1,
            &[TimedInputEvent {
                timestamp_ns: 0,
                kind: InputKind::Focus { focused: true },
            }],
        )
        .unwrap();
    let atom = engine
        .user32
        .as_mut()
        .unwrap()
        .state
        .register_class(crate::windows::user32::WindowClass32 {
            atom: 0,
            name: "VisibleWindow".encode_utf16().collect(),
            instance: 0x400000,
            procedure: GuestAddress(CALLBACK_PC),
            unicode: false,
            style: 0,
            extra_bytes: 0,
        })
        .unwrap();
    let pc = CALL_PC + 0x40;
    let outer = unit(
        &mut engine,
        &[pc, 2, pc, User32Api32::CreateWindowExA.id()],
        1,
        1,
    );
    let mut stack = [0; 52];
    for (index, value) in [
        0,
        u32::from(atom),
        0,
        0x90000000,
        10,
        20,
        640,
        480,
        0,
        0,
        0x400000,
        0,
    ]
    .into_iter()
    .enumerate()
    {
        stack[4 + index * 4..8 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(STACK), &stack)
        .unwrap();
    let mapped = engine.memory.as_ref().unwrap().mapped_pages();
    stop(&mut engine, pc, User32Api32::CreateWindowExA.id(), STACK, 0);
    let token = engine
        .capture_resident_windows_call(KEY, outer)
        .unwrap()
        .token;
    engine
        .complete_resident_windows_call(KEY, outer, token)
        .unwrap();
    let window = engine.pending_windows_callback().unwrap().arguments[0];
    for message in [0x81, 0x83, 1, 5, 3, 0x18, 0x86, 6, 7] {
        let request = engine.pending_windows_callback().unwrap();
        assert_eq!(&request.arguments[..2], &[window, message]);
        assert_eq!(engine.input().unwrap().state().focused, message == 7);
        if matches!(message, 0x86 | 6 | 7) {
            assert_eq!(engine.memory.as_ref().unwrap().mapped_pages(), mapped);
        }
        let record = engine
            .begin_windows_resident_callback(KEY, callback_unit, RETURN_PC, RETURN_ID)
            .unwrap();
        engine
            .authorize_resident_callback(KEY, callback_unit, record.token)
            .unwrap();
        stop(
            &mut engine,
            RETURN_PC,
            RETURN_ID,
            STACK,
            u32::from(message == 0x81),
        );
        engine
            .finish_windows_resident_callback(KEY, callback_unit, record.token)
            .unwrap();
    }
    assert_eq!(
        engine.user32_windows().unwrap().active().unwrap().get(),
        window
    );
    assert_eq!(
        engine.user32_windows().unwrap().focus().unwrap().get(),
        window
    );
    assert!(engine.input().unwrap().state().focused);
    assert!(!engine.input().unwrap().state().captured);
    assert_eq!(engine.memory.as_ref().unwrap().mapped_pages(), mapped);
    let state = decode_state(&engine.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]).unwrap();
    assert_eq!(state.registers[0], window);
    assert_eq!(state.registers[4], STACK + 52);
    assert!(engine.windows_replies.is_empty());
    assert!(engine.pending_call.is_none());
}
