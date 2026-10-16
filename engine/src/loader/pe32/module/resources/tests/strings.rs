use super::*;
#[test]
fn string_table_returns_counted_payload_and_empty_slot_without_inventing_terminators() {
    let memory = memory();
    let mut reader = reader(&memory);
    let location = reader
        .find_exact(
            &ResourceIdentifier32::Id(6),
            &ResourceIdentifier32::Id(1),
            0x409,
        )
        .unwrap();
    assert_eq!(
        reader.string_table(location, 0).unwrap(),
        ResourceString32 {
            data_rva: 0x2002,
            units: 5
        }
    );
    assert_eq!(
        reader.string_table(location, 1).unwrap(),
        ResourceString32 {
            data_rva: 0x200e,
            units: 0
        }
    );
    assert_eq!(
        reader.string_table(location, 16),
        Err(ResourceError32::Malformed)
    );
    assert_eq!(
        reader.string_table(
            ResourceLocation32 {
                size: 3,
                ..location
            },
            0
        ),
        Err(ResourceError32::Malformed)
    );
}
