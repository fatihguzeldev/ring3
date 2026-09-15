use ring3_core::{
    FileOffset, PeExportAddressError, PeExportDirectory, PeExportDirectoryError, PeExportEvidence,
    PeExportEvidenceError, PeExportEvidenceLimits, PeExportNameError, PeOwnedExportAddressEntry,
    PeOwnedExportAddressTable, PeOwnedExportName, PeOwnedExportNameTable, PeOwnedExportTarget,
    RelativeVirtualAddress, inspect_pe_exports,
};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixture(plus: bool) -> Vec<u8> {
    let mut bytes = vec![0; 4096];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&1_u16.to_le_bytes());
    let fixed = if plus { 112_u16 } else { 96 };
    bytes[148..150].copy_from_slice(&(fixed + 8).to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + usize::from(fixed) - 4, 1);
    put32(&mut bytes, 152 + usize::from(fixed), 0x1000);
    put32(&mut bytes, 156 + usize::from(fixed), 0x100);
    for (offset, value) in [(8, 3584), (12, 0x1000), (16, 3584), (20, 512)] {
        put32(&mut bytes, 160 + usize::from(fixed) + offset, value);
    }
    for (offset, value) in [
        (0, u32::MAX),
        (4, 0xabcd_ef12),
        (12, u32::MAX),
        (16, 0xffff_fff9),
        (20, 6),
        (24, 4),
        (28, 0x1200),
        (32, 0x1300),
        (36, 0x1340),
    ] {
        put32(&mut bytes, 512 + offset, value);
    }
    bytes[520..522].copy_from_slice(&u16::MAX.to_le_bytes());
    bytes[522..524].copy_from_slice(&32768_u16.to_le_bytes());
    bytes[640..652].copy_from_slice(b"M.#00032768\0");
    bytes[672..683].copy_from_slice(b"A.B.C\t\x01\"\\\x7f\0");
    for (i, target) in [0, 1, 0x1080, u32::MAX, 0x10a0, 0x1080]
        .into_iter()
        .enumerate()
    {
        put32(&mut bytes, 1024 + i * 4, target);
    }
    for (i, (name, index)) in [(0x1400, 5_u16), (0x1440, 1), (0x1400, 2), (0x1480, 2)]
        .into_iter()
        .enumerate()
    {
        put32(&mut bytes, 1280 + i * 4, name);
        bytes[1344 + i * 2..1346 + i * 2].copy_from_slice(&index.to_le_bytes());
    }
    bytes[1536..1546].copy_from_slice(b"zeta\t\x01\"\\\x7f\0");
    bytes[1600..1606].copy_from_slice(b"Alpha\0");
    bytes[1664..1670].copy_from_slice(b"Alias\0");
    bytes
}

fn limits() -> PeExportEvidenceLimits {
    PeExportEvidenceLimits {
        max_input_bytes: 4096,
        max_output_rows: 16,
        max_output_text_bytes: 92,
    }
}

fn directory() -> PeExportDirectory {
    PeExportDirectory {
        directory_rva: RelativeVirtualAddress::new(0x1000),
        directory_file_offset: FileOffset::new(512),
        directory_size: 0x100,
        flags: u32::MAX,
        time_date_stamp: 0xabcd_ef12,
        major_version: u16::MAX,
        minor_version: 32768,
        name_rva: RelativeVirtualAddress::new(u32::MAX),
        ordinal_base: 0xffff_fff9,
        address_table_entries: 6,
        number_of_name_pointers: 4,
        export_address_table_rva: RelativeVirtualAddress::new(0x1200),
        name_pointer_rva: RelativeVirtualAddress::new(0x1300),
        ordinal_table_rva: RelativeVirtualAddress::new(0x1340),
    }
}

fn expected() -> PeExportEvidence {
    let first = PeOwnedExportTarget::Forwarder {
        rva: RelativeVirtualAddress::new(0x1080),
        text: "M.#00032768".to_owned(),
    };
    let addresses = PeOwnedExportAddressTable {
        directory: directory(),
        entries: [
            PeOwnedExportTarget::Empty,
            PeOwnedExportTarget::Rva(RelativeVirtualAddress::new(1)),
            first.clone(),
            PeOwnedExportTarget::Rva(RelativeVirtualAddress::new(u32::MAX)),
            PeOwnedExportTarget::Forwarder {
                rva: RelativeVirtualAddress::new(0x10a0),
                text: "A.B.C\t\x01\"\\\x7f".to_owned(),
            },
            first,
        ]
        .into_iter()
        .zip(0_u32..)
        .map(|(target, i)| PeOwnedExportAddressEntry {
            table_index: i,
            ordinal: 0xffff_fff9 + i,
            entry_rva: RelativeVirtualAddress::new(0x1200 + i * 4),
            entry_file_offset: FileOffset::new(1024 + u64::from(i) * 4),
            target,
        })
        .collect(),
    };
    let names = PeOwnedExportNameTable {
        addresses: addresses.clone(),
        entries: [
            (0x1400, 1536, 5, "zeta\t\x01\"\\\x7f"),
            (0x1440, 1600, 1, "Alpha"),
            (0x1400, 1536, 2, "zeta\t\x01\"\\\x7f"),
            (0x1480, 1664, 2, "Alias"),
        ]
        .into_iter()
        .zip(0_u32..)
        .map(
            |((rva, offset, address_index, name), i)| PeOwnedExportName {
                table_index: i,
                name_pointer_rva: RelativeVirtualAddress::new(0x1300 + i * 4),
                name_pointer_file_offset: FileOffset::new(1280 + u64::from(i) * 4),
                ordinal_entry_rva: RelativeVirtualAddress::new(0x1340 + i * 2),
                ordinal_entry_file_offset: FileOffset::new(1344 + u64::from(i) * 2),
                address_index,
                name_rva: RelativeVirtualAddress::new(rva),
                name_file_offset: FileOffset::new(offset),
                name: name.to_owned(),
            },
        )
        .collect(),
    };
    PeExportEvidence {
        total_rows: 16,
        total_text_bytes: 92,
        directory: Ok(Some(directory())),
        addresses: Ok(Some(addresses)),
        names: Ok(Some(names)),
    }
}

#[test]
fn owns_complete_export_metadata_after_input_release() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        let before = bytes.clone();
        let evidence = inspect_pe_exports(&bytes, limits()).unwrap();
        assert_eq!(evidence, expected());
        assert_eq!(inspect_pe_exports(&bytes, limits()), Ok(evidence.clone()));
        assert_eq!(bytes, before);
        bytes.fill(0xee);
        drop(bytes);
        assert_eq!(evidence, expected());
    }
}

#[test]
fn admits_input_then_complete_rows_then_complete_text() {
    for plus in [false, true] {
        let bytes = fixture(plus);
        for (caps, error) in [
            (
                (4095, 0, 0),
                PeExportEvidenceError::InputTooLarge {
                    length: 4096,
                    limit: 4095,
                },
            ),
            (
                (4096, 15, 0),
                PeExportEvidenceError::OutputRowsExceeded {
                    rows: 16,
                    limit: 15,
                },
            ),
            (
                (4096, 16, 91),
                PeExportEvidenceError::OutputTextExceeded {
                    bytes: 92,
                    limit: 91,
                },
            ),
        ] {
            assert_eq!(
                inspect_pe_exports(
                    &bytes,
                    PeExportEvidenceLimits {
                        max_input_bytes: caps.0,
                        max_output_rows: caps.1,
                        max_output_text_bytes: caps.2,
                    }
                ),
                Err(error)
            );
        }
        assert_eq!(inspect_pe_exports(&bytes, limits()), Ok(expected()));
    }
}

#[test]
fn keeps_absence_distinct_from_zero_metadata_tables() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        let slot = if plus { 264 } else { 248 };
        let caps = PeExportEvidenceLimits {
            max_output_rows: 0,
            max_output_text_bytes: 0,
            ..limits()
        };
        for missing_count in [false, true] {
            let mut absent = bytes.clone();
            if missing_count {
                put32(&mut absent, slot - 4, 0);
            } else {
                absent[slot..slot + 8].fill(0);
            }
            assert_eq!(
                inspect_pe_exports(&absent, caps),
                Ok(PeExportEvidence {
                    total_rows: 0,
                    total_text_bytes: 0,
                    directory: Ok(None),
                    addresses: Ok(None),
                    names: Ok(None),
                })
            );
        }
        bytes[512..552].fill(0);
        let empty = PeExportDirectory {
            flags: 0,
            time_date_stamp: 0,
            major_version: 0,
            minor_version: 0,
            name_rva: RelativeVirtualAddress::new(0),
            ordinal_base: 0,
            address_table_entries: 0,
            number_of_name_pointers: 0,
            export_address_table_rva: RelativeVirtualAddress::new(0),
            name_pointer_rva: RelativeVirtualAddress::new(0),
            ordinal_table_rva: RelativeVirtualAddress::new(0),
            ..directory()
        };
        let addresses = PeOwnedExportAddressTable {
            directory: empty,
            entries: vec![],
        };
        assert_eq!(
            inspect_pe_exports(&bytes, caps),
            Ok(PeExportEvidence {
                total_rows: 0,
                total_text_bytes: 0,
                directory: Ok(Some(empty)),
                addresses: Ok(Some(addresses.clone())),
                names: Ok(Some(PeOwnedExportNameTable {
                    addresses,
                    entries: vec![]
                })),
            })
        );
    }
}

#[test]
fn charges_nested_addresses_when_name_count_is_zero() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        put32(&mut bytes, 536, 0);
        let mut wanted = expected();
        wanted.total_rows = 12;
        wanted.total_text_bytes = 64;
        wanted
            .directory
            .as_mut()
            .unwrap()
            .as_mut()
            .unwrap()
            .number_of_name_pointers = 0;
        wanted
            .addresses
            .as_mut()
            .unwrap()
            .as_mut()
            .unwrap()
            .directory
            .number_of_name_pointers = 0;
        let names = wanted.names.as_mut().unwrap().as_mut().unwrap();
        names.addresses.directory.number_of_name_pointers = 0;
        names.entries.clear();
        let caps = PeExportEvidenceLimits {
            max_output_rows: 12,
            max_output_text_bytes: 64,
            ..limits()
        };
        assert_eq!(inspect_pe_exports(&bytes, caps), Ok(wanted));
        assert_eq!(
            inspect_pe_exports(
                &bytes,
                PeExportEvidenceLimits {
                    max_output_rows: 6,
                    ..caps
                }
            ),
            Err(PeExportEvidenceError::OutputRowsExceeded { rows: 12, limit: 6 })
        );
        assert_eq!(
            inspect_pe_exports(
                &bytes,
                PeExportEvidenceLimits {
                    max_output_text_bytes: 63,
                    ..caps
                }
            ),
            Err(PeExportEvidenceError::OutputTextExceeded {
                bytes: 64,
                limit: 63
            })
        );
    }
}

#[test]
fn keeps_earlier_views_after_late_name_or_address_failure() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        bytes[1350..1352].copy_from_slice(&6_u16.to_le_bytes());
        let mut wanted = expected();
        wanted.total_rows = 6;
        wanted.total_text_bytes = 32;
        wanted.names = Err(PeExportNameError::AddressIndexOutOfRange {
            entry_index: 3,
            address_index: 6,
            address_count: 6,
        });
        assert_eq!(
            inspect_pe_exports(
                &bytes,
                PeExportEvidenceLimits {
                    max_output_rows: 6,
                    max_output_text_bytes: 32,
                    ..limits()
                }
            ),
            Ok(wanted)
        );
        put32(&mut bytes, 536, 4097);
        bytes[640] = 0;
        let error = PeExportAddressError::EmptyForwarder {
            entry_index: 2,
            start: RelativeVirtualAddress::new(0x1080),
        };
        assert_eq!(
            inspect_pe_exports(
                &bytes,
                PeExportEvidenceLimits {
                    max_output_rows: 0,
                    max_output_text_bytes: 0,
                    ..limits()
                }
            ),
            Ok(PeExportEvidence {
                total_rows: 0,
                total_text_bytes: 0,
                directory: Ok(Some(PeExportDirectory {
                    number_of_name_pointers: 4097,
                    ..directory()
                })),
                addresses: Err(error),
                names: Err(PeExportNameError::Addresses(error)),
            })
        );
    }
}

#[test]
fn preserves_u32_ordinal_edge_and_later_overflow() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        put32(&mut bytes, 528, 0xffff_fffa);
        let mut wanted = expected();
        wanted
            .directory
            .as_mut()
            .unwrap()
            .as_mut()
            .unwrap()
            .ordinal_base += 1;
        for table in [
            wanted.addresses.as_mut().unwrap().as_mut().unwrap(),
            &mut wanted.names.as_mut().unwrap().as_mut().unwrap().addresses,
        ] {
            table.directory.ordinal_base += 1;
            for entry in &mut table.entries {
                entry.ordinal += 1;
            }
        }
        assert_eq!(inspect_pe_exports(&bytes, limits()), Ok(wanted));
        put32(&mut bytes, 528, 0xffff_fffb);
        let error = PeExportAddressError::OrdinalOverflow {
            ordinal_base: 0xffff_fffb,
            entry_index: 5,
        };
        assert_eq!(
            inspect_pe_exports(
                &bytes,
                PeExportEvidenceLimits {
                    max_output_rows: 0,
                    max_output_text_bytes: 0,
                    ..limits()
                }
            ),
            Ok(PeExportEvidence {
                total_rows: 0,
                total_text_bytes: 0,
                directory: Ok(Some(PeExportDirectory {
                    ordinal_base: 0xffff_fffb,
                    ..directory()
                })),
                addresses: Err(error),
                names: Err(PeExportNameError::Addresses(error)),
            })
        );
    }
}

#[test]
fn retains_all_typed_directory_failures_with_zero_output_budget() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        put32(&mut bytes, if plus { 268 } else { 252 }, 39);
        let error = PeExportDirectoryError::TruncatedExportDirectory {
            rva: RelativeVirtualAddress::new(0x1000),
            size: 39,
            required: 40,
        };
        assert_eq!(
            inspect_pe_exports(
                &bytes,
                PeExportEvidenceLimits {
                    max_output_rows: 0,
                    max_output_text_bytes: 0,
                    ..limits()
                }
            ),
            Ok(PeExportEvidence {
                total_rows: 0,
                total_text_bytes: 0,
                directory: Err(error),
                addresses: Err(PeExportAddressError::Directory(error)),
                names: Err(PeExportNameError::Addresses(
                    PeExportAddressError::Directory(error)
                )),
            })
        );
    }
}
