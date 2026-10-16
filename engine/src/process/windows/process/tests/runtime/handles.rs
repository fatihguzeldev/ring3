use super::*;
use crate::memory::GuestAddress;

#[test]
fn standard_handle_replacement_preserves_windows_opaque_handle_semantics_and_updates_the_peb() {
    let (mut engine, _) = super::super::module_name_tests::engine();
    let get = api("GetStdHandle");
    let set = api("SetStdHandle");
    assert_eq!(
        engine
            .execute_process_call(get, &[0xffff_fff6])
            .unwrap()
            .value,
        0
    );
    assert_eq!(
        engine
            .execute_process_call(set, &[0xffff_fff5, 0x12345678])
            .unwrap()
            .value,
        1
    );
    assert_eq!(
        engine
            .execute_process_call(get, &[0xffff_fff5])
            .unwrap()
            .value,
        0x12345678
    );
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(
            GuestAddress(engine.process_peb_address().unwrap() + 0x11c),
            &mut bytes,
        )
        .unwrap();
    assert_eq!(u32::from_le_bytes(bytes), 0x12345678);
    assert_eq!(
        engine.execute_process_call(get, &[0]).unwrap().value,
        u64::from(u32::MAX)
    );
    assert_eq!(engine.execute_process_call(set, &[0, 7]).unwrap().value, 0);
    assert_eq!(
        engine
            .execute_process_call(get, &[0xffff_fff5])
            .unwrap()
            .value,
        0x12345678
    );
    engine
        .execute_process_call(api("GetStartupInfoW"), &[0x30000])
        .unwrap();
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x30000 + 60), &mut bytes)
        .unwrap();
    assert_eq!(u32::from_le_bytes(bytes), 0x12345678);
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x30000 + 44), &mut bytes)
        .unwrap();
    assert_eq!(u32::from_le_bytes(bytes), 0);
    let peb_output = GuestAddress(engine.process_peb_address().unwrap() + 0x11c);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(peb_output, &0x87654321u32.to_le_bytes())
        .unwrap();
    assert_eq!(
        engine
            .execute_process_call(get, &[0xffff_fff5])
            .unwrap()
            .value,
        0x87654321
    );
}
