use super::*;
fn key(offset: u32, scan: u16) -> FormatObject32 {
    FormatObject32 {
        offset,
        selector: ObjectSelector32::Key,
        kind: ObjectType32::Button,
        instance: Some(scan),
        aspect: 0,
        optional: false,
    }
}
#[test]
fn custom_keyboard_offsets_are_not_confused_with_scancode_or_structure_order() {
    let format = DataFormat32::new(
        DeviceDescription32::keyboard(),
        8,
        false,
        &[key(6, 30), key(3, 31)],
    )
    .unwrap();
    assert_eq!(format.offset_for(DeviceObject32::Key(30)), Some(6));
    assert_eq!(format.object_by_offset(3), Some(DeviceObject32::Key(31)));
    assert!(
        DataFormat32::new(
            DeviceDescription32::keyboard(),
            8,
            false,
            &[key(7, 30), key(7, 31)]
        )
        .is_err()
    );
}
#[test]
fn optional_missing_controls_have_owned_ranges_and_cannot_alias_present_objects() {
    let missing = FormatObject32 {
        offset: 4,
        selector: ObjectSelector32::Axis(7),
        kind: ObjectType32::Axis,
        instance: Some(7),
        aspect: 0,
        optional: true,
    };
    let format = DataFormat32::new(DeviceDescription32::mouse(), 8, true, &[missing]).unwrap();
    assert_eq!(format.bindings()[0].object, None);
    assert!(DataFormat32::new(DeviceDescription32::mouse(), 7, true, &[missing]).is_err());
}
