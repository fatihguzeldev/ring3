use super::*;
use crate::{
    files::{ATTR_NORMAL, AssetRef32, FileTimes32},
    process::{CallError, HostError, windows::files::FileConfig32},
    windows::comctl32::animate::tests::avi,
};
fn animation(
    engine: &mut EngineInstance,
    callback: u64,
    common: u64,
    procedure: u32,
    parent: u32,
) -> u32 {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12400),
            &"SysAnimate32\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    capture(
        engine,
        User32Api32::CreateWindowExW.id(),
        &[
            0, 0x12400, 0, 0x50000008, 0, 0, 8, 6, parent, 97, 0x400000, 0,
        ],
    );
    callbacks(engine, callback, common, procedure);
    let window = eax(engine);
    assert_ne!(window, 0);
    window
}
fn drain(
    engine: &mut EngineInstance,
    callback: u64,
    common: u64,
    procedure: u32,
    parent: u32,
    notifications: &mut Vec<u32>,
    bytes: &[u8],
    waits: &mut usize,
) {
    let mut source_owner = None;
    for _ in 0..64 {
        if engine.pending_windows_callback().is_some() {
            callback_servicing(
                engine,
                callback,
                common,
                procedure,
                &mut |_, args| {
                    if args[0] == parent && args[1] == 0x111 {
                        notifications.push(args[2] >> 16);
                    }
                    None
                },
                &mut |engine| {
                    if let Some(request) = engine.pending_file_request().unwrap() {
                        let wait = engine.pending_file_call().unwrap();
                        let pending = engine.pending_call.as_ref().unwrap();
                        let actual_owner = (pending.owner, pending.token, pending.frame);
                        if let Some(expected) = source_owner {
                            assert_eq!(actual_owner, expected);
                        } else {
                            source_owner = Some(actual_owner);
                        }
                        assert_eq!(request.asset, 991);
                        if *waits == 0 {
                            let window = actual_owner.2.arguments()[0];
                            let version = engine.memory().unwrap().transaction_version();
                            let parked = engine.user32.as_mut().unwrap().take_thread_reply();
                            assert_eq!(
                                engine
                                    .execute_common_control_procedure(&[window, 0x467, 0, 0], true),
                                Err(HostError::Call(CallError::Busy))
                            );
                            engine.user32.as_mut().unwrap().restore_thread_reply(parked);
                            assert_eq!(engine.memory().unwrap().transaction_version(), version);
                            assert_eq!(engine.pending_file_request().unwrap(), Some(request));
                            let pending = engine.pending_call.as_ref().unwrap();
                            assert_eq!((pending.owner, pending.token, pending.frame), actual_owner);
                        }
                        assert_eq!(
                            engine.cancel_file_call(KEY, wait.token),
                            Err(HostError::Call(CallError::Busy))
                        );
                        assert_eq!(
                            engine.resume_file_call(KEY + 1, wait.token),
                            Err(HostError::InvalidArtifact)
                        );
                        engine
                            .memory
                            .as_mut()
                            .unwrap()
                            .write(GuestAddress(0x12600), &[0, 0])
                            .unwrap();
                        let first = request.offset as usize;
                        for (index, chunk) in bytes[first..first + request.length as usize]
                            .chunks(4096)
                            .enumerate()
                        {
                            engine
                                .supply_file_bytes(request, index as u32 * 4096, chunk)
                                .unwrap();
                        }
                        engine.resume_file_call(KEY, wait.token).unwrap();
                        *waits += 1;
                    }
                },
            );
        } else if let Some(wait) = engine.waiting_user32_message() {
            engine.resume_user32_wait(KEY, wait).unwrap();
        } else {
            assert!(engine.pending_call.is_none());
            return;
        }
    }
    panic!("animation callbacks must finish within existing bounded callback transport");
}
fn source(engine: &mut EngineInstance, bytes: &[u8], lazy: bool) {
    engine
        .configure_files(FileConfig32 {
            vfs: crate::files::FileLimits32 {
                max_io_bytes: if lazy { 17 } else { 4096 },
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    let path = "C:\\clip.avi".encode_utf16().collect::<Vec<_>>();
    if lazy {
        engine
            .mount_file(
                &path,
                AssetRef32 {
                    id: 991,
                    length: bytes.len() as u64,
                },
                ATTR_NORMAL,
                FileTimes32::default(),
            )
            .unwrap();
    } else {
        engine.replace_provider_file(&path, bytes).unwrap();
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x12600),
            &path
                .into_iter()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
}
fn paint_pixel(
    engine: &mut EngineInstance,
    callback: u64,
    common: u64,
    procedure: u32,
    window: u32,
) -> [u8; 4] {
    send(engine, callback, common, procedure, window, [0xf, 0, 0]);
    let frame = (0..16)
        .filter_map(|i| engine.pending_gdi_frame(i).ok().flatten())
        .find(|frame| frame.window == window)
        .unwrap();
    engine
        .read_gdi_frame(frame.id, frame.revision, 0, 4)
        .unwrap()
        .try_into()
        .unwrap()
}
#[test]
fn captured_animation_lazy_open_captures_filename_and_uses_original_file_wait_owner_then_real_first_frame()
 {
    let (mut engine, callback, common, procedure, _, parent) =
        fixture_with_gdi_bytes(2 * crate::files::FILE_PAGE_BYTES);
    let window = animation(&mut engine, callback, common, procedure, parent);
    let mut bytes = avi();
    let mut junk = b"JUNK".to_vec();
    junk.extend_from_slice(&(crate::files::FILE_PAGE_BYTES as u32).to_le_bytes());
    junk.resize(crate::files::FILE_PAGE_BYTES + 8, 0);
    bytes.splice(12..12, junk);
    let size = (bytes.len() as u32) - 8;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    source(&mut engine, &bytes, true);
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[window, 0x467, 0, 0x12600],
    );
    let mut notifications = Vec::new();
    let mut waits = 0;
    drain(
        &mut engine,
        callback,
        common,
        procedure,
        parent,
        &mut notifications,
        &bytes,
        &mut waits,
    );
    assert_eq!(eax(&engine), 1);
    assert!(waits > 1);
    assert!(notifications.is_empty());
    assert!(engine.pending_file_call().is_none());
    assert!(engine.pending_file_request().unwrap().is_none());
    capture(
        &mut engine,
        User32Api32::GetWindowRect.id(),
        &[window, 0x12580],
    );
    assert_eq!(words::<4>(&engine, 0x12580), [0, 0, 2, 2]);
    assert_eq!(
        paint_pixel(&mut engine, callback, common, procedure, window),
        [0, 0, 255, 0]
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x468, 0, 0]
        ),
        0
    );
}
#[test]
fn captured_animation_actual_timer_dispatch_play_stop_seek_and_rejected_codec_preserve_real_pixels()
{
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = animation(&mut engine, callback, common, procedure, parent);
    let bytes = avi();
    source(&mut engine, &bytes, false);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x467, 0, 0x12600]
        ),
        1
    );
    let mut notifications = Vec::new();
    let mut waits = 0;
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[window, 0x465, 1, u32::from(u16::MAX) << 16],
    );
    drain(
        &mut engine,
        callback,
        common,
        procedure,
        parent,
        &mut notifications,
        &[],
        &mut waits,
    );
    assert_eq!(eax(&engine), 1);
    assert_eq!(notifications, [1]);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x468, 0, 0]
        ),
        1
    );
    for _ in 0..2 {
        engine.advance_cpu_clock(20_000_000).unwrap();
        capture(
            &mut engine,
            User32Api32::PeekMessageW.id(),
            &[0x12800, window, 0x113, 0x113, 1],
        );
        callbacks(&mut engine, callback, common, procedure);
        assert_eq!(eax(&engine), 1);
        assert_eq!(words::<4>(&engine, 0x12800), [window, 0x113, 1, 0]);
        capture(&mut engine, User32Api32::DispatchMessageW.id(), &[0x12800]);
        drain(
            &mut engine,
            callback,
            common,
            procedure,
            parent,
            &mut notifications,
            &[],
            &mut waits,
        );
    }
    assert_eq!(notifications, [1, 2]);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x468, 0, 0]
        ),
        0
    );
    assert_eq!(
        paint_pixel(&mut engine, callback, common, procedure, window),
        [255, 0, 0, 0]
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x465, 1, 0]
        ),
        1
    );
    assert_eq!(
        paint_pixel(&mut engine, callback, common, procedure, window),
        [0, 0, 255, 0]
    );
    let mut invalid = bytes.clone();
    let at = invalid.windows(4).position(|part| part == b"DIB ").unwrap();
    invalid[at..at + 4].copy_from_slice(b"XVID");
    engine
        .replace_provider_file(&"C:\\clip.avi".encode_utf16().collect::<Vec<_>>(), &invalid)
        .unwrap();
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x467, 0, 0x12600]
        ),
        0
    );
    assert_eq!(
        paint_pixel(&mut engine, callback, common, procedure, window),
        [0, 0, 255, 0]
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x467, 0, 0]
        ),
        0
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x468, 0, 0]
        ),
        0
    );
}

#[test]
fn captured_animation_named_avi_resource_is_copied_before_module_unload() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = animation(&mut engine, callback, common, procedure, parent);
    let clip = avi();
    let mut image = crate::process::windows::resources::tests::image(0x500000, true, true);
    let data = &mut image[0x400..0x800];
    for (at, value) in [(12usize, 1u16), (14, 0)] {
        data[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    for (at, value) in [
        (16usize, 0x800000e0u32),
        (20, 0x80000050),
        (0xc0, 0x2180),
        (0xc4, clip.len() as u32),
    ] {
        data[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    data[0xe0..0xe8].copy_from_slice(&[3, 0, 65, 0, 86, 0, 73, 0]);
    data[0x180..0x180 + clip.len()].copy_from_slice(&clip);
    engine
        .register_guest_module("animation.dll", &image)
        .unwrap();
    let crate::process::ModuleOperation32::Loaded { handle } =
        engine.load_library_module("animation.dll", None).unwrap()
    else {
        panic!("authored resource DLL has no initializer")
    };
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x467, handle, 7]
        ),
        1
    );
    assert_eq!(
        engine.free_library_module(handle).unwrap(),
        crate::process::ModuleOperation32::Released
    );
    assert_eq!(
        paint_pixel(&mut engine, callback, common, procedure, window),
        [0, 0, 255, 0]
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x465, 1, 1 | (1 << 16)]
        ),
        1
    );
    assert_eq!(
        paint_pixel(&mut engine, callback, common, procedure, window),
        [255, 0, 0, 0]
    );
}

#[test]
fn captured_animation_empty_play_preserves_last_error_and_missing_resource_is_boolean_failure() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = animation(&mut engine, callback, common, procedure, parent);
    capture(
        &mut engine,
        crate::windows::WindowsApi32::SetLastError.id(),
        &[0xdeadbeef],
    );
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x465, u32::MAX, u32::from(u16::MAX) << 16]
        ),
        0
    );
    capture(
        &mut engine,
        crate::windows::WindowsApi32::GetLastError.id(),
        &[],
    );
    assert_eq!(eax(&engine), 0xdeadbeef);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x467, 0, 0xffff]
        ),
        0
    );
    capture(
        &mut engine,
        crate::windows::WindowsApi32::GetLastError.id(),
        &[],
    );
    assert_eq!(eax(&engine), 1813);
    assert_eq!(
        send(
            &mut engine,
            callback,
            common,
            procedure,
            window,
            [0x467, 0, 0]
        ),
        0
    );
}

#[test]
fn captured_animation_destroyed_loading_window_retires_original_source_without_host_bytes() {
    let (mut engine, callback, common, procedure, _, parent) = fixture();
    let window = animation(&mut engine, callback, common, procedure, parent);
    let bytes = avi();
    source(&mut engine, &bytes, true);
    capture(
        &mut engine,
        User32Api32::SendMessageW.id(),
        &[window, 0x467, 0, 0x12600],
    );
    let mut retired = false;
    callback_servicing(
        &mut engine,
        callback,
        common,
        procedure,
        &mut |_, _| None,
        &mut |engine| {
            if let Some(request) = engine.pending_file_request().unwrap() {
                assert!(!retired);
                assert_eq!(request.asset, 991);
                let continuation = engine.waiting_user32_message().unwrap();
                let original = engine
                    .pending_call
                    .as_ref()
                    .map(|pending| (pending.owner, pending.token, pending.frame))
                    .unwrap();
                let parked = engine.user32.as_mut().unwrap().take_thread_reply();
                engine
                    .user32
                    .as_mut()
                    .unwrap()
                    .destroy_animation_window_for_test(
                        window,
                        engine.memory.as_mut().unwrap(),
                        crate::process::windows::user32::User32CallContext32 {
                            thread_id: 1,
                            instance: 0x400000,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                engine.user32.as_mut().unwrap().restore_thread_reply(parked);
                assert!(engine.user32.as_ref().unwrap().active_thread_wait_ready());
                let pending = engine.pending_call.as_ref().unwrap();
                assert_eq!((pending.owner, pending.token, pending.frame), original);
                engine.resume_user32_wait(KEY, continuation).unwrap();
                assert!(engine.pending_file_call().is_none());
                assert!(engine.pending_file_request().unwrap().is_none());
                retired = true;
            }
        },
    );
    callbacks(&mut engine, callback, common, procedure);
    assert!(retired);
    assert_eq!(eax(&engine), 0);
    assert!(engine.pending_call.is_none());
}
