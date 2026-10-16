use super::paint::{
    KEY, PROC, RETURN, RETURN_ID, capture, eax, finish, fixture_with_startup, stop, unit,
};
use crate::{
    abi::{
        arena::EXIT_OFFSET,
        x86::{EXIT_SIZE, encode_exit_v3},
    },
    cpu::{ExecutionExit, ExitReason},
    memory::{GuestAddress, PageRange, Permissions},
    process::EngineInstance,
    windows::{
        CallingConvention32,
        controls::{BUILTIN_PROC_W, ControlKind32},
        gdi::GdiApi32,
        user32::User32Api32,
        window::WindowHandle32,
    },
};

fn child(
    engine: &mut EngineInstance,
    parent: u32,
    parent_callback: u64,
    kind: ControlKind32,
    style: u32,
) -> (u32, u64) {
    let context = engine.user32.as_ref().unwrap();
    let (atom, procedure) = context.standard_control_gate(kind);
    let procedure = procedure.0;
    let builtin_return = 0x10040;
    let builtin = unit(
        engine,
        &[
            procedure,
            2,
            builtin_return,
            2,
            procedure,
            BUILTIN_PROC_W,
            builtin_return,
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
            &"&Hello\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    capture(
        engine,
        User32Api32::CreateWindowExW.id(),
        &[
            0,
            u32::from(atom),
            0x12180,
            style | 0x50000000,
            0,
            0,
            96,
            32,
            parent,
            1,
            0x400000,
            0,
        ],
    );
    while let Some(request) = engine.pending_windows_callback() {
        if request.procedure.0 == PROC {
            finish(
                engine,
                parent_callback,
                u32::from(request.arguments[1] == 0x81),
            );
            continue;
        }
        assert_eq!(request.procedure.0, procedure);
        let callback = engine
            .begin_windows_resident_callback(KEY, builtin, builtin_return, RETURN_ID)
            .unwrap();
        engine
            .authorize_resident_callback(KEY, builtin, callback.token)
            .unwrap();
        encode_exit_v3(
            &ExecutionExit {
                retired: 0,
                reason: ExitReason::Gate { id: BUILTIN_PROC_W },
            },
            &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
        )
        .unwrap();
        let token = engine
            .capture_resident_callback_call(
                KEY,
                builtin,
                callback.token,
                CallingConvention32::Stdcall,
                4,
            )
            .unwrap()
            .token;
        engine
            .complete_active_resident_callback_windows_call(KEY, builtin, callback.token, token)
            .unwrap();
        let value = eax(engine);
        stop(engine, builtin_return, RETURN_ID, value);
        engine
            .finish_windows_resident_callback(KEY, builtin, callback.token)
            .unwrap();
    }
    let child = eax(engine);
    assert_ne!(child, 0);
    (child, builtin)
}

fn frame(engine: &EngineInstance, window: u32) -> crate::process::GdiFrame32 {
    (0..16)
        .find_map(|index| {
            engine
                .pending_gdi_frame(index)
                .unwrap()
                .filter(|frame| frame.window == window)
        })
        .expect("paint must publish a real window frame")
}

fn pixels(engine: &EngineInstance, frame: crate::process::GdiFrame32) -> Vec<u8> {
    let mut bytes = Vec::new();
    for offset in (0..frame.bytes).step_by(4096) {
        bytes.extend_from_slice(
            engine
                .read_gdi_frame(
                    frame.id,
                    frame.revision,
                    offset,
                    (frame.bytes - offset).min(4096),
                )
                .unwrap(),
        );
    }
    bytes
}
fn surface_revision(engine: &EngineInstance, window: u32) -> u64 {
    engine
        .gdi
        .as_ref()
        .unwrap()
        .surfaces
        .iter()
        .find(|surface| {
            surface.key
                == super::super::storage::SurfaceKey32::Window {
                    window,
                    whole: false,
                }
        })
        .unwrap()
        .revision
}

#[test]
fn captured_builtin_paint_calls_parent_color_and_publishes_real_brush_and_glyphs() {
    let (mut engine, parent_callback, parent) = fixture_with_startup(true);
    let (child, _) = child(
        &mut engine,
        parent,
        parent_callback,
        ControlKind32::Static,
        0,
    );
    capture(&mut engine, GdiApi32::CreateSolidBrush.id(), &[0x00332211]);
    let brush = eax(&engine);
    capture(&mut engine, BUILTIN_PROC_W, &[child, 0x0f, 0, 0]);
    let request = engine
        .pending_windows_callback()
        .expect("built-in painting must call the real parent WM_CTLCOLORSTATIC");
    assert_eq!(request.procedure.0, PROC);
    assert_eq!(request.count, 4);
    assert_eq!(request.arguments[0], parent);
    assert_eq!(request.arguments[1], 0x138);
    let dc = request.arguments[2];
    assert_eq!(request.arguments[3], child);
    finish(&mut engine, parent_callback, brush);
    assert_eq!(eax(&engine), 0);
    assert!(engine.pending_windows_callback().is_none());
    let painted = frame(&engine, child);
    assert_eq!((painted.width, painted.height, painted.format), (96, 32, 1));
    let pixels = pixels(&engine, painted);
    assert_eq!(
        &pixels[(30 * 96 + 90) * 4..(30 * 96 + 91) * 4],
        &[0x33, 0x22, 0x11, 0]
    );
    assert!(
        pixels
            .chunks_exact(4)
            .skip(6 * 96)
            .take(20 * 96)
            .any(|pixel| pixel[..3] == [0, 0, 0]),
        "real font glyphs must be present"
    );
    assert_eq!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .gdi_update_region(WindowHandle32::try_from(child).unwrap())
            .unwrap()
            .0
            .complexity(),
        1
    );
    capture(&mut engine, GdiApi32::GetObjectType.id(), &[dc]);
    assert_eq!(eax(&engine), 0);
}

#[test]
fn captured_control_color_return_is_sealed_across_teb_fault_and_paints_exactly_once() {
    let (mut engine, parent_callback, parent) = fixture_with_startup(true);
    let (child, _) = child(&mut engine, parent, parent_callback, ControlKind32::Edit, 0);
    capture(&mut engine, GdiApi32::CreateSolidBrush.id(), &[0x00332211]);
    let brush = eax(&engine);
    let (owner, outer_token) = capture(&mut engine, BUILTIN_PROC_W, &[child, 0x0f, 0, 0]);
    let request = engine
        .pending_windows_callback()
        .expect("edit painting must call its actual parent");
    assert_eq!(request.arguments[1], 0x133);
    let before = surface_revision(&engine, child);
    assert!((0..16).all(|index| {
        engine
            .pending_gdi_frame(index)
            .unwrap()
            .is_none_or(|frame| frame.window != child)
    }));
    let callback = engine
        .begin_windows_resident_callback(KEY, parent_callback, RETURN, RETURN_ID)
        .unwrap();
    engine
        .authorize_resident_callback(KEY, parent_callback, callback.token)
        .unwrap();
    stop(&mut engine, RETURN, RETURN_ID, brush);
    let teb = engine.module_teb_address().unwrap();
    let range = PageRange::new(GuestAddress(teb & !4095), 1).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(range, Permissions::READ)
        .unwrap();
    assert!(
        engine
            .finish_windows_resident_callback(KEY, parent_callback, callback.token)
            .is_err()
    );
    assert!(engine.pending_windows_callback().is_none());
    assert_eq!(engine.pending_call.as_ref().unwrap().token, outer_token);
    assert_eq!(surface_revision(&engine, child), before);
    assert!((0..16).all(|index| {
        engine
            .pending_gdi_frame(index)
            .unwrap()
            .is_none_or(|frame| frame.window != child)
    }));
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(range, Permissions::READ_WRITE)
        .unwrap();
    engine
        .complete_resident_windows_call(KEY, owner, outer_token)
        .unwrap();
    assert!(engine.pending_call.is_none());
    assert!(engine.pending_windows_callback().is_none());
    assert_eq!(frame(&engine, child).revision, before + 1);
    assert!(
        engine
            .complete_resident_windows_call(KEY, owner, outer_token)
            .is_err()
    );
    assert_eq!(frame(&engine, child).revision, before + 1);
}

mod raster;
