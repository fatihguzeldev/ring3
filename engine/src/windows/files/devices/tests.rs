use super::*;
#[test]
fn dos_devices_are_distinct_from_extended_literal_paths_and_description_aliases() {
    for name in [
        "nul",
        "NUL:",
        "C:\\missing\\nul.txt",
        "C:Nul.tar.gz",
        "\\\\.\\NUL",
    ] {
        assert_eq!(
            device_name(&name.encode_utf16().collect::<Vec<_>>()),
            Some(DeviceName32::Null),
            "{name}"
        );
    }
    assert_eq!(
        device_name(&r"\\?\C:\nul.txt".encode_utf16().collect::<Vec<_>>()),
        None
    );
    assert_eq!(
        device_name(&"annul".encode_utf16().collect::<Vec<_>>()),
        None
    );
    let mut context = NullDevices32::new(1).unwrap();
    let prepared = context.prepare_open(true).unwrap();
    let first = context.commit_open(prepared);
    context.retain(first).unwrap();
    context.release(first).unwrap();
    assert!(context.overlapped(first).unwrap());
    context.release(first).unwrap();
    let prepared = context.prepare_open(false).unwrap();
    let second = context.commit_open(prepared);
    assert_ne!(first, second);
    assert!(context.overlapped(first).is_err());
}
