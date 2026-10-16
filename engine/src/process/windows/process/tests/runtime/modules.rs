use super::*;
use crate::memory::GuestAddress;

const OUTPUT: u32 = 0x30000;

fn handle(engine: &EngineInstance) -> u32 {
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(OUTPUT), &mut bytes)
        .unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn address_lookup_retains_ordinary_modules_and_pinning_survives_repeated_releases() {
    let (mut engine, _) = super::super::module_name_tests::engine();
    let query = api("GetModuleHandleExA");
    assert_eq!(
        engine
            .execute_process_call(query, &[4, 0x800010, OUTPUT])
            .unwrap()
            .value,
        1
    );
    assert_eq!(handle(&engine), 0x800000);
    engine.free_library_module(0x800000).unwrap();
    assert_eq!(
        engine.module_handle(Some("ordinary.dll")).unwrap(),
        Some(0x800000)
    );
    assert_eq!(
        engine
            .execute_process_call(query, &[5, 0x800010, OUTPUT])
            .unwrap()
            .value,
        1
    );
    for _ in 0..4 {
        engine.free_library_module(0x800000).unwrap();
    }
    assert_eq!(
        engine.module_handle(Some("ordinary.dll")).unwrap(),
        Some(0x800000)
    );
}

#[test]
fn failed_handle_lookup_clears_output_without_retaining_resource_only_modules() {
    let (mut engine, resource) = super::super::module_name_tests::engine();
    let query = api("GetModuleHandleExW");
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(OUTPUT), &0xfeed_beefu32.to_le_bytes())
        .unwrap();
    let reply = engine
        .execute_process_call(query, &[4, resource & !3, OUTPUT])
        .unwrap();
    assert_eq!(
        (reply.value, reply.error, handle(&engine)),
        (0, Some(126), 0)
    );
    assert!(matches!(
        engine.execute_process_call(query, &[3, 0, OUTPUT]),
        Err(HostError::WindowsProcess(ProcessError32::InvalidParameter))
    ));
    assert!(
        engine
            .execute_process_call(query, &[4, 0x800010, u32::MAX])
            .is_err()
    );
    engine.free_library_module(0x800000).unwrap();
    assert_eq!(engine.module_handle(Some("ordinary.dll")).unwrap(), None);
}
