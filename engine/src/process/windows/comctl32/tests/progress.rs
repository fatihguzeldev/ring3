use super::*;
use crate::windows::comctl32::{COMMON_PROC_W, CommonControlsApi32};

const COMMON_RETURN: u32 = 0x10040;

fn callbacks(engine: &mut EngineInstance, parent: u64, common: u64, procedure: u32) {
    for _ in 0..64 {
        if engine.pending_windows_callback().is_some() {
            callback(engine, parent, common, procedure);
        } else if let Some(wait) = engine.waiting_user32_message() {
            engine.resume_user32_wait(KEY, wait).unwrap();
        } else {
            assert!(engine.pending_call.is_none());
            return;
        }
    }
    panic!("common-control callbacks must finish within their bounded creation/message chain");
}

fn callback(engine: &mut EngineInstance, parent: u64, common: u64, procedure: u32) {
    callback_observing(engine, parent, common, procedure, &mut |_, _| {});
}

fn callback_observing(
    engine: &mut EngineInstance,
    parent: u64,
    common: u64,
    procedure: u32,
    observe: &mut impl FnMut(&EngineInstance, &[u32]),
) {
    callback_returning(engine, parent, common, procedure, &mut |engine, args| {
        observe(engine, args);
        None
    });
}

fn callback_returning(
    engine: &mut EngineInstance,
    parent: u64,
    common: u64,
    procedure: u32,
    observe: &mut impl FnMut(&EngineInstance, &[u32]) -> Option<u32>,
) {
    let request = engine.pending_windows_callback().unwrap();
    if request.procedure.0 == PROC {
        let result = observe(engine, &request.arguments[..request.count as usize])
            .unwrap_or(u32::from(request.arguments[1] == 0x81));
        finish(engine, parent, result);
        return;
    }
    assert_eq!(request.procedure.0, procedure);
    let return_esp = engine
        .pending_call
        .as_ref()
        .unwrap()
        .frame
        .state()
        .registers[4];
    let record = engine
        .begin_windows_resident_callback(KEY, common, COMMON_RETURN, RETURN_ID)
        .unwrap();
    engine
        .authorize_resident_callback(KEY, common, record.token)
        .unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: COMMON_PROC_W },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = engine
        .capture_resident_callback_call(KEY, common, record.token, CallingConvention32::Stdcall, 4)
        .unwrap()
        .token;
    engine
        .complete_active_resident_callback_windows_call(KEY, common, record.token, token)
        .unwrap();
    for _ in 0..64 {
        if engine.pending_call.is_none() {
            break;
        }
        if engine.pending_windows_callback().is_some() {
            callback_returning(engine, parent, common, procedure, observe);
        } else if let Some(wait) = engine.waiting_user32_message() {
            engine.resume_user32_wait(KEY, wait).unwrap();
        } else {
            panic!("a parked common procedure must expose its next real callback or wait");
        }
    }
    assert!(engine.pending_call.is_none());
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    assert_eq!(state.registers[4], return_esp);
    state.eip = COMMON_RETURN;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: RETURN_ID },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    engine
        .finish_windows_resident_callback(KEY, common, record.token)
        .unwrap();
}

fn fixture() -> (EngineInstance, u64, u64, u32, u32, u32) {
    let (mut engine, parent, window) = fixture_with_startup(true);
    engine.register_comctl32_module(0x72000000).unwrap();
    engine.load_library_module("comctl32.dll", None).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x12300), &[8, 0, 0, 0, 0x20, 0, 0, 0])
        .unwrap();
    capture(
        &mut engine,
        CommonControlsApi32::InitCommonControlsEx.id(),
        &[0x12300],
    );
    assert_eq!(eax(&engine), 1);
    let procedure = engine
        .loaded_windows_modules()
        .flat_map(|module| module.gates())
        .find(|gate| gate.id == COMMON_PROC_W)
        .unwrap()
        .entry
        .0;
    let common = unit(
        &mut engine,
        &[
            procedure,
            2,
            COMMON_RETURN,
            2,
            procedure,
            COMMON_PROC_W,
            COMMON_RETURN,
            RETURN_ID,
        ],
        2,
        2,
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12180),
            &"msctls_progress32\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    capture(
        &mut engine,
        User32Api32::CreateWindowExW.id(),
        &[
            0, 0x12180, 0, 0x50000001, 0, 0, 8, 6, window, 37, 0x400000, 0,
        ],
    );
    callbacks(&mut engine, parent, common, procedure);
    let progress = eax(&engine);
    assert_ne!(progress, 0);
    let record = engine
        .user32_windows()
        .unwrap()
        .get(WindowHandle32::try_from(progress).unwrap())
        .unwrap();
    assert_eq!(record.description.procedure.0, procedure);
    assert_eq!(record.description.thread_id, 1);
    (engine, parent, common, procedure, progress, window)
}

fn send(
    engine: &mut EngineInstance,
    parent: u64,
    common: u64,
    procedure: u32,
    window: u32,
    message: [u32; 3],
) -> u32 {
    capture(
        engine,
        User32Api32::SendMessageW.id(),
        &[window, message[0], message[1], message[2]],
    );
    callbacks(engine, parent, common, procedure);
    assert!(engine.pending_call.is_none());
    eax(engine)
}

#[test]
fn captured_common_progress_creation_and_messages_use_real_class_callbacks_and_native_numeric_rows()
{
    let (mut engine, parent, common, procedure, window, _) = fixture();
    for (message, previous, position) in [
        ([0x401, 0, 10 | (20 << 16)], 6_553_600u32, 10),
        ([0x402, (-5i32) as u32, 0], 10, 10),
        ([0x402, 17, 0], 10, 17),
        ([0x402, 30, 0], 17, 20),
        ([0x404, 7, 0], 10, 20),
        ([0x405, 0, 0], 20, 17),
        ([0x403, (-40i32) as u32, 0], 17, 10),
        ([0x404, (-3i32) as u32, 0], 7, 10),
        ([0x405, 0, 0], 10, 17),
    ] {
        assert_eq!(
            send(&mut engine, parent, common, procedure, window, message),
            previous
        );
        assert_eq!(
            send(
                &mut engine,
                parent,
                common,
                procedure,
                window,
                [0x408, 0, 0]
            ),
            position
        );
    }
    assert_eq!(
        send(
            &mut engine,
            parent,
            common,
            procedure,
            window,
            [0x407, 1, 0x12320]
        ),
        10
    );
    assert_eq!(words::<2>(&engine, 0x12320), [10, 20]);
}

#[test]
fn captured_common_progress_colors_and_position_publish_real_bounded_gdi_pixels() {
    let (mut engine, parent, common, procedure, window, _) = fixture();
    assert_eq!(
        send(
            &mut engine,
            parent,
            common,
            procedure,
            window,
            [0x2001, 0, 0x00224466]
        ),
        0xff000000
    );
    assert_eq!(
        send(
            &mut engine,
            parent,
            common,
            procedure,
            window,
            [0x409, 0, 0x0088aacc]
        ),
        0xff000000
    );
    assert_eq!(
        send(
            &mut engine,
            parent,
            common,
            procedure,
            window,
            [0x402, 50, 0]
        ),
        0
    );
    let frame = (0..16)
        .find_map(|index| {
            engine
                .pending_gdi_frame(index)
                .unwrap()
                .filter(|frame| frame.window == window)
        })
        .expect("position change publishes the real Progress surface");
    assert_eq!((frame.width, frame.height, frame.format), (8, 6, 1));
    let pixels = engine
        .read_gdi_frame(frame.id, frame.revision, 0, frame.bytes)
        .unwrap();
    assert_eq!(&pixels[..4], &[0x22, 0x44, 0x66, 0]);
    assert_eq!(
        &pixels[(2 * 8 + 1) * 4..(2 * 8 + 2) * 4],
        &[0x88, 0xaa, 0xcc, 0]
    );
    assert_eq!(
        &pixels[(2 * 8 + 6) * 4..(2 * 8 + 7) * 4],
        &[0x22, 0x44, 0x66, 0]
    );
    assert_eq!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .gdi_update_region(WindowHandle32::try_from(window).unwrap())
            .unwrap()
            .0
            .complexity(),
        1
    );
}

#[path = "status.rs"]
mod status;

#[path = "tab.rs"]
mod tab;
#[path = "hotkey.rs"]
mod hotkey;

#[path = "tooltip.rs"]
mod tooltip;

#[path = "header.rs"]
mod header;

#[path = "toolbar.rs"]
mod toolbar;
