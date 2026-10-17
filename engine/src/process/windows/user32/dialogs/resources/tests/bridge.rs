use super::*;
use crate::{
    input::{InputError, InputKind, InputLimits, MIN_QUEUE_EVENTS, TimedInputEvent},
    process::{MediaConfig, MediaError},
    windows::{
        thread::{ThreadSchedule32, ThreadState32},
        user32::User32Api32 as Api,
    },
};

const RETURN: u32 = CODE + 0x120;
const RETURN_ID: u32 = 0x0002_00ff;
const VALIDATE: u32 = CODE + 0x140;

fn write_template(engine: &mut EngineInstance) {
    let mut bytes: Vec<u8> = [0u16, 0x8000, 0, 0, 1, 0, 0, 100, 50, 0, 0, b'D' as u16, 0]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
    bytes.resize(bytes.len().next_multiple_of(4), 0);
    bytes.extend(
        [
            0u16,
            0x5001,
            0,
            0,
            4,
            8,
            20,
            12,
            100,
            0xffff,
            0x80,
            b'C' as u16,
            0,
            0,
        ]
        .into_iter()
        .flat_map(u16::to_le_bytes),
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(DATA), &bytes)
        .unwrap();
}

fn stop(engine: &mut EngineInstance, pc: u32, id: u32, value: u32) {
    let mut state = State32 {
        eip: pc,
        ..Default::default()
    };
    state.registers[0] = value;
    state.registers[4] = STACK;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
}

fn begin(engine: &mut EngineInstance, modal: bool) -> u32 {
    let api = if modal {
        Api::DialogBoxIndirectParamW
    } else {
        Api::CreateDialogIndirectParamW
    };
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(RETURN), &[0x0f, 0x0b])
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(VALIDATE), &[0x0f, 0x0b])
        .unwrap();
    let mut blocks = vec![(CODE, 2), (CODE + 0x100, 3), (RETURN, 2), (VALIDATE, 2)];
    let mut gates = vec![
        (CODE, api.id()),
        (RETURN, RETURN_ID),
        (VALIDATE, crate::windows::gdi::GdiApi32::ValidateRect.id()),
    ];
    for gate in engine.module_gates().unwrap().into_iter().filter(|gate| {
        matches!(
            Api::from_id(gate.id),
            Some(Api::DefDlgProcW | Api::DefWindowProcW)
        ) || gate.id == crate::windows::controls::BUILTIN_PROC_W
            || gate.id == crate::process::windows::threads::INVOCATION_RETURN_ID
    }) {
        blocks.push((gate.entry.0, 2));
        gates.push((gate.entry.0, gate.id));
    }
    let descriptors: Vec<u8> = blocks
        .iter()
        .chain(&gates)
        .flat_map(|&(first, second)| [first, second])
        .flat_map(u32::to_le_bytes)
        .collect();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + descriptors.len()]
        .copy_from_slice(&descriptors);
    engine
        .compile_with_gates(blocks.len() as u32, gates.len() as u32)
        .unwrap();
    let words: Vec<u8> = [CODE + 2, 0x400000, DATA, 0, CODE + 0x100, 0xaabbccdd]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(STACK), &words)
        .unwrap();
    stop(engine, CODE, api.id(), 0);
    let token = engine
        .capture_call(KEY, engine.generation, CallingConvention32::Stdcall, 5)
        .unwrap()
        .token;
    engine
        .complete_windows_call(KEY, engine.generation, token)
        .unwrap();
    token
}

fn complete_callback(engine: &mut EngineInstance, value: u32) -> Result<(), HostError> {
    let request = engine.pending_windows_callback().unwrap();
    let callback = engine.begin_windows_callback(KEY, engine.generation, RETURN, RETURN_ID)?;
    if request.arguments[1] == 0xf {
        // a real paint recipient validates its update region before returning.
        let words = [CODE + 0x100, request.arguments[0], 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(STACK), &words)
            .map_err(HostError::Memory)?;
        let id = crate::windows::gdi::GdiApi32::ValidateRect.id();
        stop(engine, VALIDATE, id, 0);
        let call = engine.capture_call(KEY, engine.generation, CallingConvention32::Stdcall, 2)?;
        engine.complete_windows_call(KEY, engine.generation, call.token)?;
    }
    stop(engine, RETURN, RETURN_ID, value);
    engine.finish_windows_callback(KEY, engine.generation, callback.token)?;
    Ok(())
}

fn reach_init(engine: &mut EngineInstance) -> u32 {
    for _ in 0..64 {
        let request = engine
            .pending_windows_callback()
            .expect("dialog initialization is a real callback");
        if request.arguments[1] == 0x110 {
            return request.arguments[0];
        }
        complete_callback(engine, u32::from(request.arguments[1] == 0x81)).unwrap();
    }
    panic!("dialog initialization was skipped");
}

#[test]
fn init_focus_preflights_full_input_queue_and_retries_only_the_sealed_callback_result() {
    let mut engine = engine();
    engine
        .configure_media(MediaConfig {
            input: InputLimits {
                queue_events: MIN_QUEUE_EVENTS,
                batch_events: MIN_QUEUE_EVENTS,
            },
            ..Default::default()
        })
        .unwrap();
    write_template(&mut engine);
    let token = begin(&mut engine, false);
    let frame = engine.pending_call.as_ref().unwrap().frame;
    let root = reach_init(&mut engine);
    let first = engine
        .user32
        .as_ref()
        .unwrap()
        .state
        .dialog_item(WindowHandle32::try_from(root).unwrap(), 100)
        .unwrap()
        .unwrap();
    let events = vec![
        TimedInputEvent {
            timestamp_ns: 0,
            kind: InputKind::Focus { focused: true }
        };
        MIN_QUEUE_EVENTS
    ];
    engine
        .submit_input(engine.media_epoch().unwrap(), 1, &events)
        .unwrap();
    assert_eq!(engine.input().unwrap().queued_events(), MIN_QUEUE_EVENTS);
    assert!(!engine.input().unwrap().state().focused);
    assert_eq!(engine.user32_windows().unwrap().focus(), None);
    let callback = engine
        .begin_windows_callback(KEY, engine.generation, RETURN, RETURN_ID)
        .unwrap();
    stop(&mut engine, RETURN, RETURN_ID, 1);
    let version = engine.memory.as_ref().unwrap().transaction_version();
    let accepted = engine.finish_windows_callback(KEY, engine.generation, callback.token);
    assert_eq!(
        engine.memory.as_ref().unwrap().transaction_version(),
        version
    );
    assert_eq!(engine.user32_windows().unwrap().focus(), None);
    assert!(!engine.input().unwrap().state().focused);
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    assert_eq!(engine.pending_call.as_ref().unwrap().frame, frame);
    assert!(engine.callback.is_none());
    match accepted {
        Ok(_) => {
            let wait = engine.waiting_user32_message().unwrap();
            assert_eq!(
                engine.resume_user32_wait(KEY, wait),
                Err(HostError::Media(MediaError::Input(InputError::QueueFull)))
            );
        }
        Err(error) => assert_eq!(
            error,
            HostError::Media(MediaError::Input(InputError::QueueFull))
        ),
    }
    assert_eq!(engine.user32_windows().unwrap().focus(), None);
    assert_eq!(
        engine.memory.as_ref().unwrap().transaction_version(),
        version
    );
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    assert_eq!(engine.pending_call.as_ref().unwrap().frame, frame);
    engine.input_consume(MIN_QUEUE_EVENTS).unwrap();
    if let Some(wait) = engine.waiting_user32_message() {
        engine.resume_user32_wait(KEY, wait).unwrap();
    } else {
        engine
            .complete_windows_call(KEY, engine.generation, token)
            .unwrap();
    }
    assert_eq!(engine.user32_windows().unwrap().focus(), Some(first));
    assert!(engine.input().unwrap().state().focused);
    assert_eq!(engine.pending_windows_callback().unwrap().arguments[1], 7);
}

#[test]
fn modal_message_wait_accepts_real_cross_thread_send_before_posted_message_retrieval() {
    let mut engine = engine();
    call(
        &mut engine,
        crate::windows::thread::ThreadApi32::CreateThread.id(),
        &[0, 0, CODE + 0x100, 0, 0, 0],
    )
    .unwrap();
    assert_eq!(
        engine.windows_thread_info(2).unwrap().state,
        ThreadState32::Runnable
    );
    write_template(&mut engine);
    begin(&mut engine, true);
    let root = reach_init(&mut engine);
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
    call(&mut engine, Api::SendMessageW.id(), &[root, 0x400, 3, 4]).unwrap();
    assert!(engine.has_pending_thread_invocation(1));
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    assert_eq!(
        engine.windows_thread_info(2).unwrap().state,
        ThreadState32::Waiting
    );
    assert!(!engine.has_pending_thread_invocation(1));
    let state = crate::abi::x86::decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    let frame = crate::process::windows::user32::memory::words::<5>(
        engine.memory.as_ref().unwrap(),
        state.registers[4],
    )
    .unwrap();
    assert_eq!(&frame[1..], &[root, 0x400, 3, 4]);
}

#[test]
fn cross_thread_accelerator_command_returns_only_after_the_real_owner_invocation() {
    let mut engine = engine();
    call(
        &mut engine,
        crate::windows::thread::ThreadApi32::CreateThread.id(),
        &[0, 0, CODE + 0x100, 0, 0, 0],
    )
    .unwrap();
    write_template(&mut engine);
    begin(&mut engine, true);
    let root = reach_init(&mut engine);
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
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(DATA + 0x400), &[1, 0, 65, 0, 37, 0])
        .unwrap();
    call(
        &mut engine,
        Api::CreateAcceleratorTableW.id(),
        &[DATA + 0x400, 1],
    )
    .unwrap();
    let table = crate::abi::x86::decode_state(&engine.arena()[..STATE_SIZE])
        .unwrap()
        .registers[0];
    let message: Vec<u8> = [root, 0x100, 65, 0, 0, 0, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(DATA + 0x500), &message)
        .unwrap();
    call(
        &mut engine,
        Api::TranslateAcceleratorW.id(),
        &[root, table, DATA + 0x500],
    )
    .unwrap();
    let outer = engine.pending_call.as_ref().unwrap().token;
    let outer_frame = engine.pending_call.as_ref().unwrap().frame;
    let wait = engine.waiting_user32_message().unwrap();
    engine.resume_user32_wait(KEY, wait).unwrap();
    assert_eq!(engine.pending_call.as_ref().unwrap().token, outer);
    assert_eq!(engine.pending_call.as_ref().unwrap().frame, outer_frame);
    assert!(engine.has_pending_thread_invocation(1));
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        ThreadSchedule32::Running { thread: 1 }
    );
    let mut state = crate::abi::x86::decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    let frame = crate::process::windows::user32::memory::words::<5>(
        engine.memory.as_ref().unwrap(),
        state.registers[4],
    )
    .unwrap();
    assert_eq!(&frame[1..], &[root, 0x111, 0x10025, 0]);
    assert!(!engine.has_pending_thread_invocation(1));
    state.eip = frame[0];
    state.registers[0] = 0xdeadbeef;
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
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
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
    let mut resumed_sender = false;
    for _ in 0..16 {
        match engine.schedule_windows_thread().unwrap() {
            ThreadSchedule32::Running { thread: 2 } => {
                resumed_sender = true;
                break;
            }
            ThreadSchedule32::WindowsCallback { thread: 1 } => {
                let request = engine.pending_windows_callback().unwrap();
                assert_eq!(
                    request.arguments[1], 0x0f,
                    "only pending modal paint may precede sender resumption"
                );
                let window = WindowHandle32::try_from(request.arguments[0]).unwrap();
                let context = engine.user32.as_ref().unwrap();
                let child = context
                    .state
                    .dialog_item(WindowHandle32::try_from(root).unwrap(), 100)
                    .unwrap();
                assert!(window.get() == root || Some(window) == child);
                complete_callback(&mut engine, 0).unwrap();
            }
            other => panic!("unexpected schedule after the accepted owner command: {other:?}"),
        }
    }
    assert!(
        resumed_sender,
        "the completed owner invocation makes its exact sender runnable"
    );
    if let Some(pending) = engine.pending_call.as_ref() {
        assert_eq!(pending.token, outer);
        assert_eq!(pending.frame, outer_frame);
    }
    for _ in 0..4 {
        let Some(wait) = engine.waiting_user32_message() else {
            break;
        };
        engine.resume_user32_wait(KEY, wait).unwrap();
    }
    assert!(engine.pending_call.is_none());
    assert_eq!(
        crate::abi::x86::decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        1
    );
    assert!(!engine.has_pending_thread_invocation(1));
    assert_eq!(
        engine.user32.as_ref().unwrap().wait_queue_status(1).0 & 0x40,
        0
    );
}

#[path = "bridge/queue.rs"]
mod queue;
