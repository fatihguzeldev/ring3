use crate::{
    abi::{arena::*, x86::*},
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::{GuestAddress, PageRange, Permissions},
    process::{CallError, EngineInstance, HostError, windows::gdi::GdiConfig32},
    windows::{
        CallingConvention32,
        gdi::{GdiApi32, Region32},
        user32::{Rect32, User32Api32, User32Limits32},
        window::WindowHandle32,
    },
};

pub(super) const KEY: u64 = 7;
const PC: u32 = 0x10000;
pub(super) const PROC: u32 = 0x10010;
pub(super) const RETURN: u32 = 0x10020;
pub(super) const RETURN_ID: u32 = 0x200ff;
const STACK: u32 = 0x11f00;
const PAINT: u32 = 0x12fe0;

pub(super) fn stop(engine: &mut EngineInstance, pc: u32, id: u32, result: u32) {
    let mut state = State32 {
        eip: pc,
        ..Default::default()
    };
    state.registers[0] = result;
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
pub(super) fn unit(engine: &mut EngineInstance, words: &[u32], blocks: u32, gates: u32) -> u64 {
    for (index, word) in words.iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    engine
        .compile_resident_with_gates(blocks, gates)
        .unwrap()
        .get()
}
pub(super) fn capture(engine: &mut EngineInstance, id: u32, args: &[u32]) -> (u64, u32) {
    let pc = PC
        + 0x100
        + engine
            .resident
            .as_ref()
            .map_or(0, |registry| registry.usage().units as u32)
            * 4;
    let owner = unit(engine, &[pc, 2, pc, id], 1, 1);
    let bytes = std::iter::once(pc + 2)
        .chain(args.iter().copied())
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(STACK), &bytes)
        .unwrap();
    stop(engine, pc, id, 0);
    let token = engine
        .capture_resident_call(KEY, owner, CallingConvention32::Stdcall, args.len() as u32)
        .unwrap()
        .token;
    engine
        .complete_resident_windows_call(KEY, owner, token)
        .unwrap();
    (owner, token)
}
pub(super) fn eax(engine: &EngineInstance) -> u32 {
    decode_state(&engine.arena()[..STATE_SIZE])
        .unwrap()
        .registers[0]
}
fn words<const N: usize>(engine: &EngineInstance, address: u32) -> [u32; N] {
    let mut bytes = vec![0; N * 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    std::array::from_fn(|index| {
        u32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
    })
}
pub(super) fn finish(engine: &mut EngineInstance, callback_unit: u64, result: u32) {
    let record = engine
        .begin_windows_resident_callback(KEY, callback_unit, RETURN, RETURN_ID)
        .unwrap();
    engine
        .authorize_resident_callback(KEY, callback_unit, record.token)
        .unwrap();
    stop(engine, RETURN, RETURN_ID, result);
    engine
        .finish_windows_resident_callback(KEY, callback_unit, record.token)
        .unwrap();
}
fn fixture() -> (EngineInstance, u64, u32) {
    fixture_with_startup(false)
}
pub(super) fn fixture_with_startup(startup: bool) -> (EngineInstance, u64, u32) {
    let mut engine = EngineInstance::new(if startup { 256 } else { 32 }, KEY).unwrap();
    engine
        .configure_execution(crate::process::ExecutionConfig32 {
            units: 32,
            ..Default::default()
        })
        .unwrap();
    engine.configure_cpu_clock(1_000_000, 0).unwrap();
    engine.configure_media(Default::default()).unwrap();
    engine
        .configure_user32(
            User32Limits32 {
                classes: 4,
                windows: 4,
                messages: 16,
                continuations: 8,
            },
            PageRange::new(GuestAddress(0x40000), 8).unwrap(),
        )
        .unwrap();
    engine
        .configure_gdi(GdiConfig32 {
            bitmap_base: 0x20000,
            bitmap_pages: 8,
            bitmap_bytes: 65_536,
            screen_width: 8,
            screen_height: 6,
            ..Default::default()
        })
        .unwrap();
    if startup {
        engine.register_kernel32_module(0x70000000).unwrap();
        engine.register_user32_module(0x71000000).unwrap();
        engine
            .register_guest_module(
                "control-paint.exe",
                &crate::process::windows::process::test_image_for_startup(),
            )
            .unwrap();
        engine
            .start_module_process("control-paint.exe", None, Default::default())
            .unwrap();
        engine.load_library_module("user32.dll", None).unwrap();
    }
    engine.map(PC, 4, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(PC), &[0x0f, 0x0b, 0x90, 0x90].repeat(1024))
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(PROC), &[0x90])
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(RETURN), &[0x0f, 0x0b])
        .unwrap();
    engine.protect(PC, 1, 5).unwrap();
    let callback = unit(&mut engine, &[PROC, 1, RETURN, 2, RETURN, RETURN_ID], 2, 1);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x12100), b"PaintProof\0")
        .unwrap();
    let mut class = [0u8; 40];
    for (index, value) in [(1, PROC), (4, 0x400000), (7, 16), (9, 0x12100)] {
        class[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x12000), &class)
        .unwrap();
    capture(&mut engine, User32Api32::RegisterClassA.id(), &[0x12000]);
    let atom = eax(&engine);
    assert!(atom >= 0xc000);
    capture(
        &mut engine,
        User32Api32::CreateWindowExA.id(),
        &[0, atom, 0, 0x90000000, 0, 0, 8, 6, 0, 0, 0x400000, 0],
    );
    while let Some(request) = engine.pending_windows_callback() {
        finish(
            &mut engine,
            callback,
            u32::from(request.arguments[1] == 0x81),
        );
    }
    let window = eax(&engine);
    assert_ne!(window, 0);
    (engine, callback, window)
}
#[test]
fn captured_beginpaint_erases_with_real_wndproc_then_endpaint_releases_dc() {
    let (mut engine, callback, window) = fixture();
    let (_, token) = capture(&mut engine, GdiApi32::BeginPaint.id(), &[window, PAINT]);
    let request = engine.pending_windows_callback().unwrap();
    let [dc, erase, left, top, right, bottom] = words(&engine, PAINT);
    assert_ne!(dc, 0);
    assert_eq!([erase, left, top, right, bottom], [1, 0, 0, 8, 6]);
    assert_eq!(request.procedure.0, PROC);
    assert_eq!(request.count, 4);
    assert_eq!(&request.arguments[..4], &[window, 0x14, dc, 0]);
    assert_eq!(&request.arguments[4..], &[0; 12]);
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.abandon_call(KEY, token),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    finish(&mut engine, callback, 1);
    assert_eq!(eax(&engine), dc);
    assert_eq!(words::<1>(&engine, PAINT + 4), [0]);
    capture(
        &mut engine,
        GdiApi32::GetUpdateRect.id(),
        &[window, 0x12200, 0],
    );
    assert_eq!(eax(&engine), 0);
    assert_eq!(words::<4>(&engine, 0x12200), [0; 4]);
    capture(&mut engine, GdiApi32::EndPaint.id(), &[window, PAINT]);
    assert_eq!(eax(&engine), 1);
    capture(&mut engine, GdiApi32::GetObjectType.id(), &[dc]);
    assert_eq!(eax(&engine), 0);
}
#[test]
fn invalidation_retrieves_paint_and_wakes_the_exact_waiting_getmessage() {
    let (mut engine, callback, window) = fixture();
    capture(&mut engine, GdiApi32::ValidateRect.id(), &[window, 0]);
    capture(
        &mut engine,
        User32Api32::GetMessageW.id(),
        &[0x12300, window, 0x0f, 0x0f],
    );
    let continuation = engine.waiting_user32_message().unwrap();
    let token = engine.pending_call.as_ref().unwrap().token;
    let handle = WindowHandle32::try_from(window).unwrap();
    let queue = engine.prepare_user32_queue_change_for(1).unwrap();
    engine
        .user32
        .as_mut()
        .unwrap()
        .gdi_set_update_region(
            handle,
            Region32::rectangle(Rect32 {
                left: 1,
                top: 2,
                right: 4,
                bottom: 5,
            }),
            false,
        )
        .unwrap();
    engine.commit_user32_queue_change(queue);
    engine.resume_user32_wait(KEY, continuation).unwrap();
    assert!(engine.pending_call.is_none());
    assert_eq!(eax(&engine), 1);
    assert_eq!(words::<2>(&engine, 0x12300), [window, 0x0f]);
    assert_eq!(
        engine.abandon_call(KEY, token),
        Err(HostError::Call(CallError::InvalidToken))
    );
    capture(&mut engine, GdiApi32::BeginPaint.id(), &[window, PAINT]);
    assert!(engine.pending_windows_callback().is_none());
    assert_eq!(words::<5>(&engine, PAINT + 4), [0, 1, 2, 4, 5]);
    capture(&mut engine, GdiApi32::EndPaint.id(), &[window, PAINT]);
    capture(&mut engine, GdiApi32::InvalidateRect.id(), &[window, 0, 0]);
    assert_eq!(eax(&engine), 1);
    capture(&mut engine, GdiApi32::UpdateWindow.id(), &[window]);
    let request = engine.pending_windows_callback().unwrap();
    assert_eq!(request.count, 4);
    assert_eq!(&request.arguments[..4], &[window, 0x0f, 0, 0]);
    assert_eq!(&request.arguments[4..], &[0; 12]);
    finish(&mut engine, callback, 0);
    assert_eq!(eax(&engine), 1);
}
#[test]
fn paintstruct_fault_retains_result_for_exact_outer_retry_and_rejects_remap() {
    for remap in [false, true] {
        let (mut engine, callback, window) = fixture();
        let (owner, outer_token) =
            capture(&mut engine, GdiApi32::BeginPaint.id(), &[window, PAINT]);
        let outer_frame = engine.pending_call.as_ref().unwrap().frame;
        let outer_state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
        let request = engine.pending_windows_callback().unwrap();
        let record = engine
            .begin_windows_resident_callback(KEY, callback, RETURN, RETURN_ID)
            .unwrap();
        engine
            .authorize_resident_callback(KEY, callback, record.token)
            .unwrap();
        stop(&mut engine, RETURN, RETURN_ID, 1);
        let range = PageRange::new(GuestAddress(0x13000), 1).unwrap();
        if remap {
            engine.memory.as_mut().unwrap().unmap(range).unwrap();
            engine
                .memory
                .as_mut()
                .unwrap()
                .map_zeroed(range, Permissions::READ_WRITE)
                .unwrap();
        } else {
            engine
                .memory
                .as_mut()
                .unwrap()
                .protect(range, Permissions::READ)
                .unwrap();
        }
        let bytes = words::<8>(&engine, PAINT);
        let version = engine.memory().unwrap().transaction_version();
        assert!(
            engine
                .finish_windows_resident_callback(KEY, callback, record.token)
                .is_err()
        );
        assert_eq!(
            decode_state(&engine.arena()[..STATE_SIZE]).unwrap(),
            outer_state
        );
        assert_eq!(engine.pending_call.as_ref().unwrap().frame, outer_frame);
        assert_eq!(engine.pending_call.as_ref().unwrap().token, outer_token);
        assert_eq!(
            engine.pending_call.as_ref().unwrap().owner,
            crate::process::calls::call::PendingOwner::Resident(owner)
        );
        assert!(engine.callback.is_none());
        assert!(engine.pending_windows_callback().is_none());
        assert_eq!(
            engine.windows_replies.last().unwrap().received_result(
                crate::process::windows::callbacks::WindowsCallbackProvider32::User32,
                crate::process::calls::call::PendingOwner::Resident(owner),
                outer_token,
                request.continuation,
                outer_frame
            ),
            Some(1)
        );
        let arena = engine.arena().to_vec();
        assert!(
            engine
                .complete_resident_windows_call(KEY, owner, outer_token)
                .is_err()
        );
        assert_eq!(engine.arena(), arena);
        assert_eq!(words::<8>(&engine, PAINT), bytes);
        assert_eq!(engine.memory().unwrap().transaction_version(), version);
        if !remap {
            engine
                .memory
                .as_mut()
                .unwrap()
                .protect(range, Permissions::READ_WRITE)
                .unwrap();
            engine
                .complete_resident_windows_call(KEY, owner, outer_token)
                .unwrap();
            assert_eq!(words::<1>(&engine, PAINT + 4), [0]);
            assert!(engine.pending_call.is_none());
        }
    }
}
