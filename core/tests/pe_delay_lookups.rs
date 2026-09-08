use ring3_core::{
    FileOffset, PeDelayImportDescriptor, PeDelayImportError, PeDelayImportLookup,
    PeDelayImportLookupError, PeDelayImportLookupTable, PeDelayImportName, PeDelayImportNameError,
    PeImportLookupEntry, PeImportLookupError, PeImportSymbol, PeKind, RelativeVirtualAddress,
    parse_pe_delay_import_lookups,
};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixed(plus: bool) -> usize {
    if plus { 112 } else { 96 }
}

fn directory(bytes: &mut [u8], plus: bool, rva: u32, size: u32) {
    put32(bytes, 152 + fixed(plus) + 104, rva);
    put32(bytes, 152 + fixed(plus) + 108, size);
}

fn descriptor(bytes: &mut [u8], index: usize, name: u32, lookup: u32) {
    for (i, value) in [1, name, u32::MAX, 0x10a0, lookup, 2, 3, 4]
        .into_iter()
        .enumerate()
    {
        put32(bytes, 512 + index * 32 + i * 4, value);
    }
}

fn entry(bytes: &mut [u8], plus: bool, index: usize, value: u64) {
    let width = if plus { 8 } else { 4 };
    bytes[672 + index * width..672 + (index + 1) * width]
        .copy_from_slice(&value.to_le_bytes()[..width]);
}

fn flag(plus: bool) -> u64 {
    1 << if plus { 63 } else { 31 }
}

fn fixture(plus: bool) -> Vec<u8> {
    let mut bytes = vec![0; 1024];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&1_u16.to_le_bytes());
    let size = u16::try_from(fixed(plus) + 112).unwrap();
    bytes[148..150].copy_from_slice(&size.to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + fixed(plus) - 4, 14);
    directory(&mut bytes, plus, 0x1000, 64);
    let section = 152 + fixed(plus) + 112;
    for (offset, value) in [8, 12, 16, 20].into_iter().zip([512, 0x1000, 512, 512]) {
        put32(&mut bytes, section + offset, value);
    }
    descriptor(&mut bytes, 0, 0x1080, 0x10a0);
    bytes[640..655].copy_from_slice(b"Ring3Delay.dll\0");
    bytes[768..770].copy_from_slice(&0xbeef_u16.to_le_bytes());
    bytes[770..776].copy_from_slice(b"PrObe\0");
    entry(&mut bytes, plus, 0, 0x1100);
    entry(&mut bytes, plus, 1, flag(plus) | 0x8000);
    bytes
}

fn expected(plus: bool) -> PeDelayImportLookupTable<'static> {
    let width = if plus { 8 } else { 4 };
    PeDelayImportLookupTable {
        kind: if plus { PeKind::Pe32Plus } else { PeKind::Pe32 },
        directory_rva: RelativeVirtualAddress::new(0x1000),
        directory_file_offset: FileOffset::new(512),
        directory_size: 64,
        imports: vec![PeDelayImportLookup {
            import: PeDelayImportName {
                descriptor: PeDelayImportDescriptor {
                    descriptor_rva: RelativeVirtualAddress::new(0x1000),
                    descriptor_file_offset: FileOffset::new(512),
                    attributes: 1,
                    dll_name_address: 0x1080,
                    module_handle_address: u32::MAX,
                    import_address_table_address: 0x10a0,
                    import_name_table_address: 0x10a0,
                    bound_import_address_table_address: 2,
                    unload_import_address_table_address: 3,
                    time_date_stamp: 4,
                },
                dll_name: "Ring3Delay.dll",
            },
            entries: vec![
                PeImportLookupEntry {
                    lookup_rva: RelativeVirtualAddress::new(0x10a0),
                    lookup_file_offset: FileOffset::new(672),
                    raw_value: 0x1100,
                    symbol: PeImportSymbol::ByName {
                        hint_name_rva: RelativeVirtualAddress::new(0x1100),
                        hint: 0xbeef,
                        name: "PrObe",
                    },
                },
                PeImportLookupEntry {
                    lookup_rva: RelativeVirtualAddress::new(0x10a0 + width),
                    lookup_file_offset: FileOffset::new(672 + u64::from(width)),
                    raw_value: flag(plus) | 0x8000,
                    symbol: PeImportSymbol::Ordinal(32768),
                },
            ],
        }],
        terminator_rva: RelativeVirtualAddress::new(0x1020),
        terminator_file_offset: FileOffset::new(544),
    }
}

#[test]
fn both_widths_preserve_full_metadata_borrows_order_and_input() {
    for plus in [false, true] {
        let bytes = fixture(plus);
        let before = bytes.clone();
        let table = parse_pe_delay_import_lookups(&bytes).unwrap().unwrap();
        assert_eq!(table, expected(plus));
        assert_eq!(
            table.imports[0].import.dll_name.as_ptr(),
            bytes[640..].as_ptr()
        );
        let PeImportSymbol::ByName { name, .. } = table.imports[0].entries[0].symbol else {
            panic!("expected a borrowed name");
        };
        assert_eq!(name.as_ptr(), bytes[770..].as_ptr());
        assert_eq!(bytes, before);
    }
}

#[test]
fn absent_directory_empty_directory_and_empty_int_are_distinct() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        entry(&mut bytes, plus, 0, 0);
        let mut wanted = expected(plus);
        wanted.imports[0].entries.clear();
        assert_eq!(parse_pe_delay_import_lookups(&bytes), Ok(Some(wanted)));
        bytes[512..544].fill(0);
        assert_eq!(
            parse_pe_delay_import_lookups(&bytes),
            Ok(Some(PeDelayImportLookupTable {
                imports: Vec::new(),
                terminator_rva: RelativeVirtualAddress::new(0x1000),
                terminator_file_offset: FileOffset::new(512),
                ..expected(plus)
            }))
        );
        directory(&mut bytes, plus, 0, 0);
        assert_eq!(parse_pe_delay_import_lookups(&bytes), Ok(None));
    }
}

#[test]
fn whole_raw_table_then_all_names_precede_any_lookup() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, 0x1000, 64);
        descriptor(&mut bytes, 0, 0x1080, 0);
        descriptor(&mut bytes, 1, 0x1080, 0x10a0);
        put32(&mut bytes, 544, 2);
        assert!(matches!(
            parse_pe_delay_import_lookups(&bytes),
            Err(PeDelayImportLookupError::Names(
                PeDelayImportNameError::Table(PeDelayImportError::MissingTerminator { .. })
            ))
        ));
        directory(&mut bytes, plus, 0x1000, 96);
        assert_eq!(
            parse_pe_delay_import_lookups(&bytes),
            Err(PeDelayImportLookupError::Names(
                PeDelayImportNameError::UnsupportedAttributes {
                    descriptor_index: 1,
                    attributes: 2
                }
            ))
        );
        put32(&mut bytes, 544, 1);
        put32(&mut bytes, 548, 0x1090);
        assert_eq!(
            parse_pe_delay_import_lookups(&bytes),
            Err(PeDelayImportLookupError::Names(
                PeDelayImportNameError::EmptyDllName {
                    descriptor_index: 1,
                    name_rva: RelativeVirtualAddress::new(0x1090)
                }
            ))
        );
        put32(&mut bytes, 548, 0x1080);
        assert_eq!(
            parse_pe_delay_import_lookups(&bytes),
            Err(PeDelayImportLookupError::Lookup(
                PeImportLookupError::LookupTableUnavailable {
                    descriptor_index: 0
                }
            ))
        );
    }
}

#[test]
fn repeated_imports_retain_row_order_and_later_errors_have_original_indices() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, 0x1000, 96);
        descriptor(&mut bytes, 1, 0x1080, 0x10a0);
        let mut wanted = expected(plus);
        let mut second = wanted.imports[0].clone();
        second.import.descriptor.descriptor_rva = RelativeVirtualAddress::new(0x1020);
        second.import.descriptor.descriptor_file_offset = FileOffset::new(544);
        wanted.imports.push(second);
        wanted.directory_size = 96;
        wanted.terminator_rva = RelativeVirtualAddress::new(0x1040);
        wanted.terminator_file_offset = FileOffset::new(576);
        assert_eq!(parse_pe_delay_import_lookups(&bytes), Ok(Some(wanted)));
        put32(&mut bytes, 544 + 16, 0);
        let before = bytes.clone();
        assert_eq!(
            parse_pe_delay_import_lookups(&bytes),
            Err(PeDelayImportLookupError::Lookup(
                PeImportLookupError::LookupTableUnavailable {
                    descriptor_index: 1
                }
            ))
        );
        assert_eq!(bytes, before);
    }
}
