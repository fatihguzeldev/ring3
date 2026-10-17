use super::*;
#[test]
fn three_level_lookup_enumeration_and_payload_outside_directory_are_exact() {
    let memory = memory();
    let mut reader = reader(&memory);
    assert_eq!(
        reader.types().unwrap(),
        [ResourceIdentifier32::Id(6), ResourceIdentifier32::Id(10)]
    );
    let mut reader = super::reader(&memory);
    assert_eq!(
        reader.names(&ResourceIdentifier32::Id(10)).unwrap(),
        [ResourceIdentifier32::Id(7)]
    );
    let mut reader = super::reader(&memory);
    assert_eq!(
        reader
            .languages(&ResourceIdentifier32::Id(10), &ResourceIdentifier32::Id(7))
            .unwrap(),
        [0x409]
    );
    let mut reader = super::reader(&memory);
    assert_eq!(
        reader
            .find_exact(
                &ResourceIdentifier32::Id(10),
                &ResourceIdentifier32::Id(7),
                0x409
            )
            .unwrap(),
        ResourceLocation32 {
            entry_rva: 0x10c0,
            data_rva: 0x2100,
            size: 3,
            code_page: 0
        }
    );
    let mut reader = super::reader(&memory);
    assert_eq!(
        reader.find_exact(
            &ResourceIdentifier32::Id(10),
            &ResourceIdentifier32::Id(7),
            0x407
        ),
        Err(ResourceError32::LanguageNotFound)
    );
}
#[test]
fn malformed_leaf_overflow_and_directory_cycle_cannot_read_outside_image() {
    let mut memory = memory();
    memory
        .write(GuestAddress(BASE + 0x10c0), &0x2fffu32.to_le_bytes())
        .unwrap();
    assert_eq!(
        reader(&memory).find_exact(
            &ResourceIdentifier32::Id(10),
            &ResourceIdentifier32::Id(7),
            0x409
        ),
        Err(ResourceError32::Malformed)
    );
    memory
        .write(GuestAddress(BASE + 0x10a4), &0x80000000u32.to_le_bytes())
        .unwrap();
    assert_eq!(
        reader(&memory).find_exact(
            &ResourceIdentifier32::Id(10),
            &ResourceIdentifier32::Id(7),
            0x409
        ),
        Err(ResourceError32::Malformed)
    );
}
#[test]
fn named_entries_use_counted_utf16_case_sensitive_order_and_bounded_limits() {
    let mut memory = memory();
    memory
        .write(GuestAddress(BASE + 0x100c), &[1, 0, 2, 0])
        .unwrap();
    let mut entries = [0; 24];
    for (index, (name, target)) in [
        (0x800000e0u32, 0x80000050u32),
        (6, 0x80000030),
        (10, 0x80000050),
    ]
    .into_iter()
    .enumerate()
    {
        entries[index * 8..index * 8 + 4].copy_from_slice(&name.to_le_bytes());
        entries[index * 8 + 4..index * 8 + 8].copy_from_slice(&target.to_le_bytes());
    }
    memory.write(GuestAddress(BASE + 0x1010), &entries).unwrap();
    memory
        .write(
            GuestAddress(BASE + 0x10e0),
            &[4, 0, b'T', 0, b'Y', 0, b'P', 0, b'E', 0],
        )
        .unwrap();
    assert!(
        reader(&memory)
            .find_exact(
                &ResourceIdentifier32::Name("TYPE".encode_utf16().collect()),
                &ResourceIdentifier32::Id(7),
                0x409
            )
            .is_ok()
    );
    assert_eq!(
        reader(&memory).find_exact(
            &ResourceIdentifier32::Name("type".encode_utf16().collect()),
            &ResourceIdentifier32::Id(7),
            0x409
        ),
        Err(ResourceError32::TypeNotFound)
    );
    let mut reader = PeResources32::new(
        &memory,
        BASE,
        0x3000,
        Directory32 {
            rva: 0x1000,
            size: 256,
        },
        ParserLimits32 {
            metadata_entries: 2,
            string_bytes: 128,
        },
    )
    .unwrap();
    assert_eq!(reader.types(), Err(ResourceError32::Capacity));
}

#[test]
fn selected_language_is_returned_for_explicit_and_first_available_fallbacks() {
    let mut memory = memory();
    let kind = ResourceIdentifier32::Id(10);
    let name = ResourceIdentifier32::Id(7);
    let (location, language) = reader(&memory)
        .find_languages_with_language(&kind, &name, &[0x411, 0x409], true)
        .unwrap();
    assert_eq!(language, 0x409);
    assert_eq!(location.entry_rva, 0x10c0);
    let (fallback, language) = reader(&memory)
        .find_languages_with_language(&kind, &name, &[0x411], true)
        .unwrap();
    assert_eq!((fallback, language), (location, 0x409));
    assert_eq!(
        reader(&memory)
            .find_languages(&kind, &name, &[0x411], true)
            .unwrap(),
        location
    );
    memory
        .write(GuestAddress(BASE + 0x10a0), &0u32.to_le_bytes())
        .unwrap();
    let (neutral, language) = reader(&memory)
        .find_languages_with_language(&kind, &name, &[0, 0x409], true)
        .unwrap();
    assert_eq!((neutral, language), (location, 0));
}
