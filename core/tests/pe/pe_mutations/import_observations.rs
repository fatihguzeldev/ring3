use std::fmt::Write as _;

use ring3_core::{
    FileOffset, PeImportDescriptor, PeImportLookupEntry, PeImportLookupError,
    PeImportLookupObservationError as Error, PeImportLookupSource as Source, PeImportSymbol,
    PeKind, PeObservedImportLookup, RelativeVirtualAddress as Rva, parse_pe_headers,
    parse_pe_import_descriptors, parse_pe_import_lookups,
    parse_pe_import_lookups_with_iat_fallback, resolve_pe_file_range,
};

use super::{Digest, fixture, put32};

#[derive(Debug)]
pub(super) struct Report {
    cases: u32,
    outcomes: [u32; 2],
    sources: [u32; 2],
    lookup_errors: [u32; 2],
    pub(super) digest: Digest,
}

impl Report {
    pub(super) fn new() -> Self {
        Self {
            cases: 0,
            outcomes: [0; 2],
            sources: [0; 2],
            lookup_errors: [0; 2],
            digest: Digest(0xcbf2_9ce4_8422_2325),
        }
    }
}

fn selection(descriptor: &PeImportDescriptor<'_>) -> (Source, Rva) {
    if descriptor.import_lookup_table_rva.get() != 0 {
        (
            Source::OriginalFirstThunk,
            descriptor.import_lookup_table_rva,
        )
    } else {
        assert_ne!(descriptor.import_address_table_rva.get(), 0);
        (
            Source::FirstThunkFallback,
            descriptor.import_address_table_rva,
        )
    }
}

fn source_index(source: Source) -> usize {
    usize::from(source == Source::FirstThunkFallback)
}

fn borrowed_name(bytes: &[u8], start: Rva, prefix: usize, name: &str) {
    assert!(!name.is_empty() && name.is_ascii() && name.len() < 1024);
    let length = u32::try_from(prefix + name.len() + 1).unwrap();
    let mapped = resolve_pe_file_range(bytes, start, length).unwrap();
    assert_eq!(
        &mapped.bytes[prefix..mapped.bytes.len() - 1],
        name.as_bytes()
    );
    assert_eq!(mapped.bytes.last(), Some(&0));
    assert_eq!(name.as_ptr(), mapped.bytes[prefix..].as_ptr());
}

fn successful(bytes: &[u8], tables: &[PeObservedImportLookup<'_>], report: &mut Report) {
    let width = match parse_pe_headers(bytes).unwrap().prefix.kind {
        PeKind::Pe32 => 4_u32,
        PeKind::Pe32Plus => 8,
    };
    let flag = 1_u64 << (width * 8 - 1);
    let mut total_entries = 0;
    let mut total_name_bytes = 0;
    assert!(tables.len() <= 128);
    for table in tables {
        let d = &table.descriptor;
        let mapped = resolve_pe_file_range(bytes, d.descriptor_rva, 20).unwrap();
        assert_eq!(mapped.file_offset, d.descriptor_file_offset);
        let words: Vec<_> = mapped
            .bytes
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
            .collect();
        assert_eq!(
            words,
            [
                d.import_lookup_table_rva.get(),
                d.time_date_stamp,
                d.forwarder_chain,
                d.name_rva.get(),
                d.import_address_table_rva.get()
            ]
        );
        borrowed_name(bytes, d.name_rva, 0, d.dll_name);
        let (source, start) = selection(d);
        assert_eq!(table.source, source);
        report.sources[source_index(source)] += 1;
        assert!(table.entries.len() <= 1024);
        total_entries += table.entries.len();
        for (index, entry) in table.entries.iter().enumerate() {
            let offset = u32::try_from(index).unwrap() * width;
            let mapped = resolve_pe_file_range(bytes, start, offset + width).unwrap();
            assert_eq!(
                entry.lookup_rva.get(),
                start.get().checked_add(offset).unwrap()
            );
            assert_eq!(
                entry.lookup_file_offset.get(),
                mapped.file_offset.get() + u64::from(offset)
            );
            let mut raw = [0; 8];
            raw[..usize::try_from(width).unwrap()]
                .copy_from_slice(&mapped.bytes[usize::try_from(offset).unwrap()..]);
            assert_eq!(entry.raw_value, u64::from_le_bytes(raw));
            assert_ne!(entry.raw_value, 0);
            match entry.symbol {
                PeImportSymbol::Ordinal(ordinal) => {
                    assert_eq!(entry.raw_value, flag | u64::from(ordinal));
                }
                PeImportSymbol::ByName {
                    hint_name_rva,
                    hint,
                    name,
                } => {
                    assert_eq!(entry.raw_value, u64::from(hint_name_rva.get()));
                    assert_eq!(entry.raw_value & flag, 0);
                    let mapped = resolve_pe_file_range(bytes, hint_name_rva, 2).unwrap();
                    assert_eq!(hint, u16::from_le_bytes(mapped.bytes.try_into().unwrap()));
                    borrowed_name(bytes, hint_name_rva, 2, name);
                    total_name_bytes += name.len() + 1;
                }
            }
        }
        let length = (u32::try_from(table.entries.len()).unwrap() + 1) * width;
        let mapped = resolve_pe_file_range(bytes, start, length).unwrap();
        assert!(
            mapped.bytes[mapped.bytes.len() - usize::try_from(width).unwrap()..]
                .iter()
                .all(|&byte| byte == 0)
        );
    }
    assert!(total_entries <= 4096 && total_name_bytes <= 65_536);
}

fn cause_index(cause: PeImportLookupError) -> u16 {
    match cause {
        PeImportLookupError::LookupRange {
            descriptor_index, ..
        }
        | PeImportLookupError::EntryLimitExceeded {
            descriptor_index, ..
        }
        | PeImportLookupError::TotalEntryLimitExceeded {
            descriptor_index, ..
        }
        | PeImportLookupError::InvalidOrdinalEncoding {
            descriptor_index, ..
        }
        | PeImportLookupError::InvalidNameEncoding {
            descriptor_index, ..
        }
        | PeImportLookupError::HintNameRange {
            descriptor_index, ..
        }
        | PeImportLookupError::EmptySymbolName {
            descriptor_index, ..
        }
        | PeImportLookupError::NonAsciiSymbolName {
            descriptor_index, ..
        }
        | PeImportLookupError::NameLengthLimitExceeded {
            descriptor_index, ..
        }
        | PeImportLookupError::NameScanBudgetExceeded {
            descriptor_index, ..
        } => descriptor_index,
        PeImportLookupError::Descriptors(_)
        | PeImportLookupError::LookupTableUnavailable { .. } => {
            panic!("unexpected nested cause: {cause:?}")
        }
    }
}

pub(super) fn inspect<'a>(
    bytes: &'a [u8],
    report: &mut Report,
) -> Result<Vec<PeObservedImportLookup<'a>>, Error> {
    let before = bytes.to_vec();
    let first = parse_pe_import_lookups_with_iat_fallback(bytes);
    assert_eq!(
        first,
        parse_pe_import_lookups_with_iat_fallback(bytes),
        "import observations at case {}",
        report.cases
    );
    match &first {
        Ok(tables) => successful(bytes, tables, report),
        Err(Error::Descriptors(cause)) => {
            assert_eq!(parse_pe_import_descriptors(bytes), Err(*cause));
        }
        Err(Error::LookupTableUnavailable { descriptor_index }) => {
            let descriptors = parse_pe_import_descriptors(bytes).unwrap();
            let d = &descriptors[usize::from(*descriptor_index)];
            assert_eq!(
                (
                    d.import_lookup_table_rva.get(),
                    d.import_address_table_rva.get()
                ),
                (0, 0)
            );
        }
        Err(Error::Lookup {
            descriptor_index,
            source,
            start,
            cause,
        }) => {
            let descriptors = parse_pe_import_descriptors(bytes).unwrap();
            assert_eq!(
                selection(&descriptors[usize::from(*descriptor_index)]),
                (*source, *start)
            );
            assert_eq!(*descriptor_index, cause_index(*cause));
            report.lookup_errors[source_index(*source)] += 1;
        }
    }
    assert_eq!(
        bytes, before,
        "input changed at observation {}",
        report.cases
    );
    write!(report.digest, "input:{}:{}\0", report.cases, bytes.len()).unwrap();
    report.digest.bytes(bytes);
    write!(report.digest, "\0{first:?}\0").unwrap();
    report.outcomes[usize::from(first.is_err())] += 1;
    report.cases += 1;
    first
}

pub(super) fn positive_tail(report: &mut Report) {
    assert_eq!(report.cases, super::CASE_COUNT);
    for plus in [false, true] {
        for named in [true, false] {
            let mut bytes = fixture(plus);
            put32(&mut bytes, 560, 0);
            put32(&mut bytes, 576, 4200);
            bytes[616..640].fill(0);
            let raw_value: u64 = if named {
                4224
            } else if plus {
                0x8000_0000_0000_8000
            } else {
                0x8000_8000
            };
            if plus {
                bytes[616..624].copy_from_slice(&raw_value.to_le_bytes());
            } else {
                put32(&mut bytes, 616, u32::try_from(raw_value).unwrap());
            }
            let symbol = if named {
                bytes[640..642].copy_from_slice(&0xabcd_u16.to_le_bytes());
                bytes[642..651].copy_from_slice(b"iat\tname\0");
                PeImportSymbol::ByName {
                    hint_name_rva: Rva::new(4224),
                    hint: 0xabcd,
                    name: "iat\tname",
                }
            } else {
                PeImportSymbol::Ordinal(32768)
            };
            let before = bytes.clone();
            assert_eq!(
                inspect(&bytes, report),
                Ok(vec![PeObservedImportLookup {
                    descriptor: PeImportDescriptor {
                        descriptor_rva: Rva::new(4144),
                        descriptor_file_offset: FileOffset::new(560),
                        import_lookup_table_rva: Rva::new(0),
                        time_date_stamp: 0,
                        forwarder_chain: 0,
                        name_rva: Rva::new(4184),
                        import_address_table_rva: Rva::new(4200),
                        dll_name: "x.dll",
                    },
                    source: Source::FirstThunkFallback,
                    entries: vec![PeImportLookupEntry {
                        lookup_rva: Rva::new(4200),
                        lookup_file_offset: FileOffset::new(616),
                        raw_value,
                        symbol
                    }],
                }])
            );
            assert_eq!(
                parse_pe_import_lookups(&bytes),
                Err(PeImportLookupError::LookupTableUnavailable {
                    descriptor_index: 0
                })
            );
            assert_eq!(bytes, before);
        }
    }
    assert_eq!(report.cases, super::CASE_COUNT + 4);
    assert_eq!(report.outcomes.iter().sum::<u32>(), report.cases);
    assert!(report.outcomes.iter().all(|&count| count > 0));
    assert!(report.sources.iter().all(|&count| count > 0));
    assert!(report.lookup_errors.iter().all(|&count| count > 0));
}
