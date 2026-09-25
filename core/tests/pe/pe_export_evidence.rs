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

#[derive(Clone, Copy)]
enum CompiledExport {
    Named,
    Ordinal,
    Forwarders,
}

fn compiled_expected(kind: CompiledExport) -> PeExportEvidence {
    let (
        size,
        ordinal,
        address_count,
        name_count,
        addresses_rva,
        names_rva,
        ordinals_rva,
        rows,
        text,
    ) = match kind {
        CompiledExport::Named => (77, 1, 1, 1, 8247, 8251, 8255, 3, 11),
        CompiledExport::Ordinal => (61, 32768, 1, 0, 8249, 8253, 8253, 2, 0),
        CompiledExport::Forwarders => (147, 7, 3, 2, 8252, 8264, 8272, 8, 101),
    };
    let directory = PeExportDirectory {
        directory_rva: RelativeVirtualAddress::new(8192),
        directory_file_offset: FileOffset::new(1536),
        directory_size: size,
        flags: 0,
        time_date_stamp: 0,
        major_version: 0,
        minor_version: 0,
        name_rva: RelativeVirtualAddress::new(8232),
        ordinal_base: ordinal,
        address_table_entries: address_count,
        number_of_name_pointers: name_count,
        export_address_table_rva: RelativeVirtualAddress::new(addresses_rva),
        name_pointer_rva: RelativeVirtualAddress::new(names_rva),
        ordinal_table_rva: RelativeVirtualAddress::new(ordinals_rva),
    };
    let targets = match kind {
        CompiledExport::Named | CompiledExport::Ordinal => {
            vec![PeOwnedExportTarget::Rva(RelativeVirtualAddress::new(4096))]
        }
        CompiledExport::Forwarders => vec![
            PeOwnedExportTarget::Forwarder {
                rva: RelativeVirtualAddress::new(8295),
                text: "OtherModule.ring3_target".to_owned(),
            },
            PeOwnedExportTarget::Empty,
            PeOwnedExportTarget::Forwarder {
                rva: RelativeVirtualAddress::new(8320),
                text: "OtherModule.#32768".to_owned(),
            },
        ],
    };
    let addresses = PeOwnedExportAddressTable {
        directory,
        entries: targets
            .into_iter()
            .zip(0_u32..)
            .map(|(target, i)| PeOwnedExportAddressEntry {
                table_index: i,
                ordinal: ordinal + i,
                entry_rva: RelativeVirtualAddress::new(addresses_rva + i * 4),
                entry_file_offset: FileOffset::new(u64::from(addresses_rva - 6656 + i * 4)),
                target,
            })
            .collect(),
    };
    let names = match kind {
        CompiledExport::Named => vec![(0, 8257, "ring3_probe")],
        CompiledExport::Ordinal => vec![],
        CompiledExport::Forwarders => vec![(0, 8276, "by_name"), (2, 8284, "by_ordinal")],
    };
    let names = PeOwnedExportNameTable {
        addresses: addresses.clone(),
        entries: names
            .into_iter()
            .zip(0_u32..)
            .map(|((address_index, rva, name), i)| PeOwnedExportName {
                table_index: i,
                name_pointer_rva: RelativeVirtualAddress::new(names_rva + i * 4),
                name_pointer_file_offset: FileOffset::new(u64::from(names_rva - 6656 + i * 4)),
                ordinal_entry_rva: RelativeVirtualAddress::new(ordinals_rva + i * 2),
                ordinal_entry_file_offset: FileOffset::new(u64::from(ordinals_rva - 6656 + i * 2)),
                address_index,
                name_rva: RelativeVirtualAddress::new(rva),
                name_file_offset: FileOffset::new(u64::from(rva - 6656)),
                name: name.to_owned(),
            })
            .collect(),
    };
    PeExportEvidence {
        total_rows: rows,
        total_text_bytes: text,
        directory: Ok(Some(directory)),
        addresses: Ok(Some(addresses)),
        names: Ok(Some(names)),
    }
}

fn compiled_exports() -> Vec<(Vec<u8>, PeExportEvidence)> {
    [
        ("RING3_EXPORT_PE32_NAMED_DLL", CompiledExport::Named),
        ("RING3_EXPORT_PE32PLUS_NAMED_DLL", CompiledExport::Named),
        ("RING3_EXPORT_PE32_ORDINAL_DLL", CompiledExport::Ordinal),
        ("RING3_EXPORT_PE32PLUS_ORDINAL_DLL", CompiledExport::Ordinal),
        (
            "RING3_EXPORT_FORWARD_PE32_FIXTURE",
            CompiledExport::Forwarders,
        ),
        (
            "RING3_EXPORT_FORWARD_PE32PLUS_FIXTURE",
            CompiledExport::Forwarders,
        ),
    ]
    .into_iter()
    .map(|(variable, kind)| {
        let path = std::env::var_os(variable)
            .expect("all six explicit compiled export paths are required");
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(bytes.len(), 2048);
        (bytes, compiled_expected(kind))
    })
    .collect()
}

#[test]
#[ignore = "requires all six explicit compiled export fixture paths"]
fn generated_owned_exports_preserve_compiled_metadata() {
    for (mut bytes, expected) in compiled_exports() {
        let before = bytes.clone();
        let caps = PeExportEvidenceLimits {
            max_input_bytes: 2048,
            max_output_rows: expected.total_rows,
            max_output_text_bytes: expected.total_text_bytes,
        };
        let evidence = inspect_pe_exports(&bytes, caps).unwrap();
        assert_eq!(evidence, expected);
        assert_eq!(inspect_pe_exports(&bytes, caps), Ok(evidence.clone()));
        assert_eq!(bytes, before);
        bytes.fill(0xee);
        drop(bytes);
        assert_eq!(evidence, expected);
    }
}

#[test]
#[ignore = "requires all six explicit compiled export fixture paths"]
fn generated_owned_export_refusals_preserve_compiled_budget_operands() {
    for (bytes, expected) in compiled_exports() {
        let caps = PeExportEvidenceLimits {
            max_input_bytes: 2048,
            max_output_rows: expected.total_rows,
            max_output_text_bytes: expected.total_text_bytes,
        };
        assert_eq!(inspect_pe_exports(&bytes, caps), Ok(expected.clone()));
        assert_eq!(
            inspect_pe_exports(
                &bytes,
                PeExportEvidenceLimits {
                    max_input_bytes: 2047,
                    max_output_rows: 0,
                    max_output_text_bytes: 0,
                }
            ),
            Err(PeExportEvidenceError::InputTooLarge {
                length: 2048,
                limit: 2047
            })
        );
        assert_eq!(
            inspect_pe_exports(
                &bytes,
                PeExportEvidenceLimits {
                    max_output_rows: expected.total_rows - 1,
                    max_output_text_bytes: 0,
                    ..caps
                }
            ),
            Err(PeExportEvidenceError::OutputRowsExceeded {
                rows: expected.total_rows,
                limit: expected.total_rows - 1,
            })
        );
        if expected.total_text_bytes > 0 {
            assert_eq!(
                inspect_pe_exports(
                    &bytes,
                    PeExportEvidenceLimits {
                        max_output_text_bytes: expected.total_text_bytes - 1,
                        ..caps
                    }
                ),
                Err(PeExportEvidenceError::OutputTextExceeded {
                    bytes: expected.total_text_bytes,
                    limit: expected.total_text_bytes - 1,
                })
            );
        }
    }
}

use ring3_core::{
    PeExportEvidenceLookupError, PeExportEvidenceLookupLimits, PeExportLookupError, PeExportQuery,
    PeExportSelection, lookup_pe_export, lookup_pe_export_evidence,
};

struct CompiledQueryInput {
    bytes: Vec<u8>,
    expected: PeExportEvidence,
    queries: Vec<PeExportQuery<'static>>,
    ordinal_limits: PeExportEvidenceLookupLimits,
    name_limits: PeExportEvidenceLookupLimits,
}

fn compiled_query_inputs() -> Vec<CompiledQueryInput> {
    let paths: Vec<_> = [
        ("RING3_EXPORT_PE32_NAMED_DLL", CompiledExport::Named),
        ("RING3_EXPORT_PE32PLUS_NAMED_DLL", CompiledExport::Named),
        ("RING3_EXPORT_PE32_ORDINAL_DLL", CompiledExport::Ordinal),
        ("RING3_EXPORT_PE32PLUS_ORDINAL_DLL", CompiledExport::Ordinal),
        (
            "RING3_EXPORT_FORWARD_PE32_FIXTURE",
            CompiledExport::Forwarders,
        ),
        (
            "RING3_EXPORT_FORWARD_PE32PLUS_FIXTURE",
            CompiledExport::Forwarders,
        ),
    ]
    .into_iter()
    .map(|(variable, kind)| {
        let path = std::env::var_os(variable)
            .expect("all six explicit compiled owned export query paths are required");
        (path, kind)
    })
    .collect();
    paths
        .into_iter()
        .map(|(path, kind)| {
            let bytes = std::fs::read(path).unwrap();
            assert_eq!(bytes.len(), 2048);
            let (spelling, ordinals, ordinal_caps, name_caps): (&[&str], &[u32], _, _) = match kind
            {
                CompiledExport::Named => (
                    &["ring3_probe", "RING3_PROBE", "Missing"],
                    &[0, 1, 2],
                    (1, 0),
                    (2, 11),
                ),
                CompiledExport::Ordinal => (
                    &["Missing"],
                    &[0, 32767, 32768, 32769, u32::MAX],
                    (1, 0),
                    (1, 0),
                ),
                CompiledExport::Forwarders => (
                    &["by_name", "by_ordinal", "BY_NAME", "Missing"],
                    &[6, 7, 8, 9, 10, u32::MAX],
                    (3, 42),
                    (5, 59),
                ),
            };
            let queries = spelling
                .iter()
                .map(|name| PeExportQuery::Name(name))
                .chain(ordinals.iter().copied().map(PeExportQuery::Ordinal))
                .collect();
            let caps = |(rows, text)| PeExportEvidenceLookupLimits {
                max_table_rows: rows,
                max_table_text_bytes: text,
            };
            CompiledQueryInput {
                bytes,
                expected: compiled_expected(kind),
                queries,
                ordinal_limits: caps(ordinal_caps),
                name_limits: caps(name_caps),
            }
        })
        .collect()
}

fn compiled_query_limits(
    input: &CompiledQueryInput,
    query: PeExportQuery<'_>,
) -> PeExportEvidenceLookupLimits {
    match query {
        PeExportQuery::Name(_) => input.name_limits,
        PeExportQuery::Ordinal(_) => input.ordinal_limits,
    }
}

#[test]
#[ignore = "requires all six explicit compiled owned export query paths"]
fn generated_owned_export_queries_preserve_compiled_selections_after_release() {
    let mut outcomes = 0;
    for mut input in compiled_query_inputs() {
        let before = input.bytes.clone();
        let expected: Vec<_> = input
            .queries
            .iter()
            .map(|query| {
                format!(
                    "{:?}",
                    lookup_pe_export(&input.bytes, *query)
                        .map_err(PeExportEvidenceLookupError::Reader)
                )
            })
            .collect();
        let evidence = inspect_pe_exports(
            &input.bytes,
            PeExportEvidenceLimits {
                max_input_bytes: 2048,
                max_output_rows: input.expected.total_rows,
                max_output_text_bytes: input.expected.total_text_bytes,
            },
        )
        .unwrap();
        assert_eq!(evidence, input.expected);
        assert_eq!(input.bytes, before);
        drop(before);
        input.bytes.fill(0xee);
        drop(std::mem::take(&mut input.bytes));
        let mut previous = Vec::new();
        for (query, expected) in input.queries.iter().copied().zip(&expected) {
            let caps = compiled_query_limits(&input, query);
            let selection = {
                let spelling = match query {
                    PeExportQuery::Name(s) => Some(s.to_owned()),
                    PeExportQuery::Ordinal(_) => None,
                };
                let temporary = spelling.as_deref().map_or(query, PeExportQuery::Name);
                lookup_pe_export_evidence(&evidence, temporary, caps)
            };
            assert_eq!(format!("{selection:?}"), *expected);
            assert_eq!(lookup_pe_export_evidence(&evidence, query, caps), selection);
            previous.push(selection);
            outcomes += 1;
        }
        for (query, selection) in input.queries.iter().copied().zip(previous).rev() {
            assert_eq!(
                lookup_pe_export_evidence(&evidence, query, compiled_query_limits(&input, query)),
                selection
            );
        }
    }
    assert_eq!(outcomes, 44);
}

#[test]
#[ignore = "requires all six explicit compiled owned export query paths"]
fn generated_owned_export_query_refusals_preserve_compiled_view_limits() {
    let zero = PeExportEvidenceLookupLimits {
        max_table_rows: 0,
        max_table_text_bytes: 0,
    };
    let mut outcomes = 0;
    for mut input in compiled_query_inputs() {
        let mut evidence = inspect_pe_exports(
            &input.bytes,
            PeExportEvidenceLimits {
                max_input_bytes: 2048,
                max_output_rows: input.expected.total_rows,
                max_output_text_bytes: input.expected.total_text_bytes,
            },
        )
        .unwrap();
        assert_eq!(evidence, input.expected);
        input.bytes.fill(0xee);
        drop(std::mem::take(&mut input.bytes));
        evidence.total_rows = 0;
        evidence.total_text_bytes = 0;
        for query in input.queries.iter().copied() {
            let caps = compiled_query_limits(&input, query);
            let selected = lookup_pe_export_evidence(&evidence, query, caps);
            assert!(selected.is_ok());
            assert_eq!(
                lookup_pe_export_evidence(
                    &evidence,
                    query,
                    PeExportEvidenceLookupLimits {
                        max_table_rows: caps.max_table_rows - 1,
                        max_table_text_bytes: 0
                    }
                ),
                Err(PeExportEvidenceLookupError::RowsExceeded {
                    rows: caps.max_table_rows,
                    limit: caps.max_table_rows - 1
                })
            );
            if caps.max_table_text_bytes > 0 {
                assert_eq!(
                    lookup_pe_export_evidence(
                        &evidence,
                        query,
                        PeExportEvidenceLookupLimits {
                            max_table_text_bytes: caps.max_table_text_bytes - 1,
                            ..caps
                        }
                    ),
                    Err(PeExportEvidenceLookupError::TextExceeded {
                        bytes: caps.max_table_text_bytes,
                        limit: caps.max_table_text_bytes - 1
                    })
                );
            }
            let mut independent = evidence.clone();
            independent.directory = Ok(None);
            independent.total_rows = u64::MAX;
            independent.total_text_bytes = u64::MAX;
            let name_error = PeExportNameError::NameUnavailable { entry_index: 77 };
            let address_error = PeExportAddressError::AddressTableUnavailable { count: 99 };
            match query {
                PeExportQuery::Name(_) => independent.addresses = Err(address_error),
                PeExportQuery::Ordinal(_) => independent.names = Err(name_error),
            }
            assert_eq!(
                lookup_pe_export_evidence(&independent, query, caps),
                selected
            );
            let reader_error = match query {
                PeExportQuery::Name(_) => {
                    independent.names = Err(name_error);
                    PeExportLookupError::Names(name_error)
                }
                PeExportQuery::Ordinal(_) => {
                    independent.addresses = Err(address_error);
                    PeExportLookupError::Addresses(address_error)
                }
            };
            assert_eq!(
                lookup_pe_export_evidence(&independent, query, zero),
                Err(PeExportEvidenceLookupError::Reader(reader_error))
            );
            match query {
                PeExportQuery::Name(_) => independent.names = Ok(None),
                PeExportQuery::Ordinal(_) => independent.addresses = Ok(None),
            }
            assert_eq!(
                lookup_pe_export_evidence(&independent, query, zero),
                Ok(PeExportSelection::DirectoryAbsent)
            );
            outcomes += 1;
        }
    }
    assert_eq!(outcomes, 44);
}

#[test]
#[ignore = "requires all six explicit compiled owned export query paths"]
#[expect(
    clippy::too_many_lines,
    reason = "one complete compiled batch contract"
)]
fn generated_owned_export_batches_preserve_compiled_results_and_budgets() {
    use ring3_core::{
        PeExportBatchError, PeExportBatchLimits, lookup_pe_export_batch,
        lookup_pe_export_evidence_batch,
    };

    let mut outcomes = 0;
    for (mut input, (rows, last_selected)) in
        compiled_query_inputs()
            .into_iter()
            .zip([(2, 4), (2, 4), (1, 3), (1, 3), (5, 7), (5, 7)])
    {
        let count = input.queries.len() as u64;
        let caps = PeExportBatchLimits {
            max_queries: count,
            max_selection_rows: rows,
        };
        let raw = lookup_pe_export_batch(&input.bytes, &input.queries, caps).unwrap();
        assert_eq!(raw.selection_rows, rows);
        let expected: Vec<_> = raw
            .selections
            .into_iter()
            .map(|s| format!("{:?}", s.map_err(PeExportEvidenceLookupError::Reader)))
            .collect();
        let evidence = inspect_pe_exports(
            &input.bytes,
            PeExportEvidenceLimits {
                max_input_bytes: 2048,
                max_output_rows: input.expected.total_rows,
                max_output_text_bytes: input.expected.total_text_bytes,
            },
        )
        .unwrap();
        assert_eq!(evidence, input.expected);
        input.bytes.fill(0xee);
        drop(std::mem::take(&mut input.bytes));
        let batch =
            lookup_pe_export_evidence_batch(&evidence, &input.queries, input.name_limits, caps)
                .unwrap();
        assert_eq!(batch.selection_rows, rows);
        assert_eq!(
            batch
                .selections
                .iter()
                .map(|s| format!("{s:?}"))
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            lookup_pe_export_evidence_batch(&evidence, &input.queries, input.name_limits, caps),
            Ok(batch.clone())
        );
        assert_eq!(
            lookup_pe_export_evidence_batch(
                &evidence,
                &input.queries,
                input.name_limits,
                PeExportBatchLimits {
                    max_selection_rows: rows - 1,
                    ..caps
                }
            ),
            Err(PeExportBatchError::SelectionRowsExceeded {
                index: last_selected,
                total: rows,
                limit: rows - 1
            })
        );
        assert_eq!(
            lookup_pe_export_evidence_batch(
                &evidence,
                &input.queries,
                PeExportEvidenceLookupLimits {
                    max_table_rows: 0,
                    max_table_text_bytes: 0
                },
                PeExportBatchLimits {
                    max_queries: count - 1,
                    ..caps
                }
            ),
            Err(PeExportBatchError::QueryCountExceeded {
                count,
                limit: count - 1
            })
        );
        let empty = lookup_pe_export_evidence_batch(
            &evidence,
            &[],
            input.name_limits,
            PeExportBatchLimits {
                max_queries: 0,
                max_selection_rows: 0,
            },
        )
        .unwrap();
        assert_eq!(empty.selection_rows, 0);
        assert!(empty.selections.is_empty());
        input.queries.reverse();
        let reversed =
            lookup_pe_export_evidence_batch(&evidence, &input.queries, input.name_limits, caps)
                .unwrap();
        assert_eq!(reversed.selection_rows, rows);
        assert_eq!(
            reversed.selections,
            batch.selections.into_iter().rev().collect::<Vec<_>>()
        );
        outcomes += count;
    }
    assert_eq!(outcomes, 44);
}
