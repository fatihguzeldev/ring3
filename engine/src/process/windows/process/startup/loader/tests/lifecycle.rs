use super::*;

#[test]
fn live_lists_keep_entry_addresses_and_exclude_executable_from_initialization_order() {
    let (catalog, mut set, mut memory, mut startup) = fixture();
    let executable = list(&memory, 0x20c)[0];
    assert_eq!(list(&memory, 0x214), vec![executable]);
    assert!(list(&memory, 0x21c).is_empty());
    assert_eq!(
        name(&memory, executable + 36),
        "C:\\game\\main.exe".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(
        name(&memory, executable + 44),
        "main.exe".encode_utf16().collect::<Vec<_>>()
    );
    let first = load(&catalog, &mut set, &mut memory, &mut startup, "first.dll");
    let entry = list(&memory, 0x20c)[1];
    assert_eq!(list(&memory, 0x214), vec![executable, entry]);
    assert_eq!(list(&memory, 0x21c), vec![entry]);
    assert_eq!(
        name(&memory, entry + 36),
        "W:\\System\\first.dll".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(word(&memory, entry + 24), set.get(first).unwrap().base());
    let second = load(&catalog, &mut set, &mut memory, &mut startup, "second.dll");
    let second_entry = list(&memory, 0x20c)[2];
    unload(&mut set, &mut memory, &mut startup, first);
    assert_eq!(list(&memory, 0x20c), vec![executable, second_entry]);
    assert_eq!(list(&memory, 0x21c), vec![second_entry]);
    assert!(set.get(first).is_none());
    assert!(set.get(second).is_some());
}

#[test]
fn unload_recycles_entry_and_name_storage_across_repeated_loads() {
    let (catalog, mut set, mut memory, mut startup) = fixture();
    let first = load(&catalog, &mut set, &mut memory, &mut startup, "first.dll");
    let address = list(&memory, 0x20c)[1];
    let bump = startup.bump;
    unload(&mut set, &mut memory, &mut startup, first);
    let mut previous = first;
    for _ in 0..64 {
        let id = load(&catalog, &mut set, &mut memory, &mut startup, "first.dll");
        assert!(id.get() > previous.get());
        assert_eq!(list(&memory, 0x20c)[1], address);
        assert_eq!(startup.bump, bump);
        unload(&mut set, &mut memory, &mut startup, id);
        previous = id;
    }
}
