use crate::{
    abi::{
        arena::{EXIT_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, decode_state, encode_exit_v3, encode_state_current},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::{GuestAddress, PageRange, Permissions},
    process::{
        EngineInstance,
        windows::user32::{User32CallContext32, User32Outcome32},
    },
    windows::{
        CallingConvention32, memory::MemoryApi32, user32::User32Api32 as Api,
        window::WindowHandle32,
    },
};

const KEY: u64 = 0x434c4950;
const CODE: u32 = 0x70000;
const PROCEDURE: u32 = CODE + 0x200;
const RETURN: u32 = CODE + 0x210;
const RETURN_ID: u32 = 0x0002_00ff;
const STACK: u32 = 0x11f00;

fn fixture() -> (EngineInstance, u32) {
    let (mut context, mut memory, mut call) = crate::process::windows::user32::tests::fixture();
    call.thread_id = 1;
    let atom = crate::process::windows::user32::tests::register(&mut context, &mut memory, call);
    let window =
        crate::process::windows::user32::tests::create(&mut context, &mut memory, call, atom);
    let description = crate::windows::window::WindowDescription32 {
        procedure: GuestAddress(PROCEDURE),
        ..context
            .windows()
            .get(WindowHandle32::try_from(window).unwrap())
            .unwrap()
            .description
    };
    context
        .state
        .windows_mut()
        .update_description(WindowHandle32::try_from(window).unwrap(), description)
        .unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(CODE), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    let mut engine = EngineInstance::new(16, KEY).unwrap();
    engine.memory = Some(memory);
    engine.user32 = Some(context);
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    for pc in [CODE, RETURN] {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(pc), &[0x0f, 0x0b])
            .unwrap();
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(PROCEDURE), &[0xc3])
        .unwrap();
    (engine, window)
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

fn capture_only(engine: &mut EngineInstance, id: u32, args: &[u32]) -> u32 {
    let pc = CODE;
    let descriptors: Vec<u8> = [
        CODE, 2, PROCEDURE, 1, RETURN, 2, CODE, id, RETURN, RETURN_ID,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + descriptors.len()]
        .copy_from_slice(&descriptors);
    engine.compile_with_gates(3, 2).unwrap();
    let bytes: Vec<u8> = std::iter::once(pc + 2)
        .chain(args.iter().copied())
        .flat_map(u32::to_le_bytes)
        .collect();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(STACK), &bytes)
        .unwrap();
    stop(engine, pc, id, 0);
    let token = engine
        .capture_call(
            KEY,
            engine.generation,
            CallingConvention32::Stdcall,
            args.len() as u32,
        )
        .unwrap()
        .token;
    token
}
fn begin(engine: &mut EngineInstance, id: u32, args: &[u32]) -> u32 {
    let token = capture_only(engine, id, args);
    engine
        .complete_windows_call(KEY, engine.generation, token)
        .unwrap();
    token
}

fn call(engine: &mut EngineInstance, id: u32, args: &[u32]) -> u32 {
    begin(engine, id, args);
    for _ in 0..4 {
        let Some(token) = engine.waiting_user32_message() else {
            break;
        };
        engine.resume_user32_wait(KEY, token).unwrap();
    }
    assert!(engine.pending_windows_callback().is_none());
    assert!(engine.pending_call.is_none());
    decode_state(&engine.arena()[..STATE_SIZE])
        .unwrap()
        .registers[0]
}

fn data(engine: &mut EngineInstance, bytes: &[u8]) -> u32 {
    let handle = call(
        engine,
        MemoryApi32::GlobalAlloc.id(),
        &[2, bytes.len() as u32],
    );
    assert_ne!(handle, 0);
    let memory = engine.windows_global_memory(handle).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(memory.pointer), bytes)
        .unwrap();
    handle
}

#[test]
fn captured_text_formats_transfer_global_ownership_and_retire_cached_conversions() {
    let (mut engine, _) = fixture();
    let handle = data(&mut engine, &[65, 0, 0xdf, 0, 0, 0, 0, 0]);
    assert_eq!(call(&mut engine, Api::OpenClipboard.id(), &[0]), 1);
    assert_eq!(call(&mut engine, Api::EmptyClipboard.id(), &[]), 1);
    let sequence = call(&mut engine, Api::GetClipboardSequenceNumber.id(), &[]);
    assert_eq!(
        call(&mut engine, Api::SetClipboardData.id(), &[13, handle]),
        handle
    );
    assert_eq!(call(&mut engine, Api::CountClipboardFormats.id(), &[]), 1);
    assert_eq!(
        call(&mut engine, Api::IsClipboardFormatAvailable.id(), &[1]),
        0
    );
    assert_eq!(call(&mut engine, Api::CloseClipboard.id(), &[]), 1);
    assert_eq!(call(&mut engine, Api::CountClipboardFormats.id(), &[]), 4);
    assert_eq!(
        call(&mut engine, Api::GetClipboardSequenceNumber.id(), &[]),
        sequence + 1
    );
    assert_eq!(
        call(&mut engine, MemoryApi32::GlobalFree.id(), &[handle]),
        handle
    );
    assert_eq!(engine.windows_thread.last_error(), 6);
    assert_eq!(call(&mut engine, Api::OpenClipboard.id(), &[0]), 1);
    let mut formats = Vec::new();
    let mut previous = 0;
    loop {
        previous = call(&mut engine, Api::EnumClipboardFormats.id(), &[previous]);
        if previous == 0 {
            break;
        }
        formats.push(previous);
    }
    assert_eq!(formats, [13, 16, 1, 7]);
    let ansi = call(&mut engine, Api::GetClipboardData.id(), &[1]);
    assert_ne!(ansi, handle);
    let owned = engine.windows_global_memory(ansi).unwrap();
    let mut bytes = [0; 3];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(owned.pointer), &mut bytes)
        .unwrap();
    assert_eq!(bytes, [65, 0xdf, 0]);
    assert_eq!(call(&mut engine, Api::GetClipboardData.id(), &[1]), ansi);
    let replacement = data(&mut engine, &[66, 0, 0, 0]);
    assert_eq!(
        call(&mut engine, Api::SetClipboardData.id(), &[13, replacement]),
        replacement
    );
    assert!(engine.windows_global_memory(handle).is_err());
    assert!(engine.windows_global_memory(ansi).is_err());
    assert_eq!(call(&mut engine, Api::CloseClipboard.id(), &[]), 1);
    assert_eq!(call(&mut engine, Api::OpenClipboard.id(), &[0]), 1);
    let newer = call(&mut engine, Api::GetClipboardData.id(), &[1]);
    assert_ne!(newer, ansi);
    let pointer = engine.windows_global_memory(newer).unwrap().pointer;
    let mut bytes = [0; 2];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(pointer), &mut bytes)
        .unwrap();
    assert_eq!(&bytes, b"B\0");
    assert_eq!(call(&mut engine, Api::EmptyClipboard.id(), &[]), 1);
    assert!(engine.windows_global_memory(newer).is_err());
    assert!(engine.windows_global_memory(replacement).is_err());
}

#[test]
fn captured_registration_and_rejected_set_preserve_names_content_and_sequence() {
    let (mut engine, _) = fixture();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x10200), b"Ring3 text\0")
        .unwrap();
    let format = call(&mut engine, Api::RegisterClipboardFormatA.id(), &[0x10200]);
    assert_eq!(format, 0xc000);
    assert_eq!(
        call(
            &mut engine,
            Api::GetClipboardFormatNameA.id(),
            &[format, 0x10300, 4]
        ),
        3
    );
    let mut bytes = [0; 4];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x10300), &mut bytes)
        .unwrap();
    assert_eq!(&bytes, b"Rin\0");
    assert_eq!(call(&mut engine, Api::OpenClipboard.id(), &[0]), 1);
    assert_eq!(call(&mut engine, Api::EmptyClipboard.id(), &[]), 1);
    let sequence = call(&mut engine, Api::GetClipboardSequenceNumber.id(), &[]);
    assert_eq!(
        call(
            &mut engine,
            Api::SetClipboardData.id(),
            &[format, 0xdeadbeef]
        ),
        0
    );
    assert_eq!(engine.windows_thread.last_error(), 6);
    assert_eq!(
        call(&mut engine, Api::GetClipboardSequenceNumber.id(), &[]),
        sequence
    );
    assert_eq!(call(&mut engine, Api::CountClipboardFormats.id(), &[]), 0);
    assert_eq!(call(&mut engine, Api::CloseClipboard.id(), &[]), 1);
    assert_eq!(call(&mut engine, Api::CloseClipboard.id(), &[]), 0);
    assert_eq!(engine.windows_thread.last_error(), 1418);
}

#[test]
fn captured_clipboard_query_errors_match_native_windows_without_mutating_content() {
    let (mut engine, _) = fixture();
    const CANARY: u32 = 0x12345678;
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x10200), b"Ring3 query errors\0")
        .unwrap();
    let format = call(&mut engine, Api::RegisterClipboardFormatA.id(), &[0x10200]);
    assert_eq!(call(&mut engine, Api::OpenClipboard.id(), &[0]), 1);
    assert_eq!(call(&mut engine, Api::EmptyClipboard.id(), &[]), 1);
    let sequence = call(&mut engine, Api::GetClipboardSequenceNumber.id(), &[]);
    for (api, args, expected) in [
        (Api::GetClipboardFormatNameA, vec![13, 0x10300, 16], 87),
        (Api::GetClipboardFormatNameA, vec![format, 0x10300, 0], 122),
        (Api::EnumClipboardFormats, vec![0], 0),
        (Api::EnumClipboardFormats, vec![1], 0),
        (Api::SetClipboardData, vec![0, 0], 1418),
    ] {
        engine.windows_thread = engine.windows_thread.with_last_error(CANARY);
        assert_eq!(call(&mut engine, api.id(), &args), 0, "{api:?}");
        assert_eq!(engine.windows_thread.last_error(), expected, "{api:?}");
        assert_eq!(call(&mut engine, Api::CountClipboardFormats.id(), &[]), 0);
        assert_eq!(
            call(&mut engine, Api::GetClipboardSequenceNumber.id(), &[]),
            sequence
        );
    }
    engine.windows_thread = engine.windows_thread.with_last_error(CANARY);
    assert_eq!(
        call(&mut engine, Api::GetPriorityClipboardFormat.id(), &[0, 0]),
        0
    );
    assert_eq!(engine.windows_thread.last_error(), CANARY);
}

#[test]
fn delayed_rendering_requires_the_actual_owned_callback_and_returns_its_materialized_data() {
    let (mut engine, window) = fixture();
    let handle = data(&mut engine, &[65, 0, 0, 0]);
    assert_eq!(call(&mut engine, Api::OpenClipboard.id(), &[window]), 1);
    assert_eq!(call(&mut engine, Api::EmptyClipboard.id(), &[]), 1);
    assert_eq!(call(&mut engine, Api::SetClipboardData.id(), &[13, 0]), 0);
    assert_eq!(call(&mut engine, Api::CloseClipboard.id(), &[]), 1);
    assert!(!engine.current_clipboard_renderer(13));
    assert_eq!(call(&mut engine, Api::OpenClipboard.id(), &[0]), 1);
    let outer = begin(&mut engine, Api::GetClipboardData.id(), &[13]);
    let frame = engine.pending_call.as_ref().unwrap().frame;
    let wait = engine.waiting_user32_message().unwrap();
    engine.resume_user32_wait(KEY, wait).unwrap();
    let request = engine.pending_windows_callback().unwrap();
    assert_eq!(&request.arguments[..4], &[window, 0x305, 13, 0]);
    assert!(!engine.current_clipboard_renderer(13));
    let callback = engine
        .begin_windows_callback(KEY, engine.generation, RETURN, RETURN_ID)
        .unwrap();
    assert!(engine.current_clipboard_renderer(13));
    assert_eq!(
        engine
            .execute_clipboard_call(
                Api::SetClipboardData,
                &[13, handle],
                User32CallContext32 {
                    thread_id: 1,
                    ..Default::default()
                }
            )
            .unwrap(),
        User32Outcome32::Return(handle)
    );
    stop(&mut engine, RETURN, RETURN_ID, 0);
    engine
        .finish_windows_callback(KEY, engine.generation, callback.token)
        .unwrap();
    assert_eq!(engine.pending_call.as_ref().unwrap().token, outer);
    assert_eq!(engine.pending_call.as_ref().unwrap().frame, frame);
    let wait = engine.waiting_user32_message().unwrap();
    engine.resume_user32_wait(KEY, wait).unwrap();
    assert!(engine.pending_call.is_none());
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        handle
    );
    assert!(!engine.current_clipboard_renderer(13));
}

mod graphics;
