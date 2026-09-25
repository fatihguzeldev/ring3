use std::collections::BTreeMap;
use std::fmt::{self, Write as _};

use ring3_core::{
    PeBoundImportError, PeBoundImportNameError, PeBoundImportNameLocation, PeBoundImportNameTable,
    PeBoundImportTable, parse_pe_bound_import_descriptors, parse_pe_bound_import_names,
};
use sha2::{Digest as _, Sha256};

const CASE_COUNT: u32 = 20_218;
const INPUT_SHA256: &str = "5d276632ea6d692a3e7a942ec030b326ea600593664d805853dd11371ded6b8b";

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn directory(plus: bool) -> usize {
    152 + if plus { 112 } else { 96 }
}

fn fixture(plus: bool) -> Vec<u8> {
    let mut bytes = vec![0; 1024];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    put16(&mut bytes, 134, 2);
    let directory = directory(plus);
    put16(
        &mut bytes,
        148,
        u16::try_from(directory + 128 - 152).unwrap(),
    );
    put16(&mut bytes, 152, if plus { 0x20b } else { 0x10b });
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, directory - 4, 16);
    let table = directory + 128;
    for (offset, value) in [8, 12, 16, 20].into_iter().zip([512, 4096, 512, 512]) {
        put32(&mut bytes, table + offset, value);
    }
    put32(&mut bytes, table + 52, 8192);
    put32(&mut bytes, table + 60, 1024);
    put32(&mut bytes, directory + 88, 4096);
    put32(&mut bytes, directory + 92, 40);
    for (offset, stamp, name, count) in [
        (512, 0x1234_5678, 64, 1),
        (520, 0x0102_0304, 80, 0x4321),
        (528, 0xabcd_ef01, 96, 1),
        (536, 0, 80, 0),
    ] {
        put32(&mut bytes, offset, stamp);
        put16(&mut bytes, offset + 4, name);
        put16(&mut bytes, offset + 6, count);
    }
    for (offset, text) in [
        (576, b"a.dll\0".as_slice()),
        (592, b"B.dll\0"),
        (608, b"c\x01/\x7f.dll\0"),
    ] {
        bytes[offset..offset + text.len()].copy_from_slice(text);
    }
    bytes
}

fn next(state: &mut u32) -> u32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    *state
}

fn index(state: &mut u32, length: usize) -> usize {
    usize::try_from(next(state)).unwrap() % length
}

fn visit_inputs(mut visit: impl FnMut(&[u8], bool)) {
    let mut state = 0x7233_4249;
    for plus in [false, true] {
        let original = fixture(plus);
        visit(&original, true);
        for variant in 0..4 {
            let mut bytes = original.clone();
            match variant {
                0 => bytes[directory(plus) + 88..directory(plus) + 96].fill(0),
                1 => bytes[512..520].fill(0),
                2 => bytes[576] = 255,
                _ => bytes[576] = 0,
            }
            visit(&bytes, false);
        }
        for end in 0..original.len() {
            visit(&original[..end], false);
        }
        for position in 0..original.len() {
            for bit in 0..8 {
                let mut bytes = original.clone();
                bytes[position] ^= 1 << bit;
                visit(&bytes, false);
            }
        }
        visit_word_edges(&original, plus, &mut visit);
        for round in 0..512 {
            let mut bytes = original.clone();
            let changes = 1 + next(&mut state) % 8;
            for _ in 0..changes {
                let position = index(&mut state, bytes.len());
                bytes[position] = next(&mut state).to_le_bytes()[0];
            }
            if round % 4 == 0 {
                bytes.truncate(index(&mut state, original.len()));
            }
            visit(&bytes, false);
        }
    }
}

fn visit_word_edges(original: &[u8], plus: bool, visit: &mut impl FnMut(&[u8], bool)) {
    let directory = directory(plus);
    let table = directory + 128;
    let words = [
        60,
        148,
        212,
        directory - 4,
        directory + 88,
        directory + 92,
        table + 8,
        table + 12,
        table + 16,
        table + 20,
        table + 48,
        table + 52,
        table + 56,
        table + 60,
        512,
        520,
        528,
        536,
    ];
    for position in words {
        for value in [
            0,
            1,
            7,
            8,
            15,
            16,
            39,
            40,
            127,
            128,
            1023,
            1024,
            65535,
            65536,
            0xffff_fff8,
            u32::MAX,
        ] {
            let mut bytes = original.to_vec();
            put32(&mut bytes, position, value);
            visit(&bytes, false);
        }
    }
    for position in [516, 518, 524, 526, 532, 534, 540, 542] {
        for value in [0, 1, 2, 127, 128, 129, 1023, 1024, 1025, 0x7fff, u16::MAX] {
            let mut bytes = original.to_vec();
            put16(&mut bytes, position, value);
            visit(&bytes, false);
        }
    }
}

fn raw_error(error: PeBoundImportError) -> &'static str {
    match error {
        PeBoundImportError::Base(_) => "Base",
        PeBoundImportError::InconsistentDirectory { .. } => "InconsistentDirectory",
        PeBoundImportError::DirectoryRangeOverflow { .. } => "DirectoryRangeOverflow",
        PeBoundImportError::MissingTerminator { .. } => "MissingTerminator",
        PeBoundImportError::TruncatedDescriptor { .. } => "TruncatedDescriptor",
        PeBoundImportError::DescriptorRange { .. } => "DescriptorRange",
        PeBoundImportError::DescriptorLimitExceeded { .. } => "DescriptorLimitExceeded",
        PeBoundImportError::ForwarderLimitExceeded { .. } => "ForwarderLimitExceeded",
        PeBoundImportError::TruncatedForwarderRefs { .. } => "TruncatedForwarderRefs",
        PeBoundImportError::ForwarderRange { .. } => "ForwarderRange",
    }
}

fn name_error(error: PeBoundImportNameError) -> &'static str {
    match error {
        PeBoundImportNameError::Table(_) => "Table",
        PeBoundImportNameError::NameRvaOverflow { .. } => "NameRvaOverflow",
        PeBoundImportNameError::NameRange { .. } => "NameRange",
        PeBoundImportNameError::EmptyDllName { .. } => "EmptyDllName",
        PeBoundImportNameError::NonAsciiDllName { .. } => "NonAsciiDllName",
        PeBoundImportNameError::NameLengthLimitExceeded { .. } => "NameLengthLimitExceeded",
        PeBoundImportNameError::NameScanBudgetExceeded { .. } => "NameScanBudgetExceeded",
    }
}

fn record(bytes: &[u8], offset: u64, stamp: u32, name: u16, last: u16) {
    let offset = usize::try_from(offset).unwrap();
    let raw = bytes.get(offset..offset + 8).unwrap();
    assert_eq!(&raw[..4], &stamp.to_le_bytes());
    assert_eq!(&raw[4..6], &name.to_le_bytes());
    assert_eq!(&raw[6..], &last.to_le_bytes());
}

fn check_raw(bytes: &[u8], table: &PeBoundImportTable) {
    let mut cursor = 0_u32;
    let mut references = 0;
    assert!(table.descriptors.len() <= 128);
    for descriptor in &table.descriptors {
        assert_eq!(
            descriptor.descriptor_rva.get(),
            table.directory_rva.get() + cursor
        );
        assert_eq!(
            descriptor.descriptor_file_offset.get(),
            table.directory_file_offset.get() + u64::from(cursor)
        );
        assert_eq!(
            descriptor.forwarder_refs.len(),
            usize::from(descriptor.number_of_module_forwarder_refs)
        );
        record(
            bytes,
            descriptor.descriptor_file_offset.get(),
            descriptor.time_date_stamp,
            descriptor.module_name_offset,
            descriptor.number_of_module_forwarder_refs,
        );
        assert!(
            descriptor.time_date_stamp != 0
                || descriptor.module_name_offset != 0
                || descriptor.number_of_module_forwarder_refs != 0
        );
        cursor += 8;
        for reference in &descriptor.forwarder_refs {
            assert_eq!(
                reference.reference_rva.get(),
                table.directory_rva.get() + cursor
            );
            assert_eq!(
                reference.reference_file_offset.get(),
                table.directory_file_offset.get() + u64::from(cursor)
            );
            record(
                bytes,
                reference.reference_file_offset.get(),
                reference.time_date_stamp,
                reference.module_name_offset,
                reference.reserved,
            );
            cursor += 8;
            references += 1;
        }
    }
    assert!(references <= 1024);
    assert_eq!(
        table.terminator_rva.get(),
        table.directory_rva.get() + cursor
    );
    assert_eq!(
        table.terminator_file_offset.get(),
        table.directory_file_offset.get() + u64::from(cursor)
    );
    record(bytes, table.terminator_file_offset.get(), 0, 0, 0);
    assert!(cursor + 8 <= table.directory_size && cursor + 8 <= 9224);
}

fn check_names(bytes: &[u8], names: &PeBoundImportNameTable<'_>) {
    let mut expected = Vec::new();
    for (descriptor_index, descriptor) in (0_u16..).zip(&names.table.descriptors) {
        expected.push((
            PeBoundImportNameLocation::Descriptor { descriptor_index },
            descriptor.module_name_offset,
        ));
        for (forwarder_index, reference) in (0_u16..).zip(&descriptor.forwarder_refs) {
            expected.push((
                PeBoundImportNameLocation::Forwarder {
                    descriptor_index,
                    forwarder_index,
                },
                reference.module_name_offset,
            ));
        }
    }
    assert_eq!(names.names.len(), expected.len());
    assert!(names.names.len() <= 1152);
    let mut scans = 0;
    for (entry, (location, offset)) in names.names.iter().zip(expected) {
        assert_eq!(entry.location, location);
        assert_eq!(
            Some(entry.name_rva.get()),
            names
                .table
                .directory_rva
                .get()
                .checked_add(u32::from(offset))
        );
        let start = usize::try_from(entry.name_file_offset.get()).unwrap();
        let end = start.checked_add(entry.dll_name.len()).unwrap();
        assert!(!entry.dll_name.is_empty() && entry.dll_name.len() < 1024);
        assert!(entry.dll_name.is_ascii() && !entry.dll_name.as_bytes().contains(&0));
        assert_eq!(bytes.get(start..end), Some(entry.dll_name.as_bytes()));
        assert_eq!(bytes.get(end), Some(&0));
        assert_eq!(entry.dll_name.as_ptr(), bytes[start..].as_ptr());
        scans += entry.dll_name.len() + 1;
    }
    assert!(scans <= 65536);
}

#[derive(Debug)]
struct OutcomeDigest(u64);

impl fmt::Write for OutcomeDigest {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for byte in text.bytes() {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
        }
        Ok(())
    }
}

#[derive(Debug)]
struct Campaign {
    cases: u32,
    input_sha256: String,
    outcome_digest: OutcomeDigest,
    raw_classes: [u32; 4],
    name_classes: [u32; 4],
    raw_errors: BTreeMap<&'static str, u32>,
    name_errors: BTreeMap<&'static str, u32>,
}

fn input_guard(before: &[u8], after: &[u8]) {
    assert_eq!(before, after, "mutation reader changed its input");
}

fn inspect(bytes: &[u8], baseline: bool, report: &mut Campaign) {
    let before = bytes.to_vec();
    let mut owned = bytes.to_vec();
    let raw = parse_pe_bound_import_descriptors(&owned);
    owned.fill(0);
    drop(owned);
    assert_eq!(
        raw,
        parse_pe_bound_import_descriptors(bytes),
        "raw case {}",
        report.cases
    );
    let names = parse_pe_bound_import_names(bytes);
    assert_eq!(
        names,
        parse_pe_bound_import_names(bytes),
        "names case {}",
        report.cases
    );
    match (&raw, &names) {
        (Err(error), Err(PeBoundImportNameError::Table(cause))) => assert_eq!(error, cause),
        (Ok(None), Ok(None)) => {}
        (Ok(Some(table)), Ok(Some(names))) => {
            assert_eq!(table, &names.table);
            check_names(bytes, names);
        }
        (Ok(Some(_)), Err(error)) => assert!(!matches!(error, PeBoundImportNameError::Table(_))),
        _ => panic!(
            "raw/name outcomes disagree at case {}: {raw:?} {names:?}",
            report.cases
        ),
    }
    let raw_class = match &raw {
        Ok(None) => 0,
        Ok(Some(table)) => {
            check_raw(bytes, table);
            if table.descriptors.is_empty() { 1 } else { 2 }
        }
        Err(error) => {
            *report.raw_errors.entry(raw_error(*error)).or_default() += 1;
            3
        }
    };
    let name_class = match &names {
        Ok(None) => 0,
        Ok(Some(table)) => {
            if table.names.is_empty() {
                1
            } else {
                2
            }
        }
        Err(error) => {
            *report.name_errors.entry(name_error(*error)).or_default() += 1;
            3
        }
    };
    report.raw_classes[raw_class] += 1;
    report.name_classes[name_class] += 1;
    if baseline {
        let table = names.as_ref().unwrap().as_ref().unwrap();
        assert_eq!(table.table.descriptors.len(), 2);
        assert!(
            table
                .table
                .descriptors
                .iter()
                .all(|d| d.forwarder_refs.len() == 1)
        );
        assert_eq!(
            table.names.iter().map(|n| n.dll_name).collect::<Vec<_>>(),
            ["a.dll", "B.dll", "c\x01/\x7f.dll", "B.dll"]
        );
    }
    write!(
        report.outcome_digest,
        "{}\0{raw:?}\0{names:?}\0",
        report.cases
    )
    .unwrap();
    input_guard(&before, bytes);
    report.cases += 1;
}

fn campaign() -> Campaign {
    let mut report = Campaign {
        cases: 0,
        input_sha256: String::new(),
        outcome_digest: OutcomeDigest(0xcbf2_9ce4_8422_2325),
        raw_classes: [0; 4],
        name_classes: [0; 4],
        raw_errors: [
            "Base",
            "InconsistentDirectory",
            "DirectoryRangeOverflow",
            "MissingTerminator",
            "TruncatedDescriptor",
            "DescriptorRange",
            "DescriptorLimitExceeded",
            "ForwarderLimitExceeded",
            "TruncatedForwarderRefs",
            "ForwarderRange",
        ]
        .into_iter()
        .map(|name| (name, 0))
        .collect(),
        name_errors: [
            "Table",
            "NameRvaOverflow",
            "NameRange",
            "EmptyDllName",
            "NonAsciiDllName",
            "NameLengthLimitExceeded",
            "NameScanBudgetExceeded",
        ]
        .into_iter()
        .map(|name| (name, 0))
        .collect(),
    };
    let mut input_hash = Sha256::new();
    visit_inputs(|bytes, baseline| {
        input_hash.update(report.cases.to_le_bytes());
        input_hash.update(u32::try_from(bytes.len()).unwrap().to_le_bytes());
        input_hash.update(bytes);
        inspect(bytes, baseline, &mut report);
    });
    for byte in input_hash.finalize() {
        write!(report.input_sha256, "{byte:02x}").unwrap();
    }
    assert_eq!(report.input_sha256, INPUT_SHA256);
    assert_eq!(report.cases, CASE_COUNT);
    for classes in [report.raw_classes, report.name_classes] {
        assert_eq!(classes.iter().sum::<u32>(), CASE_COUNT);
        assert!(classes.iter().all(|&count| count > 0));
    }
    assert_eq!(
        report.raw_errors.values().sum::<u32>(),
        report.raw_classes[3]
    );
    assert_eq!(
        report.name_errors.values().sum::<u32>(),
        report.name_classes[3]
    );
    report
}

#[test]
fn deterministic_bound_mutations_preserve_records_names_and_input() {
    campaign();
}
