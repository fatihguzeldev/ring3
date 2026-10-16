use crate::{
    abi::{arena::*, x86::*},
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::{GuestAddress, PageRange, Permissions},
    process::{EngineInstance, windows::files::FileConfig32},
    windows::{CallingConvention32, WindowsApi32, files::FileApi32, gdi::GdiApi32},
};

fn fixture() -> EngineInstance {
    let mut engine = EngineInstance::new(64, 7).unwrap();
    engine.map(0x10000, 2, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x10000), &[0x0f, 0x0b])
        .unwrap();
    engine.protect(0x10000, 1, 5).unwrap();
    engine.protect(0x11000, 1, 3).unwrap();
    engine.configure_files(FileConfig32::default()).unwrap();
    engine.gdi = Some(
        super::super::GdiRuntime32::new(super::super::GdiConfig32 {
            bitmap_base: 0x20000,
            bitmap_pages: 8,
            bitmap_bytes: 65_536,
            screen_width: 8,
            screen_height: 8,
            ..Default::default()
        })
        .unwrap(),
    );
    super::super::memory::write_words(
        engine.memory.as_mut().unwrap(),
        0x11080,
        &[40, 3, (-2i32) as u32, 1 | 32 << 16, 0, 24, 0, 0, 0, 0],
    )
    .unwrap();
    engine
}
fn capture(engine: &mut EngineInstance, id: u32, args: &[u32]) -> u32 {
    for (index, word) in [0x10000u32, 2, 0x10000, id].iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    engine.compile_with_gates(1, 1).unwrap();
    let mut state = State32 {
        eip: 0x10000,
        ..Default::default()
    };
    state.registers[4] = 0x11800;
    let stack = std::iter::once(0x10001)
        .chain(args.iter().copied())
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x11800), &stack)
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
    engine
        .capture_call(
            7,
            engine.generation,
            CallingConvention32::Stdcall,
            args.len() as u32,
        )
        .unwrap()
        .token
}
fn finish(engine: &mut EngineInstance, token: u32) -> u32 {
    engine
        .complete_windows_call(7, engine.generation, token)
        .unwrap();
    decode_state(&engine.arena()[..STATE_SIZE])
        .unwrap()
        .registers[0]
}
fn call(engine: &mut EngineInstance, id: u32, args: &[u32]) -> u32 {
    let token = capture(engine, id, args);
    finish(engine, token)
}
fn read<const N: usize>(engine: &EngineInstance, at: u32) -> [u8; N] {
    let mut bytes = [0; N];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(at), &mut bytes)
        .unwrap();
    bytes
}
fn word(engine: &EngineInstance, at: u32) -> u32 {
    u32::from_le_bytes(read(engine, at))
}
fn section(engine: &mut EngineInstance, protection: u32) -> u32 {
    let section = call(
        engine,
        FileApi32::CreateFileMappingW.id(),
        &[u32::MAX, 0, protection, 0, 8192, 0],
    );
    assert_ne!(section, 0);
    section
}
#[test]
fn captured_section_dib_aliases_dword_offset_and_releases_only_its_canonical_view() {
    let mut engine = fixture();
    let section = section(&mut engine, 4);
    let shared = call(
        &mut engine,
        FileApi32::MapViewOfFile.id(),
        &[section, 2, 0, 0, 8192],
    );
    assert_ne!(shared, 0);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(shared + 4), &[0x33, 0x22, 0x11, 0])
        .unwrap();
    let bitmap = call(
        &mut engine,
        GdiApi32::CreateDIBSection.id(),
        &[0, 0x11080, 0, 0x11100, section, 4],
    );
    assert_ne!(bitmap, 0);
    let bits = word(&engine, 0x11100);
    assert_ne!(bits, shared + 4);
    assert_eq!(bits % 4096, 4);
    assert_eq!(read::<4>(&engine, bits), [0x33, 0x22, 0x11, 0]);
    let dc = call(&mut engine, GdiApi32::CreateCompatibleDC.id(), &[0]);
    let old = call(&mut engine, GdiApi32::SelectObject.id(), &[dc, bitmap]);
    assert_eq!(
        call(&mut engine, GdiApi32::GetPixel.id(), &[dc, 0, 0]),
        0x00332211
    );
    assert_eq!(
        call(
            &mut engine,
            GdiApi32::SetPixel.id(),
            &[dc, 1, 0, 0x00665544]
        ),
        0x00665544
    );
    assert_eq!(read::<4>(&engine, shared + 8), [0x66, 0x55, 0x44, 0]);
    assert_eq!(
        call(
            &mut engine,
            GdiApi32::GetObjectW.id(),
            &[bitmap, 84, 0x11200]
        ),
        84
    );
    assert_eq!(word(&engine, 0x11200 + 20), bits);
    assert_eq!(word(&engine, 0x11200 + 76), section);
    assert_eq!(word(&engine, 0x11200 + 80), 4);
    assert_eq!(call(&mut engine, GdiApi32::DeleteObject.id(), &[bitmap]), 0);
    call(&mut engine, GdiApi32::SelectObject.id(), &[dc, old]);
    assert_eq!(call(&mut engine, GdiApi32::DeleteObject.id(), &[bitmap]), 1);
    let mut byte = [0];
    assert!(
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(bits), &mut byte)
            .is_err()
    );
    assert_eq!(read::<4>(&engine, shared + 8), [0x66, 0x55, 0x44, 0]);
    assert_eq!(
        call(&mut engine, FileApi32::UnmapViewOfFile.id(), &[shared]),
        1
    );
    assert_eq!(
        call(&mut engine, WindowsApi32::CloseHandle.id(), &[section]),
        1
    );
}
#[test]
fn captured_section_dib_rejects_bad_output_before_mapping_and_preserves_copy_isolation() {
    let mut engine = fixture();
    let section = section(&mut engine, 8);
    let shared = call(
        &mut engine,
        FileApi32::MapViewOfFile.id(),
        &[section, 4, 0, 0, 8192],
    );
    let token = capture(
        &mut engine,
        GdiApi32::CreateDIBSection.id(),
        &[0, 0x11080, 0, 0x12000, section, 4],
    );
    let version = engine.memory().unwrap().transaction_version();
    let mapped = engine.memory().unwrap().mapped_pages();
    assert_eq!(finish(&mut engine, token), 0);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.memory().unwrap().mapped_pages(), mapped);
    assert_eq!(engine.gdi.as_ref().unwrap().model.object_count(), 0);
    let bitmap = call(
        &mut engine,
        GdiApi32::CreateDIBSection.id(),
        &[0, 0x11080, 0, 0x11100, section, 4],
    );
    assert_ne!(bitmap, 0);
    let bits = word(&engine, 0x11100);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(bits), &[1, 2, 3, 0])
        .unwrap();
    assert_eq!(read::<4>(&engine, shared + 4), [0; 4]);
    assert_eq!(call(&mut engine, GdiApi32::DeleteObject.id(), &[bitmap]), 1);
    assert_eq!(
        call(&mut engine, WindowsApi32::CloseHandle.id(), &[section]),
        1
    );
    assert_eq!(read::<4>(&engine, shared + 4), [0; 4]);
    assert_eq!(
        call(&mut engine, FileApi32::UnmapViewOfFile.id(), &[shared]),
        1
    );
}
#[test]
fn captured_section_dib_stale_view_cannot_delete_replacement_mapping() {
    let mut engine = fixture();
    let section = section(&mut engine, 4);
    let bitmap = call(
        &mut engine,
        GdiApi32::CreateDIBSection.id(),
        &[0, 0x11080, 0, 0x11100, section, 4],
    );
    assert_ne!(bitmap, 0);
    let bits = word(&engine, 0x11100);
    let range = PageRange::new(GuestAddress(bits & !4095), 1).unwrap();
    engine.memory.as_mut().unwrap().unmap(range).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .map_zeroed(range, Permissions::READ_WRITE)
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(bits), b"replacement")
        .unwrap();
    let token = capture(&mut engine, GdiApi32::DeleteObject.id(), &[bitmap]);
    let version = engine.memory().unwrap().transaction_version();
    assert_eq!(finish(&mut engine, token), 0);
    assert_eq!(read::<11>(&engine, bits), *b"replacement");
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.gdi.as_ref().unwrap().model.object_count(), 1);
}

mod lazy;

mod failures;
