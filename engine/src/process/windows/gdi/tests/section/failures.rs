use super::*;
#[test]
fn writable_dib_pointer_is_cleared_on_creation_failure_without_mapping_or_object_publication() {
    let mut engine = fixture();
    let writable = section(&mut engine, 4);
    let readonly = section(&mut engine, 2);
    for (section, offset, error) in [
        (0x1234, 0, 87),
        (writable, 2, 0xabcdef01),
        (readonly, 0, 87),
    ] {
        call(&mut engine, WindowsApi32::SetLastError.id(), &[0xabcdef01]);
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0x11100), &0x12345678u32.to_le_bytes())
            .unwrap();
        let token = capture(
            &mut engine,
            GdiApi32::CreateDIBSection.id(),
            &[0, 0x11080, 0, 0x11100, section, offset],
        );
        let mapped = engine.memory().unwrap().mapped_pages();
        assert_eq!(finish(&mut engine, token), 0);
        assert_eq!(word(&engine, 0x11100), 0);
        assert_eq!(engine.memory().unwrap().mapped_pages(), mapped);
        assert_eq!(engine.gdi.as_ref().unwrap().model.object_count(), 0);
        assert_eq!(
            call(&mut engine, WindowsApi32::GetLastError.id(), &[]),
            error
        );
    }
}
