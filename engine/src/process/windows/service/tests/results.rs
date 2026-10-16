use super::support::*;
use crate::{
    memory::GuestAddress,
    windows::{files::FileApi32, ole32::Ole32Api32, registry::Advapi32Api32},
};

#[test]
fn guarded_filename_returns_invalid_handle_and_retry_does_not_retain_the_fault() {
    let mut engine = engine();
    put(&mut engine, DATA, b"C:\\Temp\\guard.bin\0");
    guard(&mut engine, DATA, 2);
    let args = [DATA, 0xc000_0000, 7, 0, 2, 0x80, 0];
    assert_eq!(
        call(&mut engine, FileApi32::CreateFileA.id(), &args),
        u32::MAX
    );
    assert_eq!(engine.current_windows_thread().unwrap().last_error(), 998);
    assert_eq!(
        engine.memory().unwrap().guard_page(GuestAddress(DATA)),
        None
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .guard_page(GuestAddress(DATA + 4096))
            .is_some()
    );
    let handle = call(&mut engine, FileApi32::CreateFileA.id(), &args);
    assert_ne!(handle, u32::MAX);
    assert_ne!(handle, 0);
}

#[test]
fn guard_failure_preserves_registry_lstatus_and_com_hresult_conventions() {
    let mut engine = engine();
    put(&mut engine, DATA, &[0]);
    put(&mut engine, DATA + 0x2000, &0xdead_beefu32.to_le_bytes());
    guard(&mut engine, DATA, 1);
    let args = [0x8000_0001, DATA, 0, 0x20019, DATA + 0x2000];
    assert_eq!(
        call(&mut engine, Advapi32Api32::RegOpenKeyExA.id(), &args),
        998
    );
    assert_eq!(
        engine.current_windows_thread().unwrap().last_error(),
        0x1234_5678
    );
    assert_eq!(word(&engine, DATA + 0x2000), 0xdead_beef);
    assert_eq!(
        call(&mut engine, Advapi32Api32::RegOpenKeyExA.id(), &args),
        0
    );
    assert_ne!(word(&engine, DATA + 0x2000), 0xdead_beef);

    guard(&mut engine, DATA, 1);
    put(&mut engine, DATA + 0x2000, &0xfeed_faceu32.to_le_bytes());
    assert_eq!(
        call(
            &mut engine,
            Ole32Api32::CoGetClassObject.id(),
            &[DATA, 1, 0, DATA + 0x1000, DATA + 0x2000]
        ),
        0x8000_4003
    );
    assert_eq!(
        engine.current_windows_thread().unwrap().last_error(),
        0x1234_5678
    );
    assert_eq!(word(&engine, DATA + 0x2000), 0xfeed_face);
    assert_eq!(
        engine.memory().unwrap().guard_page(GuestAddress(DATA)),
        None
    );
}
