use super::super::paint::{
    KEY, PROC, RETURN, RETURN_ID, capture, eax, finish, fixture_with_startup,
};
use crate::{
    memory::{GuestAddress, PageRange, Permissions},
    process::{HostError, WindowsCallbackProvider32},
    windows::gdi::GdiApi32,
};
fn filter(engine: &mut crate::process::EngineInstance, family: &str, charset: u8) {
    let mut bytes = [0; 92];
    bytes[23] = charset;
    for (output, unit) in bytes[28..].chunks_exact_mut(2).zip(family.encode_utf16()) {
        output.copy_from_slice(&unit.to_le_bytes());
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x12500), &bytes)
        .unwrap();
}
fn face(engine: &crate::process::EngineInstance, address: u32) -> Vec<u16> {
    let mut bytes = [0; 64];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address + 28), &mut bytes)
        .unwrap();
    bytes
        .chunks_exact(2)
        .map(|part| u16::from_le_bytes(part.try_into().unwrap()))
        .take_while(|unit| *unit != 0)
        .collect()
}
#[test]
fn captured_enum_fonts_uses_provider_ten_real_structures_snapshot_and_last_callback_result() {
    let (mut engine, callback, _) = fixture_with_startup(false);
    capture(&mut engine, GdiApi32::CreateCompatibleDC.id(), &[0]);
    let dc = eax(&engine);
    filter(&mut engine, "", 1);
    capture(
        &mut engine,
        GdiApi32::EnumFontFamiliesExW.id(),
        &[dc, 0x12500, PROC, 0xcafebeef, 0],
    );
    let first = engine.pending_windows_callback().unwrap();
    assert_eq!(
        (first.provider, first.procedure.0, first.count),
        (WindowsCallbackProvider32::Gdi, PROC, 4)
    );
    assert_eq!(
        &first.arguments[1..4],
        &[first.arguments[0] + 348, 4, 0xcafebeef]
    );
    assert_eq!(
        face(&engine, first.arguments[0]),
        "Noto Sans".encode_utf16().collect::<Vec<_>>()
    );
    filter(&mut engine, "other captured input", 0);
    finish(&mut engine, callback, 42);
    let second = engine.pending_windows_callback().unwrap();
    assert_eq!(second.continuation, first.continuation);
    assert_eq!(second.arguments[0], first.arguments[0]);
    assert_eq!(
        face(&engine, second.arguments[0]),
        "Noto Sans Mono".encode_utf16().collect::<Vec<_>>()
    );
    finish(&mut engine, callback, 73);
    assert!(engine.pending_windows_callback().is_none());
    assert_eq!(eax(&engine), 73);
    assert!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(first.arguments[0]))
            .is_err()
    );
    assert!(engine.gdi.as_ref().unwrap().font_enumerations.is_empty());
}
#[test]
fn captured_enum_stop_zero_and_unmatched_family_do_not_fabricate_callbacks_or_boolean_results() {
    let (mut engine, callback, _) = fixture_with_startup(false);
    capture(&mut engine, GdiApi32::CreateCompatibleDC.id(), &[0]);
    let dc = eax(&engine);
    filter(&mut engine, "Noto Sans", 0);
    capture(
        &mut engine,
        GdiApi32::EnumFontFamiliesExW.id(),
        &[dc, 0x12500, PROC, 17, 0],
    );
    assert!(engine.pending_windows_callback().is_some());
    finish(&mut engine, callback, 0);
    assert_eq!(eax(&engine), 0);
    assert!(engine.pending_windows_callback().is_none());
    filter(&mut engine, "absent private family", 1);
    capture(
        &mut engine,
        GdiApi32::EnumFontFamiliesExW.id(),
        &[dc, 0x12500, PROC, 17, 0],
    );
    assert!(engine.pending_windows_callback().is_none());
    assert_eq!(eax(&engine), 1);
}
#[test]
fn remapped_owned_font_scratch_rejects_callback_entry_without_touching_replacement() {
    let (mut engine, callback, _) = fixture_with_startup(false);
    capture(&mut engine, GdiApi32::CreateCompatibleDC.id(), &[0]);
    let dc = eax(&engine);
    filter(&mut engine, "", 1);
    capture(
        &mut engine,
        GdiApi32::EnumFontFamiliesExW.id(),
        &[dc, 0x12500, PROC, 0, 0],
    );
    let request = engine.pending_windows_callback().unwrap();
    let range = PageRange::new(GuestAddress(request.arguments[0]), 1).unwrap();
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
        .write(GuestAddress(request.arguments[0]), b"replacement")
        .unwrap();
    let version = engine.memory().unwrap().transaction_version();
    let pending = engine.pending_call.as_ref().unwrap().token;
    assert!(matches!(
        engine.begin_windows_resident_callback(KEY, callback, RETURN, RETURN_ID),
        Err(HostError::Memory(_))
    ));
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.pending_call.as_ref().unwrap().token, pending);
    assert_eq!(engine.pending_windows_callback(), Some(request));
    let mut bytes = [0; 11];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(request.arguments[0]), &mut bytes)
        .unwrap();
    assert_eq!(&bytes, b"replacement");
}
