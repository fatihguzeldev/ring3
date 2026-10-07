#[path = "pe32.rs"]
#[allow(dead_code)]
mod pe;

pub const PREFERRED_BASE: u32 = 0x0040_0000;
pub const ACTUAL_BASE: u32 = 0x0050_0000;
pub const IMAGE_SIZE: u32 = 0x6000;
pub const TEXT_RVA: u32 = 0x1000;
pub const DATA_RVA: u32 = 0x3000;
pub const FIXUP_RVA: u32 = 0x5000;
pub const TEXT_RAW: usize = 0x200;
pub const DATA_RAW: usize = 0x600;
pub const FIXUP_RAW: usize = 0x800;
pub const GATE_BASE: u32 = 0x8000;
pub const GATE_OFFSETS: [u32; 3] = [32, 64, 80];
pub const STACK_BASE: u32 = 0x70000;
pub const WORDS: u32 = 256;
pub const BATCHES: u32 = 10;
pub const SEED: u32 = 0x6d2b_79f5;
pub const CHECKSUM_RVA: u32 = DATA_RVA;
pub const AGGREGATE_RVA: u32 = DATA_RVA + 4;
pub const COUNTER_RVA: u32 = DATA_RVA + 8;
pub const CANARY_RVA: u32 = DATA_RVA + 12;
pub const IAT_FREE_RVA: u32 = 0x3150;
pub const IAT_ALLOC_RVA: u32 = 0x3154;
pub const IAT_EXIT_RVA: u32 = 0x3158;
pub const FILL_OFFSET: u32 = 0x100;
pub const SORT_OFFSET: u32 = 0x180;
pub const CHECKSUM_OFFSET: u32 = 0x200;
pub const MAIN_ENTRIES: [u32; 7] = [0, 20, 27, 32, 37, 62, 74];
pub const FILL_ENTRIES: [u32; 2] = [FILL_OFFSET, FILL_OFFSET + 41];
pub const SORT_ENTRIES: [u32; 5] = [
    SORT_OFFSET,
    SORT_OFFSET + 14,
    SORT_OFFSET + 21,
    SORT_OFFSET + 28,
    SORT_OFFSET + 39,
];
pub const CHECKSUM_ENTRIES: [u32; 2] = [CHECKSUM_OFFSET, CHECKSUM_OFFSET + 20];

// absolute operands are preferred-base values; only these eight words relocate.
const MAIN: [u8; 98] = [
    0x6a, 0x04, 0x68, 0x00, 0x30, 0x00, 0x00, 0x68, 0x00, 0x04, 0x00, 0x00, 0x6a, 0x00, 0xff, 0x15,
    0x54, 0x31, 0x40, 0x00, 0x89, 0xc7, 0xe8, 0xe5, 0x00, 0x00, 0x00, 0xe8, 0x60, 0x01, 0x00, 0x00,
    0xe8, 0xdb, 0x01, 0x00, 0x00, 0xa3, 0x00, 0x30, 0x40, 0x00, 0x01, 0x05, 0x04, 0x30, 0x40, 0x00,
    0x68, 0x00, 0x80, 0x00, 0x00, 0x6a, 0x00, 0x57, 0xff, 0x15, 0x50, 0x31, 0x40, 0x00, 0xff, 0x0d,
    0x08, 0x30, 0x40, 0x00, 0x0f, 0x85, 0xb6, 0xff, 0xff, 0xff, 0xff, 0x35, 0x04, 0x30, 0x40, 0x00,
    0xff, 0x15, 0x58, 0x31, 0x40, 0x00, 0xc7, 0x05, 0x0c, 0x30, 0x40, 0x00, 0xa5, 0xa5, 0xa5, 0xa5,
    0x0f, 0x0b,
];

// edi remains the allocation base across all three guest functions.
const FILL: [u8; 42] = [
    0xb8, 0xf5, 0x79, 0x2b, 0x6d, 0xb9, 0x00, 0x01, 0x00, 0x00, 0x89, 0xfe, 0x89, 0xc2, 0xc1, 0xe2,
    0x0d, 0x31, 0xd0, 0x89, 0xc2, 0xc1, 0xea, 0x11, 0x31, 0xd0, 0x89, 0xc2, 0xc1, 0xe2, 0x05, 0x31,
    0xd0, 0x89, 0x06, 0x83, 0xc6, 0x04, 0x49, 0x75, 0xe3, 0xc3,
];

// jbe tests unsigned order; esi cannot move below edi before the previous-word load.
const SORT: [u8; 40] = [
    0xb9, 0x01, 0x00, 0x00, 0x00, 0x8d, 0x34, 0x8f, 0x8b, 0x06, 0x39, 0xfe, 0x76, 0x0e, 0x8b, 0x5e,
    0xfc, 0x39, 0xc3, 0x76, 0x07, 0x89, 0x1e, 0x83, 0xee, 0x04, 0xeb, 0xee, 0x89, 0x06, 0x41, 0x81,
    0xf9, 0x00, 0x01, 0x00, 0x00, 0x72, 0xde, 0xc3,
];

const CHECKSUM: [u8; 21] = [
    0x31, 0xc0, 0xb9, 0x00, 0x01, 0x00, 0x00, 0x89, 0xfe, 0xc1, 0xc0, 0x05, 0x33, 0x06, 0x83, 0xc6,
    0x04, 0x49, 0x75, 0xf5, 0xc3,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fixup {
    pub offset: u32,
    pub target_rva: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryGroup {
    pub name: &'static str,
    pub offsets: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fixture {
    pub image: Vec<u8>,
    pub program: Vec<u8>,
    pub fixups: Vec<Fixup>,
    pub groups: Vec<EntryGroup>,
}

impl Fixture {
    pub fn manifest_json(&self) -> String {
        use std::fmt::Write;

        let mut json = format!(
            "{{\"preferred_base\":{PREFERRED_BASE},\"actual_base\":{ACTUAL_BASE},\"image_size\":{IMAGE_SIZE},\"text_rva\":{TEXT_RVA},\"data_rva\":{DATA_RVA},\"fixup_rva\":{FIXUP_RVA},\"text_raw\":{TEXT_RAW},\"data_raw\":{DATA_RAW},\"fixup_raw\":{FIXUP_RAW},\"gate_base\":{GATE_BASE},\"stack_base\":{STACK_BASE},\"words\":{WORDS},\"batches\":{BATCHES},\"seed\":{SEED},\"checksum_rva\":{CHECKSUM_RVA},\"aggregate_rva\":{AGGREGATE_RVA},\"counter_rva\":{COUNTER_RVA},\"canary_rva\":{CANARY_RVA},\"program_hex\":\""
        );
        for byte in &self.program {
            write!(&mut json, "{byte:02x}").unwrap();
        }
        json.push_str("\",\"fixups\":[");
        for (index, fixup) in self.fixups.iter().enumerate() {
            if index != 0 {
                json.push(',');
            }
            write!(
                &mut json,
                "{{\"offset\":{},\"target_rva\":{}}}",
                fixup.offset, fixup.target_rva
            )
            .unwrap();
        }
        json.push_str("],\"groups\":[");
        for (index, group) in self.groups.iter().enumerate() {
            if index != 0 {
                json.push(',');
            }
            write!(&mut json, "{{\"name\":\"{}\",\"offsets\":[", group.name).unwrap();
            for (ordinal, offset) in group.offsets.iter().enumerate() {
                if ordinal != 0 {
                    json.push(',');
                }
                write!(&mut json, "{offset}").unwrap();
            }
            json.push_str("]}");
        }
        json.push_str("],\"imports\":[");
        for (index, (name, rva, offset)) in [
            ("ExitProcess", IAT_EXIT_RVA, GATE_OFFSETS[0]),
            ("VirtualAlloc", IAT_ALLOC_RVA, GATE_OFFSETS[1]),
            ("VirtualFree", IAT_FREE_RVA, GATE_OFFSETS[2]),
        ]
        .into_iter()
        .enumerate()
        {
            if index != 0 {
                json.push(',');
            }
            write!(
                &mut json,
                "{{\"name\":\"{name}\",\"iat_rva\":{rva},\"gate_offset\":{offset}}}"
            )
            .unwrap();
        }
        json.push_str("]}");
        json
    }
}

pub fn fixture() -> Fixture {
    let mut program = vec![0xcc; CHECKSUM_OFFSET as usize + CHECKSUM.len()];
    for (offset, bytes) in [
        (0, MAIN.as_slice()),
        (FILL_OFFSET as usize, FILL.as_slice()),
        (SORT_OFFSET as usize, SORT.as_slice()),
        (CHECKSUM_OFFSET as usize, CHECKSUM.as_slice()),
    ] {
        program[offset..offset + bytes.len()].copy_from_slice(bytes);
    }
    let fixups = [
        (16, IAT_ALLOC_RVA),
        (38, CHECKSUM_RVA),
        (44, AGGREGATE_RVA),
        (58, IAT_FREE_RVA),
        (64, COUNTER_RVA),
        (76, AGGREGATE_RVA),
        (82, IAT_EXIT_RVA),
        (88, CANARY_RVA),
    ]
    .into_iter()
    .map(|(offset, target_rva)| Fixup { offset, target_rva })
    .collect();
    let groups = [
        ("main", MAIN_ENTRIES.as_slice()),
        ("fill", FILL_ENTRIES.as_slice()),
        ("sort", SORT_ENTRIES.as_slice()),
        ("checksum", CHECKSUM_ENTRIES.as_slice()),
    ]
    .into_iter()
    .map(|(name, offsets)| EntryGroup {
        name,
        offsets: offsets.to_vec(),
    })
    .collect();
    let image = authored_image(&program);
    Fixture {
        image,
        program,
        fixups,
        groups,
    }
}

pub fn image() -> Vec<u8> {
    fixture().image
}

fn authored_image(program: &[u8]) -> Vec<u8> {
    let mut bytes = pe::image();
    bytes.resize(0xa00, 0);
    bytes[TEXT_RAW..].fill(0);
    pe::put16(&mut bytes, pe::COFF + 18, 0x0102);
    pe::put32(&mut bytes, pe::OPTIONAL + 4, 1024);
    pe::put32(&mut bytes, pe::OPTIONAL + 8, 1024);
    pe::put32(&mut bytes, pe::OPTIONAL + 12, 0);
    pe::put32(&mut bytes, pe::OPTIONAL + 56, IMAGE_SIZE);
    for (index, rva, size) in [(1, 0x3100, 40), (5, FIXUP_RVA, 24), (12, IAT_FREE_RVA, 16)] {
        pe::put32(&mut bytes, pe::OPTIONAL + 96 + index * 8, rva);
        pe::put32(&mut bytes, pe::OPTIONAL + 100 + index * 8, size);
    }
    pe::put32(&mut bytes, pe::section(0) + 8, program.len() as u32);
    pe::put32(&mut bytes, pe::section(0) + 16, 1024);
    bytes[TEXT_RAW..DATA_RAW].fill(0xcc);
    bytes[TEXT_RAW..TEXT_RAW + program.len()].copy_from_slice(program);
    pe::put32(&mut bytes, pe::section(1) + 8, 512);
    pe::put32(&mut bytes, pe::section(1) + 20, DATA_RAW as u32);
    pe::put32(&mut bytes, DATA_RAW + 8, BATCHES);
    for (index, value) in [0x3140, 0, 0, 0x3160, IAT_FREE_RVA].into_iter().enumerate() {
        pe::put32(&mut bytes, DATA_RAW + 0x100 + index * 4, value);
    }
    for table in [DATA_RAW + 0x140, DATA_RAW + 0x150] {
        for (index, name) in [0x3190, 0x31b0, 0x3170].into_iter().enumerate() {
            pe::put32(&mut bytes, table + index * 4, name);
        }
    }
    bytes[DATA_RAW + 0x160..DATA_RAW + 0x16d].copy_from_slice(b"KeRnEl32.dLl\0");
    for (offset, hint, symbol) in [
        (0x170, 0x1234, b"ExitProcess\0".as_slice()),
        (0x190, 0x5678, b"VirtualFree\0".as_slice()),
        (0x1b0, 0x9abc, b"VirtualAlloc\0".as_slice()),
    ] {
        pe::put16(&mut bytes, DATA_RAW + offset, hint);
        bytes[DATA_RAW + offset + 2..DATA_RAW + offset + 2 + symbol.len()].copy_from_slice(symbol);
    }
    bytes[pe::section(2)..pe::section(2) + 8].copy_from_slice(b".fixups\0");
    pe::put32(&mut bytes, pe::section(2) + 8, 24);
    pe::put32(&mut bytes, pe::section(2) + 16, 512);
    pe::put32(&mut bytes, pe::section(2) + 20, FIXUP_RAW as u32);
    pe::put32(&mut bytes, pe::section(2) + 36, 0x4000_0040);
    pe::put32(&mut bytes, FIXUP_RAW, TEXT_RVA);
    pe::put32(&mut bytes, FIXUP_RAW + 4, 24);
    for (index, offset) in [16, 38, 44, 58, 64, 76, 82, 88].into_iter().enumerate() {
        pe::put16(&mut bytes, FIXUP_RAW + 8 + index * 2, 0x3000 | offset);
    }
    bytes
}
