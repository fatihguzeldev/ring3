use ring3_core::{
    FileOffset, PeExportBatchError, PeExportBatchLimits, PeExportLookupError, PeExportNameError,
    PeExportSelection, PeExportTarget, PeImportExportError, PeImportLookupError, PeImportSymbol,
    RelativeVirtualAddress, lookup_pe_import_exports,
};
mod importing {
    pub(super) fn put32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    pub(super) fn file_offset(rva: u32) -> usize {
        usize::try_from(rva - 0x1000 + 512).unwrap()
    }

    pub(super) fn source(index: u16) -> u32 {
        0x2000 + u32::from(index) * 0x2400
    }

    pub(super) fn entry(bytes: &mut [u8], plus: bool, descriptor: u16, index: usize, raw: u64) {
        let width = if plus { 8 } else { 4 };
        let offset = file_offset(source(descriptor)) + index * width;
        bytes[offset..offset + width].copy_from_slice(&raw.to_le_bytes()[..width]);
    }

    pub(super) fn fixture(plus: bool, descriptors: u16) -> Vec<u8> {
        let mut bytes = vec![0; 65_536];
        bytes[..2].copy_from_slice(b"MZ");
        put32(&mut bytes, 60, 128);
        bytes[128..132].copy_from_slice(b"PE\0\0");
        bytes[134..136].copy_from_slice(&1_u16.to_le_bytes());
        let fixed = if plus { 112 } else { 96_u16 };
        bytes[148..150].copy_from_slice(&(fixed + 16).to_le_bytes());
        bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
        put32(&mut bytes, 212, 512);
        put32(&mut bytes, 152 + usize::from(fixed) - 4, 2);
        put32(&mut bytes, 152 + usize::from(fixed) + 8, 0x1000);
        put32(
            &mut bytes,
            152 + usize::from(fixed) + 12,
            (u32::from(descriptors) + 1) * 20,
        );
        let section = 152 + usize::from(fixed) + 16;
        for (offset, value) in [(8, 60_000), (12, 0x1000), (16, 60_000), (20, 512)] {
            put32(&mut bytes, section + offset, value);
        }
        for index in 0..descriptors {
            let record = 512 + usize::from(index) * 20;
            put32(&mut bytes, record, source(index));
            put32(&mut bytes, record + 4, 123);
            put32(&mut bytes, record + 12, 0x1800);
            put32(&mut bytes, record + 16, u32::MAX);
        }
        let dll = file_offset(0x1800);
        bytes[dll..dll + 8].copy_from_slice(b"Own.DLL\0");
        let hint = file_offset(0xe000);
        bytes[hint..hint + 2].copy_from_slice(&0xabcd_u16.to_le_bytes());
        bytes[hint + 2..hint + 11].copy_from_slice(b"Symbol\tA\0");
        bytes
    }
}
mod providing {
    pub(super) fn put32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    pub(super) fn file_offset(rva: u32) -> usize {
        usize::try_from(rva - 0x1000 + 512).unwrap()
    }

    pub(super) fn directory(bytes: &mut [u8], plus: bool, rva: u32, size: u32) {
        let slot = if plus { 264 } else { 248 };
        put32(bytes, slot, rva);
        put32(bytes, slot + 4, size);
    }

    pub(super) fn section(bytes: &mut [u8], plus: bool, index: usize, fields: [u32; 4]) {
        let table = if plus { 272 } else { 256 };
        for (offset, value) in [8, 12, 16, 20].into_iter().zip(fields) {
            put32(bytes, table + index * 40 + offset, value);
        }
    }

    pub(super) fn row(bytes: &mut [u8], index: u32, rva: u32, address: u16) {
        put32(bytes, file_offset(0x5000 + index * 4), rva);
        let offset = file_offset(0x9000 + index * 2);
        bytes[offset..offset + 2].copy_from_slice(&address.to_le_bytes());
    }

    pub(super) fn fixture(plus: bool, count: u32) -> Vec<u8> {
        let mut bytes = vec![0; 65_536];
        bytes[..2].copy_from_slice(b"MZ");
        put32(&mut bytes, 60, 128);
        bytes[128..132].copy_from_slice(b"PE\0\0");
        bytes[134..136].copy_from_slice(&2_u16.to_le_bytes());
        let fixed = if plus { 112_u16 } else { 96 };
        bytes[148..150].copy_from_slice(&(fixed + 8).to_le_bytes());
        bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
        put32(&mut bytes, 212, 512);
        put32(&mut bytes, 152 + usize::from(fixed) - 4, 1);
        directory(&mut bytes, plus, 0x1000, 0x1000);
        section(&mut bytes, plus, 0, [60_000, 0x1000, 60_000, 512]);
        put32(&mut bytes, 528, 7);
        put32(&mut bytes, 532, 3);
        put32(&mut bytes, 536, count);
        put32(&mut bytes, 540, 0x3000);
        put32(&mut bytes, 544, 0x5000);
        put32(&mut bytes, 548, 0x9000);
        put32(&mut bytes, file_offset(0x3004), 0x40_0000);
        put32(&mut bytes, file_offset(0x3008), 0x1100);
        bytes[768..772].copy_from_slice(b"W.F\0");
        let first = file_offset(0xc000);
        bytes[first..first + 5].copy_from_slice(b"zeta\0");
        let second = file_offset(0xc010);
        bytes[second..second + 6].copy_from_slice(b"Alpha\0");
        for index in 0..count.min(4096) {
            row(&mut bytes, index, 0xc000, 0);
        }
        bytes
    }
}
fn limits(queries: u64, rows: u64) -> PeExportBatchLimits {
    PeExportBatchLimits {
        max_queries: queries,
        max_selection_rows: rows,
    }
}
fn mixed_importer(plus: bool) -> Vec<u8> {
    let mut bytes = importing::fixture(plus, 1);
    let name = importing::file_offset(0xe000) + 2;
    bytes[name..name + 5].copy_from_slice(b"zeta\0");
    let flag = if plus { 1_u64 << 63 } else { 1 << 31 };
    for (i, value) in [0xe000, flag | 7, flag | 8, flag | 9, flag | 65535]
        .into_iter()
        .enumerate()
    {
        importing::entry(&mut bytes, plus, 0, i, value);
    }
    bytes
}

#[test]
fn mixed_queries_preserve_import_metadata_and_explicit_provider_order() {
    for (import_plus, provider_plus) in [(false, false), (true, true), (false, true), (true, false)]
    {
        let importer = mixed_importer(import_plus);
        let provider = providing::fixture(provider_plus, 2);
        let batch = lookup_pe_import_exports(&importer, 0, &provider, limits(5, 5)).unwrap();
        assert_eq!(batch.imports.descriptor.dll_name, "Own.DLL");
        assert_eq!(batch.imports.descriptor.time_date_stamp, 123);
        assert_eq!(
            batch.imports.descriptor.import_address_table_rva,
            RelativeVirtualAddress::new(u32::MAX)
        );
        assert_eq!(batch.imports.entries.len(), 5);
        assert_eq!(batch.exports.selections.len(), 5);
        assert_eq!(batch.exports.selection_rows, 5);
        assert_eq!(
            batch.imports.entries[0].symbol,
            PeImportSymbol::ByName {
                hint_name_rva: RelativeVirtualAddress::new(0xe000),
                hint: 0xabcd,
                name: "zeta"
            }
        );
        let flag = if import_plus { 1_u64 << 63 } else { 1 << 31 };
        assert_eq!(batch.imports.entries[4].raw_value, flag | 65535);
        assert_eq!(
            batch.imports.entries[4].symbol,
            PeImportSymbol::Ordinal(65535)
        );
        let PeExportSelection::AmbiguousName { matches } =
            batch.exports.selections[0].as_ref().unwrap()
        else {
            panic!()
        };
        assert_eq!((matches[0].table_index, matches[1].table_index), (0, 1));
        for (i, ordinal) in [(1, 7), (2, 8), (3, 9)] {
            let PeExportSelection::Selected {
                address,
                name: None,
            } = batch.exports.selections[i].as_ref().unwrap()
            else {
                panic!()
            };
            assert_eq!(address.ordinal, ordinal);
        }
        assert_eq!(
            batch.exports.selections[4],
            Ok(PeExportSelection::OrdinalOutOfRange {
                ordinal: 65535,
                base: 7,
                address_count: 3
            })
        );
    }
}

#[test]
fn complete_import_errors_precede_descriptor_and_export_budget_checks() {
    let mut importer = mixed_importer(false);
    importing::put32(&mut importer, 512, 0);
    assert_eq!(
        lookup_pe_import_exports(&importer, 9, b"", limits(0, 0)),
        Err(PeImportExportError::Imports(
            PeImportLookupError::LookupTableUnavailable {
                descriptor_index: 0
            }
        ))
    );
    for plus in [false, true] {
        let mut importer = importing::fixture(plus, 2);
        importing::entry(&mut importer, plus, 0, 0, 0xe000);
        importing::put32(&mut importer, 532, 0);
        assert_eq!(
            lookup_pe_import_exports(&importer, 0, b"", limits(0, 0)),
            Err(PeImportExportError::Imports(
                PeImportLookupError::LookupTableUnavailable {
                    descriptor_index: 1
                }
            ))
        );
    }
}

#[test]
fn missing_and_empty_descriptors_do_not_inspect_provider_bytes() {
    let none = importing::fixture(false, 0);
    assert_eq!(
        lookup_pe_import_exports(&none, 0, b"", limits(0, 0)),
        Err(PeImportExportError::DescriptorNotFound {
            descriptor_index: 0,
            descriptor_count: 0
        })
    );
    let empty = importing::fixture(true, 2);
    assert_eq!(
        lookup_pe_import_exports(&empty, 2, b"", limits(0, 0)),
        Err(PeImportExportError::DescriptorNotFound {
            descriptor_index: 2,
            descriptor_count: 2
        })
    );
    let selected = lookup_pe_import_exports(&empty, 1, b"", limits(0, 0)).unwrap();
    assert_eq!(
        selected.imports.descriptor.descriptor_file_offset,
        FileOffset::new(532)
    );
    assert!(selected.imports.entries.is_empty());
    assert!(selected.exports.selections.is_empty());
    assert_eq!(selected.exports.selection_rows, 0);
}

#[test]
fn derived_query_count_precedes_provider_errors_and_rows_are_exact() {
    let importer = mixed_importer(false);
    assert_eq!(
        lookup_pe_import_exports(&importer, 0, b"", limits(4, 0)),
        Err(PeImportExportError::ExportBatch(
            PeExportBatchError::QueryCountExceeded { count: 5, limit: 4 }
        ))
    );
    let provider = providing::fixture(false, 2);
    assert_eq!(
        lookup_pe_import_exports(&importer, 0, &provider, limits(5, 4)),
        Err(PeImportExportError::ExportBatch(
            PeExportBatchError::SelectionRowsExceeded {
                index: 3,
                total: 5,
                limit: 4
            }
        ))
    );
}

#[test]
fn provider_name_entry_errors_stay_aligned_with_valid_ordinals() {
    let importer = mixed_importer(true);
    let mut provider = providing::fixture(true, 2);
    providing::row(&mut provider, 1, 0xc010, 3);
    let batch = lookup_pe_import_exports(&importer, 0, &provider, limits(5, 3)).unwrap();
    assert_eq!(batch.exports.selection_rows, 3);
    assert_eq!(
        batch.exports.selections[0],
        Err(PeExportLookupError::Names(
            PeExportNameError::AddressIndexOutOfRange {
                entry_index: 1,
                address_index: 3,
                address_count: 3
            }
        ))
    );
    let PeExportSelection::Selected { address, .. } = batch.exports.selections[3].as_ref().unwrap()
    else {
        panic!()
    };
    assert_eq!(
        address.target,
        PeExportTarget::Forwarder {
            rva: RelativeVirtualAddress::new(0x1100),
            text: "W.F"
        }
    );
}

#[test]
fn malformed_provider_errors_are_ordered_zero_row_results() {
    let importer = mixed_importer(false);
    let batch = lookup_pe_import_exports(&importer, 0, b"", limits(5, 0)).unwrap();
    assert_eq!(batch.exports.selection_rows, 0);
    assert_eq!(batch.exports.selections.len(), 5);
    assert!(matches!(
        batch.exports.selections[0],
        Err(PeExportLookupError::Names(_))
    ));
    assert!(
        batch.exports.selections[1..]
            .iter()
            .all(|x| matches!(x, Err(PeExportLookupError::Addresses(_))))
    );
}

#[test]
fn both_inputs_keep_their_own_borrowed_text_across_repeated_batches() {
    let importer = mixed_importer(true);
    let provider = providing::fixture(false, 2);
    let before_importer = importer.clone();
    let before_provider = provider.clone();
    for _ in 0..8 {
        let batch = lookup_pe_import_exports(&importer, 0, &provider, limits(5, 5)).unwrap();
        let PeImportSymbol::ByName { name, .. } = batch.imports.entries[0].symbol else {
            panic!()
        };
        assert!(std::ptr::eq(
            name.as_ptr(),
            importer[importing::file_offset(0xe000) + 2..].as_ptr()
        ));
        assert!(std::ptr::eq(
            batch.imports.descriptor.dll_name.as_ptr(),
            importer[importing::file_offset(0x1800)..].as_ptr()
        ));
        let PeExportSelection::AmbiguousName { matches } =
            batch.exports.selections[0].as_ref().unwrap()
        else {
            panic!()
        };
        assert!(matches.iter().all(|x| std::ptr::eq(
            x.name.as_ptr(),
            provider[providing::file_offset(0xc000)..].as_ptr()
        )));
        let PeExportSelection::Selected { address, .. } =
            batch.exports.selections[3].as_ref().unwrap()
        else {
            panic!()
        };
        let PeExportTarget::Forwarder { text, .. } = address.target else {
            panic!()
        };
        assert!(std::ptr::eq(text.as_ptr(), provider[768..].as_ptr()));
    }
    assert_eq!(importer, before_importer);
    assert_eq!(provider, before_provider);
}

fn read_corpus_image(variable: &str, kind: ring3_core::PeKind) -> (std::path::PathBuf, Vec<u8>) {
    let path = std::path::PathBuf::from(
        std::env::var_os(variable).expect("explicit generated image path"),
    );
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes.len(), 2048);
    assert_eq!(
        ring3_core::parse_pe_header_prefix(&bytes).unwrap().kind,
        kind
    );
    (path, bytes)
}

fn corpus_selection(ordinal: bool) -> PeExportSelection<'static> {
    use ring3_core::{PeExportAddressEntry, PeExportName};
    PeExportSelection::Selected {
        address: PeExportAddressEntry {
            table_index: 0,
            ordinal: if ordinal { 32768 } else { 1 },
            entry_rva: RelativeVirtualAddress::new(if ordinal { 8249 } else { 8247 }),
            entry_file_offset: FileOffset::new(if ordinal { 1593 } else { 1591 }),
            target: PeExportTarget::Rva(RelativeVirtualAddress::new(4096)),
        },
        name: if ordinal {
            None
        } else {
            Some(PeExportName {
                table_index: 0,
                name_pointer_rva: RelativeVirtualAddress::new(8251),
                name_pointer_file_offset: FileOffset::new(1595),
                ordinal_entry_rva: RelativeVirtualAddress::new(8255),
                ordinal_entry_file_offset: FileOffset::new(1599),
                address_index: 0,
                name_rva: RelativeVirtualAddress::new(8257),
                name_file_offset: FileOffset::new(1601),
                name: "ring3_probe",
            })
        },
    }
}

fn check_corpus_pair(
    importer_variable: &str,
    provider_variable: &str,
    kind: ring3_core::PeKind,
    ordinal: bool,
) {
    use ring3_core::{PeImportDescriptor, PeImportLookup, PeImportLookupEntry};
    let (importer_path, importer) = read_corpus_image(importer_variable, kind);
    let (provider_path, provider) = read_corpus_image(provider_variable, kind);
    let importer_before = importer.clone();
    let provider_before = provider.clone();
    let plus = kind == ring3_core::PeKind::Pe32Plus;
    let hint_rva = if plus { 8264 } else { 8248 };
    let dll_rva = if ordinal { hint_rva } else { hint_rva + 14 };
    let symbol = if ordinal {
        PeImportSymbol::Ordinal(32768)
    } else {
        PeImportSymbol::ByName {
            hint_name_rva: RelativeVirtualAddress::new(hint_rva),
            hint: 0,
            name: "ring3_probe",
        }
    };
    let imports = PeImportLookup {
        descriptor: PeImportDescriptor {
            descriptor_rva: RelativeVirtualAddress::new(8192),
            descriptor_file_offset: FileOffset::new(1536),
            import_lookup_table_rva: RelativeVirtualAddress::new(8232),
            time_date_stamp: 0,
            forwarder_chain: 0,
            name_rva: RelativeVirtualAddress::new(dll_rva),
            import_address_table_rva: RelativeVirtualAddress::new(if plus { 8248 } else { 8240 }),
            dll_name: if ordinal {
                "Ring3Ordinal.dll"
            } else {
                "Ring3Probe.dll"
            },
        },
        entries: vec![PeImportLookupEntry {
            lookup_rva: RelativeVirtualAddress::new(8232),
            lookup_file_offset: FileOffset::new(1576),
            raw_value: if ordinal {
                if plus {
                    0x8000_0000_0000_8000
                } else {
                    0x8000_8000
                }
            } else {
                u64::from(hint_rva)
            },
            symbol,
        }],
    };
    let selection = corpus_selection(ordinal);
    for _ in 0..8 {
        assert_eq!(
            lookup_pe_import_exports(&importer, 0, &provider, limits(0, 1)),
            Err(PeImportExportError::ExportBatch(
                PeExportBatchError::QueryCountExceeded { count: 1, limit: 0 }
            ))
        );
        assert_eq!(
            lookup_pe_import_exports(&importer, 0, &provider, limits(1, 0)),
            Err(PeImportExportError::ExportBatch(
                PeExportBatchError::SelectionRowsExceeded {
                    index: 0,
                    total: 1,
                    limit: 0
                }
            ))
        );
        let batch = lookup_pe_import_exports(&importer, 0, &provider, limits(1, 1)).unwrap();
        assert_eq!(batch.imports, imports);
        assert_eq!(batch.exports.selection_rows, 1);
        assert_eq!(batch.exports.selections, vec![Ok(selection.clone())]);
        assert!(std::ptr::eq(
            batch.imports.descriptor.dll_name.as_ptr(),
            importer[(dll_rva - 6656) as usize..].as_ptr()
        ));
        if let PeImportSymbol::ByName { name, .. } = batch.imports.entries[0].symbol {
            assert!(std::ptr::eq(
                name.as_ptr(),
                importer[(hint_rva - 6656 + 2) as usize..].as_ptr()
            ));
        }
        if let PeExportSelection::Selected {
            name: Some(name), ..
        } = batch.exports.selections[0].as_ref().unwrap()
        {
            assert!(std::ptr::eq(name.name.as_ptr(), provider[1601..].as_ptr()));
        }
    }
    assert_eq!(importer, importer_before);
    assert_eq!(provider, provider_before);
    assert_eq!(std::fs::read(importer_path).unwrap(), importer_before);
    assert_eq!(std::fs::read(provider_path).unwrap(), provider_before);
}

#[test]
#[ignore = "requires explicit RING3_IMPORT_PE32_FIXTURE and RING3_EXPORT_PE32_NAMED_DLL"]
fn generated_pe32_named_imports_match_explicit_exports() {
    check_corpus_pair(
        "RING3_IMPORT_PE32_FIXTURE",
        "RING3_EXPORT_PE32_NAMED_DLL",
        ring3_core::PeKind::Pe32,
        false,
    );
}

#[test]
#[ignore = "requires explicit RING3_ORDINAL_PE32_FIXTURE and RING3_EXPORT_PE32_ORDINAL_DLL"]
fn generated_pe32_ordinal_imports_match_explicit_exports() {
    check_corpus_pair(
        "RING3_ORDINAL_PE32_FIXTURE",
        "RING3_EXPORT_PE32_ORDINAL_DLL",
        ring3_core::PeKind::Pe32,
        true,
    );
}

#[test]
#[ignore = "requires explicit RING3_IMPORT_PE32PLUS_FIXTURE and RING3_EXPORT_PE32PLUS_NAMED_DLL"]
fn generated_pe32plus_named_imports_match_explicit_exports() {
    check_corpus_pair(
        "RING3_IMPORT_PE32PLUS_FIXTURE",
        "RING3_EXPORT_PE32PLUS_NAMED_DLL",
        ring3_core::PeKind::Pe32Plus,
        false,
    );
}

#[test]
#[ignore = "requires explicit RING3_ORDINAL_PE32PLUS_FIXTURE and RING3_EXPORT_PE32PLUS_ORDINAL_DLL"]
fn generated_pe32plus_ordinal_imports_match_explicit_exports() {
    check_corpus_pair(
        "RING3_ORDINAL_PE32PLUS_FIXTURE",
        "RING3_EXPORT_PE32PLUS_ORDINAL_DLL",
        ring3_core::PeKind::Pe32Plus,
        true,
    );
}
