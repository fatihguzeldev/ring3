use std::fmt::{self, Write as _};

use ring3_core::{
    PeResourceDataEntryError, PeResourceDirectoryError, PeResourceDirectoryNameError,
    RelativeVirtualAddress, parse_pe_base_relocation_blocks, parse_pe_certificate_entries,
    parse_pe_certificate_table, parse_pe_debug_directory, parse_pe_delay_import_descriptors,
    parse_pe_delay_import_lookups, parse_pe_delay_import_names, parse_pe_export_addresses,
    parse_pe_export_directory, parse_pe_export_names, parse_pe_header_prefix, parse_pe_headers,
    parse_pe_import_descriptors, parse_pe_import_lookups, parse_pe_load_config_prefix,
    parse_pe_resource_data_entries, parse_pe_resource_directories,
    parse_pe_resource_directory_names, parse_pe_resource_root, parse_pe_resource_root_names,
    parse_pe_sections, parse_pe_tls_directory, resolve_pe_file_range,
};

const READER_COUNT: usize = 23;
const CASE_COUNT: u32 = 3074;

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn rva(offset: u32) -> u32 {
    0x1000 + offset - 512
}

fn directory_base(plus: bool) -> usize {
    152 + if plus { 112 } else { 96 }
}

fn fixture(plus: bool) -> Vec<u8> {
    let mut bytes = vec![0; 1024];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&1_u16.to_le_bytes());
    let directory = directory_base(plus);
    bytes[148..150].copy_from_slice(&u16::try_from(directory + 128 - 152).unwrap().to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, directory - 4, 16);
    for (field, value) in [8, 12, 16, 20].into_iter().zip([512, 0x1000, 512, 512]) {
        put32(&mut bytes, directory + 128 + field, value);
    }
    for (slot, offset, size) in [
        (0, 672, 40),
        (1, 560, 40),
        (2, 512, 48),
        (5, 760, 12),
        (6, 896, 28),
        (9, 784, if plus { 40 } else { 24 }),
        (10, 960, 24),
        (13, 832, 64),
    ] {
        put32(&mut bytes, directory + slot * 8, rva(offset));
        put32(&mut bytes, directory + slot * 8 + 4, size);
    }
    put32(&mut bytes, directory + 32, 936);
    put32(&mut bytes, directory + 36, 16);
    for (offset, value) in [
        (512, 0x1234_5678),
        (516, 0x90ab_cdef),
        (520, 0xabcd_1234),
        (524, 0x0001_0001),
        (528, 0x8000_0020),
        (532, 0x8000_0000),
        (536, 7),
        (540, u32::MAX),
        (560, rva(616)),
        (572, rva(600)),
        (576, rva(640)),
        (684, rva(716)),
        (688, 1),
        (692, 1),
        (696, 1),
        (700, rva(728)),
        (704, rva(736)),
        (708, rva(742)),
        (728, 0x1500),
        (736, rva(744)),
        (760, 0x1000),
        (764, 12),
        (832, 1),
        (836, rva(600)),
        (848, rva(616)),
        (908, u32::MAX),
        (912, u32::MAX),
        (916, 0xffff_fffd),
        (920, 0xffff_fffe),
        (936, 8),
        (944, 8),
        (960, if plus { 112 } else { 64 }),
        (964, 0xaabb_ccdd),
        (972, 0x1122_3344),
        (976, 0x5566_7788),
        (980, u32::MAX),
    ] {
        put32(&mut bytes, offset, value);
    }
    bytes[600..606].copy_from_slice(b"x.dll\0");
    bytes[544..552].copy_from_slice(&[3, 0, 65, 0, 0, 0xd8, 0, 0]);
    bytes[716..722].copy_from_slice(b"e.dll\0");
    bytes[744..746].copy_from_slice(b"e\0");
    if plus {
        bytes[616..624].copy_from_slice(&0x8000_0000_0000_0007_u64.to_le_bytes());
    } else {
        put32(&mut bytes, 616, 0x8000_0007);
    }
    bytes
}

#[derive(Debug)]
struct Digest(u64);

impl Digest {
    fn bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
        }
    }
}

impl fmt::Write for Digest {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.bytes(text.as_bytes());
        Ok(())
    }
}

#[derive(Debug)]
struct Campaign {
    cases: u32,
    outcomes: [[u32; 2]; READER_COUNT],
    digest: Digest,
}

fn inspect(bytes: &[u8], baseline: bool, report: &mut Campaign) {
    let before = bytes.to_vec();
    write!(report.digest, "input:{}:{}\0", report.cases, bytes.len()).unwrap();
    report.digest.bytes(bytes);
    macro_rules! observe {
        ($index:expr, $reader:ident $(, $argument:expr)*) => {{
            let first = $reader(bytes $(, $argument)*);
            let second = $reader(bytes $(, $argument)*);
            assert_eq!(first, second, "{} at case {}", stringify!($reader), report.cases);
            if baseline {
                assert!(first.is_ok(), "{} baseline: {first:?}", stringify!($reader));
            }
            report.outcomes[$index][usize::from(first.is_err())] += 1;
            write!(report.digest, "\0{}:{first:?}\0", stringify!($reader)).unwrap();
        }};
    }
    observe!(0, parse_pe_header_prefix);
    observe!(1, parse_pe_headers);
    observe!(2, parse_pe_sections);
    observe!(
        3,
        resolve_pe_file_range,
        RelativeVirtualAddress::new(0x1000),
        1
    );
    observe!(4, parse_pe_import_descriptors);
    observe!(5, parse_pe_import_lookups);
    observe!(6, parse_pe_export_directory);
    observe!(7, parse_pe_export_addresses);
    observe!(8, parse_pe_export_names);
    observe!(9, parse_pe_base_relocation_blocks);
    observe!(10, parse_pe_tls_directory);
    observe!(11, parse_pe_delay_import_descriptors);
    observe!(12, parse_pe_delay_import_names);
    observe!(13, parse_pe_delay_import_lookups);
    observe!(14, parse_pe_certificate_table);
    observe!(15, parse_pe_certificate_entries);
    observe!(16, parse_pe_debug_directory);
    observe!(17, parse_pe_load_config_prefix);
    observe!(18, parse_pe_resource_root);
    observe!(19, parse_pe_resource_root_names);
    inspect_resource_graphs(bytes, baseline, report);
    assert_eq!(bytes, before, "input changed at case {}", report.cases);
    report.cases += 1;
}

fn inspect_resource_graphs(bytes: &[u8], baseline: bool, report: &mut Campaign) {
    let first = parse_pe_resource_directories(bytes);
    let second = parse_pe_resource_directories(bytes);
    assert_eq!(
        first, second,
        "resource directories at case {}",
        report.cases
    );
    if baseline {
        assert_eq!(
            first,
            Err(PeResourceDirectoryError::DirectoryOutsideResource {
                offset: 0x7fff_ffff,
                length: 16,
                directory_size: 48,
            })
        );
    }
    report.outcomes[20][usize::from(first.is_err())] += 1;
    write!(report.digest, "\0parse_pe_resource_directories:{first:?}\0").unwrap();
    let first = parse_pe_resource_data_entries(bytes);
    let second = parse_pe_resource_data_entries(bytes);
    assert_eq!(first, second, "resource data at case {}", report.cases);
    if baseline {
        assert_eq!(
            first,
            Err(PeResourceDataEntryError::Graph(
                PeResourceDirectoryError::DirectoryOutsideResource {
                    offset: 0x7fff_ffff,
                    length: 16,
                    directory_size: 48,
                }
            ))
        );
    }
    report.outcomes[21][usize::from(first.is_err())] += 1;
    write!(
        report.digest,
        "\0parse_pe_resource_data_entries:{first:?}\0"
    )
    .unwrap();
    let first = parse_pe_resource_directory_names(bytes);
    let second = parse_pe_resource_directory_names(bytes);
    assert_eq!(first, second, "directory names at case {}", report.cases);
    if baseline {
        assert_eq!(
            first,
            Err(PeResourceDirectoryNameError::Graph(
                PeResourceDirectoryError::DirectoryOutsideResource {
                    offset: 0x7fff_ffff,
                    length: 16,
                    directory_size: 48,
                }
            ))
        );
    }
    report.outcomes[22][usize::from(first.is_err())] += 1;
    write!(
        report.digest,
        "\0parse_pe_resource_directory_names:{first:?}\0"
    )
    .unwrap();
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

fn campaign() -> Campaign {
    let mut report = Campaign {
        cases: 0,
        outcomes: [[0; 2]; READER_COUNT],
        digest: Digest(0xcbf2_9ce4_8422_2325),
    };
    let mut state = 0x7233_4636;
    for plus in [false, true] {
        let original = fixture(plus);
        inspect(&original, true, &mut report);
        for end in 0..original.len() {
            inspect(&original[..end], false, &mut report);
        }
        for _ in 0..128 {
            let mut bytes = original.clone();
            let position = index(&mut state, bytes.len());
            bytes[position] ^= 1 << (next(&mut state) % 8);
            inspect(&bytes, false, &mut report);
        }
        let directory = directory_base(plus);
        let mut words = vec![60, 148, 212, directory - 4];
        words.extend((0..32).map(|i| directory + i * 4));
        words.extend([560, 616, 692, 696, 760, 764, 832, 848, 896, 912, 936, 944]);
        let boundaries = [
            0,
            1,
            7,
            8,
            27,
            28,
            255,
            256,
            257,
            511,
            512,
            1024,
            0xffff_fffc,
            u32::MAX,
        ];
        for round in 0..128 {
            let mut bytes = original.clone();
            let position = words[index(&mut state, words.len())];
            put32(&mut bytes, position, boundaries[round % boundaries.len()]);
            inspect(&bytes, false, &mut report);
        }
        for round in 0..256 {
            let mut bytes = original.clone();
            let changes = 1 + next(&mut state) % 8;
            for _ in 0..changes {
                let position = index(&mut state, bytes.len());
                bytes[position] = next(&mut state).to_le_bytes()[0];
            }
            if round % 4 == 0 {
                bytes.truncate(index(&mut state, original.len()));
            }
            inspect(&bytes, false, &mut report);
        }
    }
    assert_eq!(report.cases, CASE_COUNT);
    for [successes, failures] in report.outcomes {
        assert!(successes > 0 && failures > 0);
        assert_eq!(successes + failures, CASE_COUNT);
    }
    report
}

#[test]
fn deterministic_mutations_keep_every_reader_repeatable_and_input_unchanged() {
    campaign();
}
