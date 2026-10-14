use super::*;

#[test]
fn dword_fields_decode_literally_without_losing_reserved_or_static_bytes() {
    let mut bytes = [0xa5; 112];
    bytes[0..2].copy_from_slice(&[0x4b, 0]);
    for (offset, value) in [
        (28, 0x12345018_u32),
        (32, 0x89abcdef),
        (36, 0x003f7fd7),
        (40, 0x11111111),
        (44, 0x22222222),
        (48, 0x33333333),
        (52, 0x44444444),
        (56, 0x55555555),
        (60, 0x66666666),
        (64, 0x77777777),
        (68, 0x88888888),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    for (offset, value) in [
        (72, 0x2b_u16),
        (76, 0x1b),
        (80, 0x23),
        (84, 0x33),
        (88, 0x3b),
        (92, 0x43),
        (96, 0x50),
        (100, 0xfe01),
        (102, 0x1234),
    ] {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    let before = bytes;
    let record = TaskRecord::new(TaskFormat::Dword, &bytes).unwrap();
    assert_eq!(record.backlink(), 0x4b);
    assert_eq!(
        record.decode(),
        TaskState::Dword(TaskState32 {
            eip: 0x89abcdef,
            flags: 0x003f7fd7,
            general: [
                0x11111111, 0x22222222, 0x33333333, 0x44444444, 0x55555555, 0x66666666, 0x77777777,
                0x88888888
            ],
            selectors: [0x2b, 0x1b, 0x23, 0x33, 0x3b, 0x43],
            ldtr: 0x50,
            cr3: 0x12345018,
            task_trap: true,
            io_map_base: 0x1234,
        })
    );
    assert_eq!(bytes, before);
}

#[test]
fn dword_return_save_changes_only_dynamic_fields_and_clears_only_nt() {
    let mut bytes = [0xa5; 128];
    let save = TaskSave {
        next_eip: 0x1003,
        flags: 0x003f7fd7,
        general: [
            0x11111111, 0x22222222, 0x33333333, 0x44444444, 0x55555555, 0x66666666, 0x77777777,
            0x88888888,
        ],
        selectors: [0x2b, 0x1b, 0x23, 0x33, 0x3b, 0x43],
    };
    encode_return(TaskFormat::Dword, &save, &mut bytes).unwrap();
    let mut expected = [0xa5; 128];
    expected[32..40].copy_from_slice(&[3, 0x10, 0, 0, 0xd7, 0x3f, 0x3f, 0]);
    expected[40..72].copy_from_slice(&[
        0x11, 0x11, 0x11, 0x11, 0x22, 0x22, 0x22, 0x22, 0x33, 0x33, 0x33, 0x33, 0x44, 0x44, 0x44,
        0x44, 0x55, 0x55, 0x55, 0x55, 0x66, 0x66, 0x66, 0x66, 0x77, 0x77, 0x77, 0x77, 0x88, 0x88,
        0x88, 0x88,
    ]);
    for (offset, value) in [
        (72, [0x2b, 0]),
        (76, [0x1b, 0]),
        (80, [0x23, 0]),
        (84, [0x33, 0]),
        (88, [0x3b, 0]),
        (92, [0x43, 0]),
    ] {
        expected[offset..offset + 2].copy_from_slice(&value);
    }
    assert_eq!(
        bytes, expected,
        "backlink, CR3, LDT, privilege stacks, reserved words and tail remain exact"
    );
}

#[test]
fn word_record_keeps_narrow_state_typed_and_save_truncates_without_static_writes() {
    let mut bytes = [0xa5; 52];
    bytes[0..2].copy_from_slice(&[0x4b, 0]);
    bytes[42..44].copy_from_slice(&[0x58, 0]);
    let save = TaskSave {
        next_eip: 0xabcdef01,
        flags: 0x003f7fd7,
        general: [
            0x11223344, 0x55667788, 0x99aabbcc, 0xddeeff00, 0x13572468, 0x24681357, 0xabcd9876,
            0x89abcdef,
        ],
        selectors: [0x2b, 0x1b, 0x23, 0x33, 0x3b, 0x43],
    };
    encode_return(TaskFormat::Word, &save, &mut bytes).unwrap();
    let mut expected = [0xa5; 52];
    expected[0..2].copy_from_slice(&[0x4b, 0]);
    expected[14..44].copy_from_slice(&[
        1, 0xef, 0xd7, 0x3f, 0x44, 0x33, 0x88, 0x77, 0xcc, 0xbb, 0, 0xff, 0x68, 0x24, 0x57, 0x13,
        0x76, 0x98, 0xef, 0xcd, 0x2b, 0, 0x1b, 0, 0x23, 0, 0x33, 0, 0x58, 0,
    ]);
    assert_eq!(bytes, expected);
    let record = TaskRecord::new(TaskFormat::Word, &bytes).unwrap();
    assert_eq!(record.backlink(), 0x4b);
    assert_eq!(
        record.decode(),
        TaskState::Word(TaskState16 {
            ip: 0xef01,
            flags: 0x3fd7,
            general: [
                0x3344, 0x7788, 0xbbcc, 0xff00, 0x2468, 0x1357, 0x9876, 0xcdef
            ],
            selectors: [0x2b, 0x1b, 0x23, 0x33],
            ldtr: 0x58,
        })
    );
}

#[test]
fn incomplete_images_are_missing_authority_and_encode_is_neutral() {
    for (format, lengths) in [
        (TaskFormat::Word, [0, 1, 43]),
        (TaskFormat::Dword, [0, 2, 103]),
    ] {
        for length in lengths {
            let mut bytes = vec![0xa5; length];
            let before = bytes.clone();
            assert_eq!(
                TaskRecord::new(format, &bytes).unwrap_err(),
                TaskReturnError::MissingAuthority
            );
            assert_eq!(
                encode_return(
                    format,
                    &TaskSave {
                        next_eip: 0,
                        flags: 0,
                        general: [0; 8],
                        selectors: [0; 6],
                    },
                    &mut bytes
                ),
                Err(TaskReturnError::MissingAuthority)
            );
            assert_eq!(bytes, before);
        }
    }
    for (format, length) in [(TaskFormat::Word, 44), (TaskFormat::Dword, 104)] {
        let mut bytes = vec![0xa5; length];
        assert!(TaskRecord::new(format, &bytes).is_ok());
        encode_return(
            format,
            &TaskSave {
                next_eip: 0x1003,
                flags: 2,
                general: [0; 8],
                selectors: [0; 6],
            },
            &mut bytes,
        )
        .unwrap();
    }
}
