use super::*;
use crate::cpu::x86::tasks::{TaskFormat, TaskRecord, TaskState32};

#[test]
fn word_load_uses_only_its_record_and_explicit_upper_word_model() {
    let mut bytes = [0xa5; 104];
    let words = [
        (14, 0xff01),
        (16, 0x4fd7),
        (18, 0x8001),
        (20, 0xffff),
        (22, 0x1234),
        (24, 0xabcd),
        (26, 0x7000),
        (28, 0x8000),
        (30, 0),
        (32, 0x55aa),
        (34, 0x23),
        (36, 0x1b),
        (38, 0x23),
        (40, 0x2b),
        (42, 0x30),
    ];
    for (offset, value) in words {
        bytes[offset..offset + 2].copy_from_slice(&u16::to_le_bytes(value));
    }
    let narrow = TaskRecord::new(TaskFormat::Word, &bytes[..44])
        .unwrap()
        .decode();
    let loaded = TaskLoadState32::from_record(narrow);
    assert_eq!(
        loaded,
        TaskLoadState32 {
            eip: 0xff01,
            flags: 0x4fd7,
            general: [0x8001, 0xffff, 0x1234, 0xabcd, 0x7000, 0x8000, 0, 0x55aa],
            selectors: [0x23, 0x1b, 0x23, 0x2b, 0, 0],
            ldtr: 0x30,
            task_trap: false,
        }
    );
    // bytes that would own dword fs/gs/cr3/t are never word-task inputs.
    bytes[44..].fill(0xff);
    assert_eq!(
        TaskLoadState32::from_record(TaskRecord::new(TaskFormat::Word, &bytes).unwrap().decode()),
        loaded
    );
    assert_eq!(loaded.flags & 0xffff_0000, 0);
}

#[test]
fn dword_load_keeps_full_execution_fields_and_its_trap_bit() {
    let source = TaskState32 {
        eip: 0x1234_5678,
        flags: 0x250fd7,
        general: [
            0x8000_0001,
            0xffff_ffff,
            0x1234_5678,
            0xabcd_ef01,
            0xdead_7000,
            0x89ab_cdef,
            0,
            0x55aa_00ff,
        ],
        selectors: [0x23, 0x1b, 0x23, 0x2b, 0x3b, 0x53],
        ldtr: 0x30,
        cr3: 0x1234_5000,
        task_trap: true,
        io_map_base: 0x1234,
    };
    assert_eq!(
        TaskLoadState32::from_record(TaskState::Dword(source)),
        TaskLoadState32 {
            eip: source.eip,
            flags: source.flags,
            general: source.general,
            selectors: source.selectors,
            ldtr: source.ldtr,
            task_trap: true,
        }
    );
}
