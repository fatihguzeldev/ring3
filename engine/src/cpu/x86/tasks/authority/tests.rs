use super::*;
use crate::cpu::x86::tasks::{TaskReturnError, prepare_return};

#[test]
fn cached_current_tss_is_independent_of_live_current_descriptor_and_target_rpl() {
    let current = TaskRegister::from_cached(
        0x43,
        SegmentDescriptorRecord::from_words(0x50000067, 0x8b00),
    )
    .unwrap();
    let mut records = [None; 10];
    records[8] = Some(SegmentDescriptorRecord::from_words(0x90000067, 0x8b00));
    records[9] = Some(SegmentDescriptorRecord::from_words(0x60000067, 0x8b00));
    let current_bytes = [0x4b, 0];
    let authority = TaskAuthority::new(
        current,
        TaskDescriptorTable::new(0x4f, &records),
        &current_bytes,
    );
    let prepared = prepare_return(Some(&authority)).unwrap();
    assert_eq!(
        (
            prepared.current().base(),
            prepared.target().base(),
            prepared.target().selector()
        ),
        (0x5000, 0x6000, 0x4b)
    );
    assert_eq!(current_bytes, [0x4b, 0]);
    assert_eq!(records[8].unwrap().to_words(), (0x90000067, 0x8b00));
}

#[test]
fn omitted_bytes_or_slots_are_not_zero_backlinks_or_zero_descriptors() {
    assert_eq!(prepare_return(None), Err(TaskReturnError::MissingAuthority));
    let current = TaskRegister::from_cached(
        0x43,
        SegmentDescriptorRecord::from_words(0x50000067, 0x8b00),
    )
    .unwrap();
    let records = [None; 10];
    for bytes in [&[][..], &[0x4b]] {
        let authority =
            TaskAuthority::new(current, TaskDescriptorTable::new(0x4f, &records), bytes);
        assert_eq!(
            prepare_return(Some(&authority)),
            Err(TaskReturnError::MissingAuthority)
        );
    }
    let bytes = [0x4b, 0];
    let authority = TaskAuthority::new(current, TaskDescriptorTable::new(0x4f, &records), &bytes);
    assert_eq!(
        prepare_return(Some(&authority)),
        Err(TaskReturnError::MissingAuthority)
    );
    let mut explicit = records;
    explicit[9] = Some(SegmentDescriptorRecord::from_words(0, 0));
    let authority = TaskAuthority::new(current, TaskDescriptorTable::new(0x4f, &explicit), &bytes);
    assert_eq!(
        prepare_return(Some(&authority)),
        Err(TaskReturnError::InvalidTss(0x48))
    );
    let zero = [0, 0];
    let authority = TaskAuthority::new(current, TaskDescriptorTable::new(0x4f, &records), &zero);
    assert_eq!(
        prepare_return(Some(&authority)),
        Err(TaskReturnError::InvalidTss(0))
    );
}

#[test]
fn cached_current_limit_precedes_backlink_bytes_and_target_geometry_precedes_missing_slot() {
    let current = TaskRegister::from_cached(
        0x43,
        SegmentDescriptorRecord::from_words(0x50000000, 0x8b00),
    )
    .unwrap();
    let records = [];
    let authority = TaskAuthority::new(current, TaskDescriptorTable::new(0x4f, &records), &[]);
    assert_eq!(
        prepare_return(Some(&authority)),
        Err(TaskReturnError::InvalidTss(0x40))
    );
    let short = TaskRegister::from_cached(
        0x43,
        SegmentDescriptorRecord::from_words(0x50000001, 0x8b00),
    )
    .unwrap();
    let authority = TaskAuthority::new(short, TaskDescriptorTable::new(0x4f, &records), &[0x4b, 0]);
    assert_eq!(
        prepare_return(Some(&authority)),
        Err(TaskReturnError::MissingAuthority)
    );
    let current = TaskRegister::from_cached(
        0x43,
        SegmentDescriptorRecord::from_words(0x50000067, 0x8b00),
    )
    .unwrap();
    for (bytes, limit, error) in [([0x4b, 0], 0x4e, 0x48), ([0x4f, 0], 0xffff, 0x4c)] {
        let authority =
            TaskAuthority::new(current, TaskDescriptorTable::new(limit, &records), &bytes);
        assert_eq!(
            prepare_return(Some(&authority)),
            Err(TaskReturnError::InvalidTss(error))
        );
    }
}
