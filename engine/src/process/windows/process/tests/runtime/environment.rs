use super::*;
use crate::memory::GuestAddress;

#[test]
fn ansi_environment_snapshots_keep_their_contents_and_require_the_matching_free_api() {
    let (mut engine, _) = super::super::module_name_tests::engine();
    engine
        .process_runtime
        .environment
        .set("R3_CRT", Some("Grüße"))
        .unwrap();
    let first = engine
        .execute_process_call(api("GetEnvironmentStringsA"), &[])
        .unwrap()
        .value as u32;
    let mut bytes = [0; 15];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(first), &mut bytes)
        .unwrap();
    assert_eq!(&bytes, b"R3_CRT=Gr\x81\xe1e\0\0\0");
    engine
        .process_runtime
        .environment
        .set("R3_CRT", Some("changed"))
        .unwrap();
    let second = engine
        .execute_process_call(api("GetEnvironmentStringsA"), &[])
        .unwrap()
        .value as u32;
    assert_ne!(first, second);
    let mut retained = [0; 14];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(first), &mut retained)
        .unwrap();
    assert_eq!(&retained, b"R3_CRT=Gr\x81\xe1e\0\0");
    assert!(matches!(
        engine.execute_process_call(api("FreeEnvironmentStringsW"), &[first]),
        Err(HostError::WindowsProcess(ProcessError32::InvalidParameter))
    ));
    assert_eq!(
        engine
            .execute_process_call(api("FreeEnvironmentStringsA"), &[first])
            .unwrap()
            .value,
        1
    );
    assert!(
        engine
            .execute_process_call(api("FreeEnvironmentStringsA"), &[first])
            .is_err()
    );
    assert_eq!(
        engine
            .execute_process_call(api("FreeEnvironmentStringsA"), &[second])
            .unwrap()
            .value,
        1
    );
}
