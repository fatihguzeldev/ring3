use super::*;
#[test]
fn binary_bstr_prefix_preserves_embedded_nulls_and_odd_byte_lengths() {
    assert_eq!(
        bstr_bytes(&[b'a', 0, 0, 0, b'b', 0], 12).unwrap(),
        [6, 0, 0, 0, b'a', 0, 0, 0, b'b', 0, 0, 0]
    );
    assert_eq!(
        bstr_bytes(&[0xab, 0xcd, 0xef], 9).unwrap(),
        [3, 0, 0, 0, 0xab, 0xcd, 0xef, 0, 0]
    );
    assert_eq!(bstr_bytes(&[1; 4], 9), Err(AutomationError32::Capacity));
    assert_eq!(Oleaut32Api32::SysReAllocStringLen.stack_words(), 3);
    assert_eq!(Oleaut32Api32::from_id(0x0020001f), None);
}
#[test]
fn automation_limits_and_error_receipts_are_literal_and_bounded() {
    AutomationLimits32::default().validate().unwrap();
    assert_eq!(
        AutomationLimits32 {
            objects: 0,
            ..Default::default()
        }
        .validate(),
        Err(AutomationError32::InvalidParameter)
    );
    assert_eq!(AutomationError32::ArrayLocked.hresult(), 0x8002000d);
    assert_eq!(AutomationError32::Overflow.hresult(), 0x8002000a);
}

#[test]
fn bstr_export_ordinals_match_the_authored_x86_windows_reference() {
    let module = oleaut32_module(0x74000000);
    assert_eq!(
        module.exports[..8]
            .iter()
            .map(|e| e.ordinal.unwrap())
            .collect::<Vec<_>>(),
        [2, 4, 150, 3, 5, 6, 7, 149]
    );
    assert_eq!(
        module.exports[24..]
            .iter()
            .map(|export| export.ordinal.unwrap())
            .collect::<Vec<_>>(),
        [8, 9, 10, 11, 12, 147]
    );
    assert_eq!(Oleaut32Api32::VariantInit.stack_words(), 1);
    assert_eq!(Oleaut32Api32::VariantChangeTypeEx.stack_words(), 5);
}

#[test]
fn native_two_dimensional_layout_reverses_only_descriptor_bounds_and_keeps_input_order_for_indices()
{
    let layout = ArrayLayout32::new(
        3,
        &[
            ArrayBound32 {
                count: 2,
                lower: -3,
            },
            ArrayBound32 { count: 3, lower: 7 },
        ],
        false,
        AutomationLimits32::default(),
    )
    .unwrap();
    let mut descriptor = [0; 32];
    layout
        .write_descriptor(&mut descriptor, 0x12340000, 0)
        .unwrap();
    assert_eq!(
        &descriptor[16..32],
        &[3, 0, 0, 0, 7, 0, 0, 0, 2, 0, 0, 0, 253, 255, 255, 255]
    );
    assert_eq!(
        layout.bound(1).unwrap(),
        ArrayBound32 {
            count: 2,
            lower: -3
        }
    );
    assert_eq!(
        layout.bound(2).unwrap(),
        ArrayBound32 { count: 3, lower: 7 }
    );
    assert_eq!(layout.offset(&[-3, 7]).unwrap(), 0);
    assert_eq!(layout.offset(&[-2, 7]).unwrap(), 4);
    assert_eq!(layout.offset(&[-3, 8]).unwrap(), 8);
    assert_eq!(layout.offset(&[7, -3]), Err(AutomationError32::BadIndex));
    let vector = ArrayLayout32::new(
        3,
        &[ArrayBound32 { count: 0, lower: 0 }],
        true,
        AutomationLimits32::default(),
    )
    .unwrap();
    assert_eq!(vector.features, 0x2080);
    assert_eq!(vector.data_bytes, 0);
    assert_eq!(
        ArrayLayout32::new(
            3,
            &[ArrayBound32 {
                count: u32::MAX,
                lower: 0
            }],
            false,
            AutomationLimits32::default()
        )
        .err(),
        Some(AutomationError32::Capacity)
    );
}
