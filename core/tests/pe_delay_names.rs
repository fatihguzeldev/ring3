use ring3_core::{
    FileOffset, PeDelayImportDescriptor, PeDelayImportError, PeDelayImportName,
    PeDelayImportNameError, PeDelayImportNameTable, PeHeaderError, PeKind, PeRvaError,
    RelativeVirtualAddress, parse_pe_delay_import_names,
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

fn section(bytes: &mut [u8], plus: bool, index: usize, fields: [u32; 4]) {
    let table = 152 + fixed(plus) + 112 + index * 40;
    for (offset, value) in [8, 12, 16, 20].into_iter().zip(fields) {
        put32(bytes, table + offset, value);
    }
}

fn descriptor(bytes: &mut [u8], index: usize, attributes: u32, name: u32) {
    let offset = 512 + index * 32;
    for (i, value) in [attributes, name, u32::MAX, 2, 3, 4, 5, 6]
        .into_iter()
        .enumerate()
    {
        put32(bytes, offset + i * 4, value);
    }
}

fn fixture(plus: bool) -> Vec<u8> {
    let mut bytes = vec![0; 768];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&2_u16.to_le_bytes());
    let size = u16::try_from(fixed(plus) + 112).unwrap();
    bytes[148..150].copy_from_slice(&size.to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + fixed(plus) - 4, 14);
    directory(&mut bytes, plus, 0x1000, 64);
    section(&mut bytes, plus, 0, [128, 0x1000, 128, 512]);
    section(&mut bytes, plus, 1, [128, 0x6000, 128, 640]);
    descriptor(&mut bytes, 0, 1, 0x6000);
    bytes[640..655].copy_from_slice(b"Ring3Delay.dll\0");
    bytes
}

fn raw(index: u32, name: u32) -> PeDelayImportDescriptor {
    PeDelayImportDescriptor {
        descriptor_rva: RelativeVirtualAddress::new(0x1000 + index * 32),
        descriptor_file_offset: FileOffset::new(512 + u64::from(index) * 32),
        attributes: 1,
        dll_name_address: name,
        module_handle_address: u32::MAX,
        import_address_table_address: 2,
        import_name_table_address: 3,
        bound_import_address_table_address: 4,
        unload_import_address_table_address: 5,
        time_date_stamp: 6,
    }
}

fn expected(plus: bool) -> PeDelayImportNameTable<'static> {
    PeDelayImportNameTable {
        kind: if plus { PeKind::Pe32Plus } else { PeKind::Pe32 },
        directory_rva: RelativeVirtualAddress::new(0x1000),
        directory_file_offset: FileOffset::new(512),
        directory_size: 64,
        imports: vec![PeDelayImportName {
            descriptor: raw(0, 0x6000),
            dll_name: "Ring3Delay.dll",
        }],
        terminator_rva: RelativeVirtualAddress::new(0x1020),
        terminator_file_offset: FileOffset::new(544),
    }
}

#[test]
fn both_widths_preserve_exact_borrowed_name_raw_metadata_and_input() {
    for plus in [false, true] {
        let bytes = fixture(plus);
        let before = bytes.clone();
        let table = parse_pe_delay_import_names(&bytes).unwrap().unwrap();
        assert_eq!(table, expected(plus));
        assert_eq!(table.imports[0].dll_name.as_ptr(), bytes[640..].as_ptr());
        assert_eq!(bytes, before);
    }
}

#[test]
fn duplicate_names_retain_distinct_descriptors_and_original_order() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, 0x1000, 96);
        descriptor(&mut bytes, 1, 1, 0x6000);
        let table = parse_pe_delay_import_names(&bytes).unwrap().unwrap();
        assert_eq!(
            table,
            PeDelayImportNameTable {
                directory_size: 96,
                imports: vec![
                    PeDelayImportName {
                        descriptor: raw(0, 0x6000),
                        dll_name: "Ring3Delay.dll"
                    },
                    PeDelayImportName {
                        descriptor: raw(1, 0x6000),
                        dll_name: "Ring3Delay.dll"
                    },
                ],
                terminator_rva: RelativeVirtualAddress::new(0x1040),
                terminator_file_offset: FileOffset::new(576),
                ..expected(plus)
            }
        );
        assert_eq!(
            table.imports[0].dll_name.as_ptr(),
            table.imports[1].dll_name.as_ptr()
        );
    }
}

#[test]
fn absent_and_present_empty_tables_keep_the_raw_distinction() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        bytes[512..544].fill(0);
        assert_eq!(
            parse_pe_delay_import_names(&bytes),
            Ok(Some(PeDelayImportNameTable {
                imports: Vec::new(),
                terminator_rva: RelativeVirtualAddress::new(0x1000),
                terminator_file_offset: FileOffset::new(512),
                ..expected(plus)
            }))
        );
        directory(&mut bytes, plus, 0, 0);
        assert_eq!(parse_pe_delay_import_names(&bytes), Ok(None));
        directory(&mut bytes, plus, u32::MAX, u32::MAX);
        put32(&mut bytes, 152 + fixed(plus) - 4, 13);
        assert_eq!(parse_pe_delay_import_names(&bytes), Ok(None));
    }
}

#[test]
fn unsupported_attributes_precede_name_targets() {
    for plus in [false, true] {
        for attributes in [0, 2, 3, u32::MAX] {
            let mut bytes = fixture(plus);
            descriptor(&mut bytes, 0, attributes, u32::MAX);
            assert_eq!(
                parse_pe_delay_import_names(&bytes),
                Err(PeDelayImportNameError::UnsupportedAttributes {
                    descriptor_index: 0,
                    attributes,
                })
            );
        }
    }
}

#[test]
fn the_complete_raw_table_is_validated_before_any_name_or_attribute() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        descriptor(&mut bytes, 0, 0, u32::MAX);
        directory(&mut bytes, plus, 0x1000, 32);
        assert_eq!(
            parse_pe_delay_import_names(&bytes),
            Err(PeDelayImportNameError::Table(
                PeDelayImportError::MissingTerminator {
                    descriptor_index: 1
                }
            ))
        );
        directory(&mut bytes, plus, 0, 0);
        bytes[0] = 0;
        assert_eq!(
            parse_pe_delay_import_names(&bytes),
            Err(PeDelayImportNameError::Table(PeDelayImportError::Base(
                PeRvaError::Parse(PeHeaderError::InvalidDosSignature {
                    offset: FileOffset::new(0)
                })
            )))
        );
    }
}
