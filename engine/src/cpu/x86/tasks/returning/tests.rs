use super::*;
use crate::cpu::x86::{
    SegmentDescriptorRecord,
    tasks::{TaskDescriptorTable, TaskFormat},
};

#[test]
fn selected_p6_presence_busy_limit_order_has_a_discriminating_unavailable_absent_case() {
    for (high, limit, expected) in [
        (0x0900, 0x67, Err(TaskReturnError::NotPresent(0x48))),
        (0x0100, 0x2c, Err(TaskReturnError::NotPresent(0x48))),
        (0x8100, 0x2c, Err(TaskReturnError::InvalidTss(0x48))),
        (0x8900, 0x00, Err(TaskReturnError::InvalidTss(0x48))),
        (0x0b00, 0x00, Err(TaskReturnError::NotPresent(0x48))),
        (0x8b00, 0x66, Err(TaskReturnError::InvalidTss(0x48))),
        (0x8b00, 0x67, Ok(TaskFormat::Dword)),
        (0x8300, 0x2b, Err(TaskReturnError::InvalidTss(0x48))),
        (0x8300, 0x2c, Ok(TaskFormat::Word)),
        (0x808b00, 0, Ok(TaskFormat::Dword)),
    ] {
        let mut records = [None; 10];
        records[9] = Some(SegmentDescriptorRecord::from_words(
            0x60000000 | limit,
            high,
        ));
        let table = TaskDescriptorTable::new(0x4f, &records);
        assert_eq!(
            admit_target(0x4b, &table).map(|value| value.format()),
            expected
        );
    }
}

#[test]
fn target_type_is_explicit_tss_and_descriptor_dpl_or_selector_rpl_cannot_authorize_or_deny_iret() {
    for selector in [0x48, 0x49, 0x4a, 0x4b] {
        for dpl in [0, 1, 2, 3] {
            for (kind, format, limit) in
                [(3, TaskFormat::Word, 0x2c), (11, TaskFormat::Dword, 0x67)]
            {
                let mut records = [None; 10];
                records[9] = Some(SegmentDescriptorRecord::from_words(
                    0x60000000 | limit,
                    0x8000 | (kind << 8) | (dpl << 13),
                ));
                let table = TaskDescriptorTable::new(0x4f, &records);
                let value = admit_target(selector, &table).unwrap();
                assert_eq!(
                    (value.selector(), value.format(), value.base()),
                    (selector, format, 0x6000)
                );
            }
        }
    }
    for high in [0, 0x0b00 | 0x1000, 0x8a00, 0x8f00] {
        let mut records = [None; 10];
        records[9] = Some(SegmentDescriptorRecord::from_words(0x60000067, high));
        let table = TaskDescriptorTable::new(0x4f, &records);
        assert_eq!(
            admit_target(0x4b, &table),
            Err(TaskReturnError::InvalidTss(0x48))
        );
    }
}
