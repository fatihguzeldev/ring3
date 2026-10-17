use super::*;
use crate::{
    abi::{
        arena::STATE_OFFSET,
        x86::{STATE_SIZE, decode_state},
    },
    files::{ATTR_NORMAL, AssetRef32, FileTimes32},
    process::{CallError, HostError, windows::files::FileConfig32},
};

#[test]
fn captured_file_image_keeps_exact_lazy_request_and_original_filename_until_real_bytes_arrive() {
    let mut engine = fixture(false);
    engine
        .configure_files(FileConfig32 {
            vfs: crate::files::FileLimits32 {
                max_io_bytes: 17,
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    let mut bytes = vec![0; 86];
    bytes[..2].copy_from_slice(b"BM");
    word(&mut bytes, 2, 86);
    word(&mut bytes, 10, 70);
    for (at, value) in [(14, 40), (18, 2), (22, (-2i32) as u32), (26, 1 | 32 << 16)] {
        word(&mut bytes, at, value);
    }
    bytes[70..].copy_from_slice(
        &[0u32, 0xff, 0xff00, 0xff0000]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    let path = r"C:\authored.bmp".encode_utf16().collect::<Vec<_>>();
    engine
        .mount_file(
            &path,
            AssetRef32 {
                id: 909,
                length: bytes.len() as u64,
            },
            ATTR_NORMAL,
            FileTimes32::default(),
        )
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(DATA),
            &path
                .iter()
                .copied()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    let modules = engine.loaded_windows_modules().count();
    call(&mut engine, Api::LoadImageW, &[0, DATA, 0, 0, 0, 0x10]);
    let pending = engine.pending_user32_image_call().unwrap().unwrap();
    assert_eq!(
        (pending.key, pending.request.asset, pending.ready),
        (7, 909, false)
    );
    assert_eq!(
        engine.resume_user32_image_call(7, pending.token),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.resume_user32_image_call(8, pending.token),
        Err(HostError::InvalidArtifact)
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(DATA), &[0, 0])
        .unwrap();
    let mut waits = 0;
    while let Some(wait) = engine.pending_user32_image_call().unwrap() {
        assert_eq!(wait.token, pending.token);
        assert_eq!(wait.request.asset, 909);
        let start = wait.request.offset as usize;
        for (index, chunk) in bytes[start..start + wait.request.length as usize]
            .chunks(4096)
            .enumerate()
        {
            engine
                .supply_file_bytes(wait.request, index as u32 * 4096, chunk)
                .unwrap();
        }
        assert!(engine.pending_user32_image_call().unwrap().unwrap().ready);
        engine.resume_user32_image_call(7, wait.token).unwrap();
        waits += 1;
    }
    assert!(waits > 0);
    assert!(engine.pending_call.is_none());
    let handle = decode_state(&engine.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
        .unwrap()
        .registers[0];
    assert_ne!(handle, 0);
    let info = engine
        .gdi
        .as_ref()
        .unwrap()
        .bitmap_info(engine.memory.as_ref().unwrap(), handle)
        .unwrap();
    assert_eq!((info.width, info.height, info.dib_section), (2, 2, false));
    assert_eq!(engine.loaded_windows_modules().count(), modules);
    assert!(engine.pending_file_request().unwrap().is_none());
    assert_eq!(gdi(&mut engine, GdiApi32::DeleteObject, &[handle]), 1);
    assert_eq!(
        engine.resume_user32_image_call(7, pending.token),
        Err(HostError::Call(CallError::InvalidToken))
    );
}

#[test]
fn completed_image_bytes_survive_real_dib_ram_capacity_failure_and_resume_without_a_file_wait() {
    use crate::memory::{MemoryError, Permissions};
    let mut engine = fixture(false);
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine.configure_files(Default::default()).unwrap();
    let mut bytes = vec![0; 70];
    bytes[..2].copy_from_slice(b"BM");
    for (at, value) in [
        (2, 70),
        (10, 54),
        (14, 40),
        (18, 2),
        (22, (-2i32) as u32),
        (26, 1 | 32 << 16),
    ] {
        word(&mut bytes, at, value);
    }
    let path = r"C:\retry.bmp".encode_utf16().collect::<Vec<_>>();
    engine
        .mount_file(
            &path,
            AssetRef32 {
                id: 910,
                length: bytes.len() as u64,
            },
            ATTR_NORMAL,
            FileTimes32::default(),
        )
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(DATA),
            &path
                .into_iter()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    call(&mut engine, Api::LoadImageW, &[0, DATA, 0, 0, 0, 0x2010]);
    let wait = engine.pending_user32_image_call().unwrap().unwrap();
    engine.supply_file_bytes(wait.request, 0, &bytes).unwrap();
    let frame = engine.pending_call.as_ref().unwrap().frame;
    let memory = engine.memory.as_mut().unwrap();
    let filler = PageRange::new(
        GuestAddress(0x50000000),
        memory.capacity_pages() - memory.mapped_pages(),
    )
    .unwrap();
    memory.map_zeroed(filler, Permissions::READ_WRITE).unwrap();
    let version = memory.transaction_version();
    assert_eq!(
        engine.resume_user32_image_call(7, wait.token),
        Err(HostError::Memory(MemoryError::Capacity))
    );
    assert_eq!(
        engine.memory.as_ref().unwrap().transaction_version(),
        version
    );
    assert_eq!(engine.pending_call.as_ref().unwrap().frame, frame);
    assert_eq!(engine.pending_call.as_ref().unwrap().token, wait.token);
    assert!(engine.pending_user32_image_call().unwrap().is_none());
    assert!(engine.pending_file_request().unwrap().is_none());
    engine.memory.as_mut().unwrap().unmap(filler).unwrap();
    assert_eq!(
        engine.schedule_windows_thread().unwrap(),
        crate::windows::thread::ThreadSchedule32::Running { thread: 1 }
    );
    assert!(engine.pending_call.is_none());
    let handle = decode_state(&engine.arena()[..STATE_SIZE])
        .unwrap()
        .registers[0];
    assert_ne!(handle, 0);
    assert!(
        engine
            .gdi
            .as_ref()
            .unwrap()
            .bitmap_info(engine.memory.as_ref().unwrap(), handle)
            .unwrap()
            .dib_section
    );
    assert!(engine.pending_user32_image_call().unwrap().is_none());
    assert_eq!(gdi(&mut engine, GdiApi32::DeleteObject, &[handle]), 1);
}
