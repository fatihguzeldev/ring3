use std::fmt::Write as _;

use ring3_core::{
    FileOffset, PeKind, PeTlsCallbackEntry, PeTlsCallbackError as Error,
    PeTlsCallbackLimits as Limits, PeTlsCallbackTable, PeTlsCallbacks, PeTlsDirectory,
    RelativeVirtualAddress as Rva, parse_pe_headers, parse_pe_tls_callbacks,
    parse_pe_tls_directory, resolve_pe_file_range,
};

use super::{Digest, directory_base, fixture, put32};

#[derive(Debug)]
pub(super) struct Report {
    cases: u32,
    states: [u32; 4],
    errors: [u32; 5],
    pub(super) digest: Digest,
}

impl Report {
    pub(super) fn new() -> Self {
        Self {
            cases: 0,
            states: [0; 4],
            errors: [0; 5],
            digest: Digest(0xcbf2_9ce4_8422_2325),
        }
    }
}

fn width(kind: PeKind) -> u32 {
    if kind == PeKind::Pe32 { 4 } else { 8 }
}

fn raw_word(bytes: &[u8]) -> u64 {
    let mut raw = [0; 8];
    raw[..bytes.len()].copy_from_slice(bytes);
    u64::from_le_bytes(raw)
}

fn successful(bytes: &[u8], value: &PeTlsCallbacks, cap: u16) -> usize {
    assert_eq!(parse_pe_tls_directory(bytes), Ok(Some(value.directory)));
    assert_eq!(
        value.image_base,
        parse_pe_headers(bytes).unwrap().optional.image_base
    );
    let Some(table) = &value.table else {
        assert_eq!(value.directory.address_of_callbacks, 0);
        return 1;
    };
    assert_ne!(value.directory.address_of_callbacks, 0);
    assert_eq!(
        value
            .image_base
            .checked_add(u64::from(table.table_rva.get())),
        Some(value.directory.address_of_callbacks)
    );
    assert!(table.entries.len() <= usize::from(cap));
    let width = width(value.directory.kind);
    let length = (u32::try_from(table.entries.len()).unwrap() + 1) * width;
    let mapped = resolve_pe_file_range(bytes, table.table_rva, length).unwrap();
    assert_eq!(mapped.file_offset, table.table_file_offset);
    let words: Vec<_> = mapped
        .bytes
        .chunks_exact(usize::try_from(width).unwrap())
        .map(raw_word)
        .collect();
    assert_eq!(words.last(), Some(&0));
    for (index, entry) in table.entries.iter().enumerate() {
        let index = u32::try_from(index).unwrap();
        assert_eq!(entry.table_index, index);
        assert_eq!(
            entry.slot_rva.get(),
            table.table_rva.get().checked_add(index * width).unwrap()
        );
        assert_eq!(
            entry.slot_file_offset.get(),
            table.table_file_offset.get() + u64::from(index * width)
        );
        assert_eq!(entry.raw_va, words[usize::try_from(index).unwrap()]);
        assert_ne!(entry.raw_va, 0);
    }
    let offset = length - width;
    assert_eq!(
        table.terminator_rva.get(),
        table.table_rva.get().checked_add(offset).unwrap()
    );
    assert_eq!(
        table.terminator_file_offset.get(),
        table.table_file_offset.get() + u64::from(offset)
    );
    if table.entries.is_empty() { 2 } else { 3 }
}

fn failure(bytes: &[u8], error: Error, cap: u16) -> usize {
    if let Error::Directory(cause) = error {
        assert_eq!(parse_pe_tls_directory(bytes), Err(cause));
        return 0;
    }
    let directory = parse_pe_tls_directory(bytes).unwrap().unwrap();
    let base = parse_pe_headers(bytes).unwrap().optional.image_base;
    match error {
        Error::CallbackAddressBelowImageBase {
            address,
            image_base,
        } => {
            assert_eq!(
                (address, image_base),
                (directory.address_of_callbacks, base)
            );
            assert_ne!(address, 0);
            assert!(address < image_base);
            1
        }
        Error::CallbackRvaOverflow {
            address,
            image_base,
        } => {
            assert_eq!(
                (address, image_base),
                (directory.address_of_callbacks, base)
            );
            assert!(address.checked_sub(image_base).unwrap() > u64::from(u32::MAX));
            2
        }
        Error::TableRange {
            table_index,
            table_rva,
            length,
            cause,
        } => {
            assert_ne!(directory.address_of_callbacks, 0);
            assert_eq!(
                base.checked_add(u64::from(table_rva.get())),
                Some(directory.address_of_callbacks)
            );
            assert!(table_index <= u32::from(cap));
            assert_eq!(length, (table_index + 1) * width(directory.kind));
            assert_eq!(resolve_pe_file_range(bytes, table_rva, length), Err(cause));
            3
        }
        Error::CallbackLimitExceeded { index, limit } => {
            assert_eq!(limit, cap);
            assert_eq!(index, u32::from(cap));
            let start = Rva::new(
                u32::try_from(directory.address_of_callbacks.checked_sub(base).unwrap()).unwrap(),
            );
            let width = width(directory.kind);
            let mapped = resolve_pe_file_range(bytes, start, (index + 1) * width).unwrap();
            assert!(
                mapped
                    .bytes
                    .chunks_exact(usize::try_from(width).unwrap())
                    .all(|word| raw_word(word) != 0)
            );
            4
        }
        Error::Directory(_) => unreachable!(),
    }
}

pub(super) fn inspect(
    bytes: &[u8],
    cap: u16,
    report: &mut Report,
) -> Result<Option<PeTlsCallbacks>, Error> {
    let before = bytes.to_vec();
    let mut owned = bytes.to_vec();
    let first = parse_pe_tls_callbacks(&owned, Limits { max_callbacks: cap });
    assert_eq!(owned, before);
    owned.fill(0);
    drop(owned);
    assert_eq!(
        first,
        parse_pe_tls_callbacks(bytes, Limits { max_callbacks: cap })
    );
    match &first {
        Ok(None) => {
            assert_eq!(parse_pe_tls_directory(bytes), Ok(None));
            report.states[0] += 1;
        }
        Ok(Some(value)) => report.states[successful(bytes, value, cap)] += 1,
        Err(error) => report.errors[failure(bytes, *error, cap)] += 1,
    }
    assert_eq!(bytes, before);
    write!(
        report.digest,
        "input:{}:{}:{cap}\0",
        report.cases,
        bytes.len()
    )
    .unwrap();
    report.digest.bytes(bytes);
    write!(report.digest, "\0{first:?}\0").unwrap();
    report.cases += 1;
    first
}

fn tail(plus: bool, mode: u8) -> (Vec<u8>, u16, Result<Option<PeTlsCallbacks>, Error>) {
    let mut bytes = fixture(plus);
    let base: u64 = if plus { 5_368_709_120 } else { 4_194_304 };
    let width: u32 = if plus { 8 } else { 4 };
    let raw = if plus {
        0xffff_ffff_ffff_ff01_u64
    } else {
        0xffff_fff1
    };
    let size = if plus { 40 } else { 24 };
    let (base_at, callback_at) = if plus { (176, 808) } else { (180, 796) };
    bytes[base_at..base_at + usize::try_from(width).unwrap()]
        .copy_from_slice(&base.to_le_bytes()[..usize::try_from(width).unwrap()]);
    bytes[784..784 + size].fill(0);
    bytes[832..856].fill(0);
    if mode == 0 {
        bytes[directory_base(plus) + 72..directory_base(plus) + 80].fill(0);
        return (bytes, 0, Ok(None));
    }
    let address = if mode == 1 { 0 } else { base + 4416 };
    bytes[callback_at..callback_at + usize::try_from(width).unwrap()]
        .copy_from_slice(&address.to_le_bytes()[..usize::try_from(width).unwrap()]);
    let entries = if mode >= 3 { 2 } else { 0 };
    for index in 0..entries {
        let at = 832 + usize::try_from(index * width).unwrap();
        if plus {
            bytes[at..at + 8].copy_from_slice(&raw.to_le_bytes());
        } else {
            put32(&mut bytes, at, u32::try_from(raw).unwrap());
        }
    }
    if mode == 4 {
        return (
            bytes,
            1,
            Err(Error::CallbackLimitExceeded { index: 1, limit: 1 }),
        );
    }
    let expected = PeTlsCallbacks {
        directory: PeTlsDirectory {
            kind: if plus { PeKind::Pe32Plus } else { PeKind::Pe32 },
            directory_rva: Rva::new(4368),
            directory_file_offset: FileOffset::new(784),
            directory_size: u32::try_from(size).unwrap(),
            start_address_of_raw_data: 0,
            end_address_of_raw_data: 0,
            address_of_index: 0,
            address_of_callbacks: address,
            size_of_zero_fill: 0,
            characteristics: 0,
        },
        image_base: base,
        table: (mode != 1).then(|| PeTlsCallbackTable {
            table_rva: Rva::new(4416),
            table_file_offset: FileOffset::new(832),
            entries: (0..entries)
                .map(|index| PeTlsCallbackEntry {
                    table_index: index,
                    slot_rva: Rva::new(4416 + index * width),
                    slot_file_offset: FileOffset::new(832 + u64::from(index * width)),
                    raw_va: raw,
                })
                .collect(),
            terminator_rva: Rva::new(4416 + entries * width),
            terminator_file_offset: FileOffset::new(832 + u64::from(entries * width)),
        }),
    };
    (bytes, u16::try_from(entries).unwrap(), Ok(Some(expected)))
}

pub(super) fn positive_tail(report: &mut Report) {
    assert_eq!(report.cases, super::CASE_COUNT);
    for plus in [false, true] {
        for mode in 0..5 {
            let (bytes, cap, expected) = tail(plus, mode);
            assert_eq!(inspect(&bytes, cap, report), expected);
        }
    }
    assert_eq!(report.cases, super::CASE_COUNT + 10);
    assert!(report.states.iter().all(|&count| count > 0));
    assert!(report.errors[0] > 0 && report.errors[3] > 0 && report.errors[4] > 0);
    assert_eq!(
        report.states.iter().sum::<u32>() + report.errors.iter().sum::<u32>(),
        report.cases
    );
}
