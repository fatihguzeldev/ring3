use super::*;

#[test]
fn cached_system_base_limit_type_and_selector_are_lossless() {
    let raw = SegmentDescriptorRecord::from_words(0xfff00067, 0xff008bff);
    let register = TaskRegister::from_cached(0x43, raw).unwrap();
    assert_eq!(
        (
            register.selector(),
            register.base(),
            register.limit(),
            register.format()
        ),
        (0x43, 0xfffffff0, 0x67, TaskFormat::Dword)
    );
    assert_eq!(register.descriptor(), raw);
    let granular = SegmentDescriptorRecord::from_words(0xfff00067, 0xff808bff);
    assert_eq!(
        TaskRegister::from_cached(0x43, granular).unwrap().limit(),
        0x67fff
    );
}

#[test]
fn cached_register_requires_explicit_nonnull_gdt_busy_present_tss_but_not_minimum_limit() {
    let raw = SegmentDescriptorRecord::from_words(0x50000000, 0x00008b00);
    assert_eq!(TaskRegister::from_cached(0x43, raw).unwrap().limit(), 0);
    for selector in [0, 1, 2, 3, 4, 0x47] {
        assert_eq!(
            TaskRegister::from_cached(selector, raw),
            Err(TaskReturnError::InvalidTss(u32::from(selector & !3)))
        );
    }
    for high in [0x8900, 0x8300 | 0x1000, 0x8000] {
        assert_eq!(
            TaskRegister::from_cached(0x43, SegmentDescriptorRecord::from_words(0x50000067, high)),
            Err(TaskReturnError::InvalidTss(0x40))
        );
    }
    assert_eq!(
        TaskRegister::from_cached(
            0x43,
            SegmentDescriptorRecord::from_words(0x50000067, 0x0b00)
        ),
        Err(TaskReturnError::NotPresent(0x40))
    );
}
