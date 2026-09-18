use ring3_core::{
    FileOffset, PeKind, PeRvaError, PeTlsCallbackEntry, PeTlsCallbackError, PeTlsCallbackLimits,
    PeTlsCallbackTable, PeTlsCallbacks, PeTlsDirectory, PeTlsDirectoryError,
    RelativeVirtualAddress, parse_pe_tls_callbacks,
};

fn put(bytes: &mut [u8], offset: usize, value: u64, width: usize) {
    bytes[offset..offset + width].copy_from_slice(&value.to_le_bytes()[..width]);
}

fn width(plus: bool) -> usize {
    if plus { 8 } else { 4 }
}

fn base(plus: bool) -> u64 {
    if plus { 0x1_4000_0000 } else { 0x40_0000 }
}

fn fixed(plus: bool) -> usize {
    if plus { 112 } else { 96 }
}

fn section(bytes: &mut [u8], plus: bool, index: usize, fields: [u32; 4]) {
    let start = 152 + fixed(plus) + 80 + index * 40;
    for (offset, value) in [8, 12, 16, 20].into_iter().zip(fields) {
        put(bytes, start + offset, u64::from(value), 4);
    }
}

fn image(plus: bool, entries: &[u64]) -> Vec<u8> {
    let span = ((entries.len() + 1) * width(plus)).max(128);
    let mut bytes = vec![0; 640 + span];
    bytes[..2].copy_from_slice(b"MZ");
    put(&mut bytes, 60, 128, 4);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    put(&mut bytes, 134, 3, 2);
    put(&mut bytes, 148, u64::try_from(fixed(plus) + 80).unwrap(), 2);
    put(&mut bytes, 152, if plus { 0x20b } else { 0x10b }, 2);
    put(
        &mut bytes,
        152 + if plus { 24 } else { 28 },
        base(plus),
        width(plus),
    );
    put(&mut bytes, 212, 512, 4);
    put(&mut bytes, 152 + fixed(plus) - 4, 10, 4);
    put(&mut bytes, 152 + fixed(plus) + 72, 0x1000, 4);
    put(
        &mut bytes,
        152 + fixed(plus) + 76,
        if plus { 40 } else { 24 },
        4,
    );
    section(&mut bytes, plus, 0, [128, 0x1000, 128, 512]);
    let span = u32::try_from(span).unwrap();
    section(&mut bytes, plus, 1, [span, 0x2000, span, 640]);
    section(&mut bytes, plus, 2, [128, 0x20_0000, 128, 512]);
    put(
        &mut bytes,
        512 + 3 * width(plus),
        base(plus) + 0x2000,
        width(plus),
    );
    for (index, value) in entries.iter().enumerate() {
        put(&mut bytes, 640 + index * width(plus), *value, width(plus));
    }
    bytes
}

fn expected(plus: bool, words: &[u64]) -> PeTlsCallbacks {
    let width = u32::try_from(width(plus)).unwrap();
    let end = u32::try_from(words.len()).unwrap() * width;
    PeTlsCallbacks {
        directory: PeTlsDirectory {
            kind: if plus { PeKind::Pe32Plus } else { PeKind::Pe32 },
            directory_rva: RelativeVirtualAddress::new(0x1000),
            directory_file_offset: FileOffset::new(512),
            directory_size: if plus { 40 } else { 24 },
            start_address_of_raw_data: 0,
            end_address_of_raw_data: 0,
            address_of_index: 0,
            address_of_callbacks: base(plus) + 0x2000,
            size_of_zero_fill: 0,
            characteristics: 0,
        },
        image_base: base(plus),
        table: Some(PeTlsCallbackTable {
            table_rva: RelativeVirtualAddress::new(0x2000),
            table_file_offset: FileOffset::new(640),
            entries: words
                .iter()
                .enumerate()
                .map(|(index, raw_va)| {
                    let index = u32::try_from(index).unwrap();
                    PeTlsCallbackEntry {
                        table_index: index,
                        slot_rva: RelativeVirtualAddress::new(0x2000 + index * width),
                        slot_file_offset: FileOffset::new(640 + u64::from(index * width)),
                        raw_va: *raw_va,
                    }
                })
                .collect(),
            terminator_rva: RelativeVirtualAddress::new(0x2000 + end),
            terminator_file_offset: FileOffset::new(640 + u64::from(end)),
        }),
    }
}

fn read(bytes: &[u8], max_callbacks: u16) -> Result<Option<PeTlsCallbacks>, PeTlsCallbackError> {
    parse_pe_tls_callbacks(bytes, PeTlsCallbackLimits { max_callbacks })
}

#[test]
fn absent_null_and_present_empty_tables_remain_distinct() {
    for plus in [false, true] {
        let mut bytes = image(plus, &[]);
        assert_eq!(read(&bytes, 0), Ok(Some(expected(plus, &[]))));
        put(&mut bytes, 512 + 3 * width(plus), 0, width(plus));
        let mut null = expected(plus, &[]);
        null.directory.address_of_callbacks = 0;
        null.table = None;
        assert_eq!(read(&bytes, 0), Ok(Some(null)));
        put(&mut bytes, 152 + fixed(plus) + 72, 0, 8);
        assert_eq!(read(&bytes, 0), Ok(None));
    }
}

#[test]
fn raw_addresses_keep_order_duplicates_and_ownership_after_input_drop() {
    for plus in [false, true] {
        let max = if plus { u64::MAX } else { u64::from(u32::MAX) };
        let words = [max, 1, max];
        let mut bytes = image(plus, &words);
        let before = bytes.clone();
        let result = read(&bytes, 3);
        assert_eq!(bytes, before);
        assert_eq!(read(&bytes, 3), result);
        bytes.fill(0);
        drop(bytes);
        assert_eq!(result, Ok(Some(expected(plus, &words))));
    }
}

#[test]
fn terminator_backing_precedes_count_and_requires_one_complete_region() {
    for plus in [false, true] {
        let mut bytes = image(plus, &[1, 2]);
        assert_eq!(
            read(&bytes, 1),
            Err(PeTlsCallbackError::CallbackLimitExceeded { index: 1, limit: 1 })
        );
        let width = u32::try_from(width(plus)).unwrap();
        section(&mut bytes, plus, 1, [2 * width, 0x2000, width, 640]);
        let table_rva = RelativeVirtualAddress::new(0x2000);
        assert_eq!(
            read(&bytes, 1),
            Err(PeTlsCallbackError::TableRange {
                table_index: 1,
                table_rva,
                length: 2 * width,
                cause: PeRvaError::NotFileBacked {
                    start: table_rva,
                    length: 2 * width,
                    section_index: 1
                },
            })
        );
        section(&mut bytes, plus, 1, [width, 0x2000, width, 640]);
        section(&mut bytes, plus, 2, [width, 0x2000 + width, width, 512]);
        assert_eq!(
            read(&bytes, 1),
            Err(PeTlsCallbackError::TableRange {
                table_index: 1,
                table_rva,
                length: 2 * width,
                cause: PeRvaError::CrossesRegionBoundary {
                    start: table_rva,
                    length: 2 * width
                },
            })
        );
    }
}

#[test]
fn preferred_base_subtraction_is_checked_but_raw_targets_are_not() {
    for plus in [false, true] {
        let mut bytes = image(plus, &[]);
        let address = base(plus) - 1;
        put(&mut bytes, 512 + 3 * width(plus), address, width(plus));
        assert_eq!(
            read(&bytes, 0),
            Err(PeTlsCallbackError::CallbackAddressBelowImageBase {
                address,
                image_base: base(plus)
            })
        );
        put(&mut bytes, 512 + 3 * width(plus), base(plus), width(plus));
        assert_eq!(
            read(&bytes, 0),
            Err(PeTlsCallbackError::CallbackLimitExceeded { index: 0, limit: 0 })
        );
    }
    let mut bytes = image(true, &[]);
    let address = base(true) + (1_u64 << 32);
    put(&mut bytes, 536, address, 8);
    assert_eq!(
        read(&bytes, 0),
        Err(PeTlsCallbackError::CallbackRvaOverflow {
            address,
            image_base: base(true)
        })
    );
}

#[test]
fn fixed_directory_failure_is_preserved_before_callback_work() {
    for plus in [false, true] {
        let mut bytes = image(plus, &[]);
        put(&mut bytes, 152 + fixed(plus) + 72, 0x9000, 4);
        let start = RelativeVirtualAddress::new(0x9000);
        let length = if plus { 40 } else { 24 };
        assert_eq!(
            read(&bytes, 0),
            Err(PeTlsCallbackError::Directory(
                PeTlsDirectoryError::DirectoryRange {
                    start,
                    length,
                    cause: PeRvaError::UnmappedRva { start, length },
                }
            ))
        );
    }
}

#[test]
fn maximum_limit_reaches_the_extra_slot_without_narrow_arithmetic() {
    for plus in [false, true] {
        let bytes = image(plus, &vec![1; 65_536]);
        assert_eq!(
            read(&bytes, u16::MAX),
            Err(PeTlsCallbackError::CallbackLimitExceeded {
                index: 65_535,
                limit: u16::MAX
            })
        );
    }
}
