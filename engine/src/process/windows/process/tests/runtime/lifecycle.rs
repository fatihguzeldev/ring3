use super::*;
use crate::memory::GuestAddress;
#[test]
fn process_priority_round_trips_and_unsupported_host_capabilities_fail_explicitly() {
    let mut engine = EngineInstance::new(16, 7).unwrap();
    engine.map(0x10000, 1, 3).unwrap();
    assert_eq!(
        engine
            .execute_process_call(api("GetPriorityClass"), &[u32::MAX])
            .unwrap()
            .value,
        0x20
    );
    assert_eq!(
        engine
            .execute_process_call(api("SetPriorityClass"), &[u32::MAX, 0x80])
            .unwrap()
            .value,
        1
    );
    assert_eq!(
        engine
            .execute_process_call(api("GetPriorityClass"), &[u32::MAX])
            .unwrap()
            .value,
        0x80
    );
    assert_eq!(
        engine
            .execute_process_call(api("GetExitCodeProcess"), &[u32::MAX, 0x10000])
            .unwrap()
            .value,
        1
    );
    let mut status = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x10000), &mut status)
        .unwrap();
    assert_eq!(u32::from_le_bytes(status), 259);
    for (name, args, want) in [
        ("CreateProcessA", vec![0; 10], 50),
        ("CreateProcessW", vec![0; 10], 50),
        ("SetLocalTime", vec![0], 1314),
        ("Beep", vec![440, 10], 50),
    ] {
        let result = engine.execute_process_call(api(name), &args).unwrap();
        assert_eq!(result.value, 0);
        assert_eq!(result.error, Some(want));
    }
}

#[test]
fn process_version_uses_pe_subsystem_version_instead_of_operating_system_version() {
    let mut image = crate::process::windows::process::test_image_for_startup();
    let nt = u32::from_le_bytes(image[0x3c..0x40].try_into().unwrap()) as usize;
    let optional = nt + 24;
    image[optional + 40..optional + 44].copy_from_slice(&[6, 0, 2, 0]);
    image[optional + 48..optional + 52].copy_from_slice(&[4, 0, 1, 0]);
    let mut engine = EngineInstance::new(128, 7).unwrap();
    engine.register_guest_module("version.exe", &image).unwrap();
    engine
        .start_module_process("version.exe", None, Default::default())
        .unwrap();
    assert_eq!(
        engine
            .execute_process_call(api("GetProcessVersion"), &[0])
            .unwrap()
            .value,
        0x0004_0001
    );
}
