use super::support_pe32_sort as sort;

pub use sort::{
    ACTUAL_BASE, CANARY_RVA, CHECKSUM_RVA, DATA_RAW, DATA_RVA, EntryGroup, FIXUP_RAW, FIXUP_RVA,
    Fixup, GATE_BASE, GATE_OFFSETS, IAT_ALLOC_RVA, IAT_EXIT_RVA, IAT_FREE_RVA, IMAGE_SIZE,
    PREFERRED_BASE, STACK_BASE, TEXT_RAW, TEXT_RVA,
};

pub const SEED: u32 = 0xdead_beef;
pub const COUNTS: [u8; 5] = [0, 1, 2, 15, 16];
pub const RECORD_RVA: u32 = DATA_RVA + 0x20;
pub const RECORD_BYTES: usize = 128;
pub const KERNEL_OFFSET: u32 = 0x200;
pub const KERNEL: &[u8] =
    include_bytes!("../../programs/records/fixtures/p2-compiled-records/kernel.text.bin");
pub const KERNEL_SHA256: &str = "8bb4f64cef988239a8042519e089027aa03cb938dcd8cc99de18124470cef5be";
pub const RECORD_SHA256: &str = "71c5b89591bf15c6b81fae547ca83764e2f6d9968449142b791ddd088491199d";
pub const MAIN_ENTRIES: [u32; 5] = [0, 20, 47, 60, 82];
pub const KERNEL_ENTRIES: [u32; 4] = [0x200, 0x210, 0x216, 0x245];

const MAIN: [u8; 106] = [
    0x6a, 0x04, 0x68, 0x00, 0x30, 0x00, 0x00, 0x68, 0x80, 0x00, 0x00, 0x00, 0x6a, 0x00, 0xff, 0x15,
    0x54, 0x31, 0x40, 0x00, 0x89, 0xc7, 0xbe, 0x20, 0x30, 0x40, 0x00, 0x89, 0xc2, 0xb9, 0x20, 0x00,
    0x00, 0x00, 0x8b, 0x06, 0x89, 0x02, 0x83, 0xc6, 0x04, 0x83, 0xc2, 0x04, 0x49, 0x75, 0xf3, 0x68,
    0xef, 0xbe, 0xad, 0xde, 0x6a, 0x10, 0x57, 0xe8, 0xc4, 0x01, 0x00, 0x00, 0x83, 0xc4, 0x0c, 0xa3,
    0x00, 0x30, 0x40, 0x00, 0x68, 0x00, 0x80, 0x00, 0x00, 0x6a, 0x00, 0x57, 0xff, 0x15, 0x50, 0x31,
    0x40, 0x00, 0xff, 0x35, 0x00, 0x30, 0x40, 0x00, 0xff, 0x15, 0x58, 0x31, 0x40, 0x00, 0xc7, 0x05,
    0x0c, 0x30, 0x40, 0x00, 0xa5, 0xa5, 0xa5, 0xa5, 0x0f, 0x0b,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub signed_byte: i8,
    pub tag: u8,
    pub signed_word: i16,
    pub weight: u16,
    pub marker: u16,
}

pub const RECORDS: [Record; 16] = [
    Record {
        signed_byte: -128,
        tag: 0,
        signed_word: -32768,
        weight: 65535,
        marker: 0xa001,
    },
    Record {
        signed_byte: 127,
        tag: 1,
        signed_word: 32767,
        weight: 65535,
        marker: 0xa002,
    },
    Record {
        signed_byte: -1,
        tag: 2,
        signed_word: 32767,
        weight: 0,
        marker: 0xa003,
    },
    Record {
        signed_byte: 1,
        tag: 3,
        signed_word: -32768,
        weight: 1,
        marker: 0xa004,
    },
    Record {
        signed_byte: 0,
        tag: 255,
        signed_word: -1,
        weight: 32768,
        marker: 0xa005,
    },
    Record {
        signed_byte: 127,
        tag: 254,
        signed_word: -32768,
        weight: 1,
        marker: 0xa006,
    },
    Record {
        signed_byte: -128,
        tag: 127,
        signed_word: 32767,
        weight: 65535,
        marker: 0xa007,
    },
    Record {
        signed_byte: 1,
        tag: 128,
        signed_word: 1,
        weight: 32767,
        marker: 0xa008,
    },
    Record {
        signed_byte: -127,
        tag: 5,
        signed_word: -32767,
        weight: 2,
        marker: 0xa009,
    },
    Record {
        signed_byte: 126,
        tag: 6,
        signed_word: 32766,
        weight: 65534,
        marker: 0xa00a,
    },
    Record {
        signed_byte: -2,
        tag: 7,
        signed_word: -2,
        weight: 3,
        marker: 0xa00b,
    },
    Record {
        signed_byte: 2,
        tag: 8,
        signed_word: 2,
        weight: 32768,
        marker: 0xa00c,
    },
    Record {
        signed_byte: -64,
        tag: 9,
        signed_word: 0,
        weight: 65535,
        marker: 0xa00d,
    },
    Record {
        signed_byte: 64,
        tag: 10,
        signed_word: -1,
        weight: 0,
        marker: 0xa00e,
    },
    Record {
        signed_byte: -1,
        tag: 11,
        signed_word: 1,
        weight: 32769,
        marker: 0xa00f,
    },
    Record {
        signed_byte: 1,
        tag: 12,
        signed_word: -1,
        weight: 32767,
        marker: 0xa010,
    },
];

pub fn record_bytes() -> [u8; RECORD_BYTES] {
    let mut bytes = [0; RECORD_BYTES];
    for (record, output) in RECORDS.iter().zip(bytes.chunks_exact_mut(8)) {
        output[0] = record.signed_byte as u8;
        output[1] = record.tag;
        output[2..4].copy_from_slice(&record.signed_word.to_le_bytes());
        output[4..6].copy_from_slice(&record.weight.to_le_bytes());
        output[6..8].copy_from_slice(&record.marker.to_le_bytes());
    }
    bytes
}

pub fn expected_checksum(count: u8) -> u32 {
    match count {
        0 => 0xdead_beef,
        1 => 0xd5f7_ddfb,
        2 => 0xbec5_c0fa,
        15 => 0xc9d7_87cb,
        16 => 0x3af6_f96c,
        _ => panic!("undeclared record count"),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fixture {
    pub image: Vec<u8>,
    pub program: Vec<u8>,
    pub fixups: Vec<Fixup>,
    pub groups: Vec<EntryGroup>,
    pub count: u8,
}

pub fn fixture(count: u8) -> Fixture {
    assert!(COUNTS.contains(&count));
    assert_eq!(KERNEL.len(), 74);
    let mut program = vec![0xcc; KERNEL_OFFSET as usize + KERNEL.len()];
    program[..MAIN.len()].copy_from_slice(&MAIN);
    program[53] = count;
    program[KERNEL_OFFSET as usize..].copy_from_slice(KERNEL);
    let fixups = [
        (16, IAT_ALLOC_RVA),
        (23, RECORD_RVA),
        (64, CHECKSUM_RVA),
        (78, IAT_FREE_RVA),
        (84, CHECKSUM_RVA),
        (90, IAT_EXIT_RVA),
        (96, CANARY_RVA),
    ]
    .into_iter()
    .map(|(offset, target_rva)| Fixup { offset, target_rva })
    .collect::<Vec<_>>();
    let groups = [
        EntryGroup {
            name: "main",
            offsets: MAIN_ENTRIES.to_vec(),
        },
        EntryGroup {
            name: "kernel",
            offsets: KERNEL_ENTRIES.to_vec(),
        },
    ]
    .to_vec();
    let mut image = sort::image();
    image[TEXT_RAW..DATA_RAW].fill(0xcc);
    image[TEXT_RAW..TEXT_RAW + program.len()].copy_from_slice(&program);
    image[DATA_RAW..DATA_RAW + 0x20].fill(0);
    image[DATA_RAW + 0x20..DATA_RAW + 0x20 + RECORD_BYTES].copy_from_slice(&record_bytes());
    image[0x180..0x184].copy_from_slice(&(program.len() as u32).to_le_bytes());
    image[FIXUP_RAW..FIXUP_RAW + 24].fill(0);
    image[FIXUP_RAW..FIXUP_RAW + 4].copy_from_slice(&TEXT_RVA.to_le_bytes());
    image[FIXUP_RAW + 4..FIXUP_RAW + 8].copy_from_slice(&24_u32.to_le_bytes());
    for (index, fixup) in fixups.iter().enumerate() {
        let start = FIXUP_RAW + 8 + index * 2;
        image[start..start + 2].copy_from_slice(&(0x3000 | fixup.offset as u16).to_le_bytes());
    }
    Fixture {
        image,
        program,
        fixups,
        groups,
        count,
    }
}

impl Fixture {
    pub fn manifest_json(&self) -> String {
        use std::fmt::Write;
        let mut json = format!(
            "{{\"preferred_base\":{PREFERRED_BASE},\"actual_base\":{ACTUAL_BASE},\"image_size\":{IMAGE_SIZE},\"text_rva\":{TEXT_RVA},\"data_rva\":{DATA_RVA},\"fixup_rva\":{FIXUP_RVA},\"text_raw\":{TEXT_RAW},\"data_raw\":{DATA_RAW},\"fixup_raw\":{FIXUP_RAW},\"gate_base\":{GATE_BASE},\"stack_base\":{STACK_BASE},\"count\":{},\"seed\":{SEED},\"record_rva\":{RECORD_RVA},\"record_bytes\":{RECORD_BYTES},\"record_sha256\":\"{RECORD_SHA256}\",\"checksum_rva\":{CHECKSUM_RVA},\"canary_rva\":{CANARY_RVA},\"allocation_bytes\":128,\"cdecl_arguments\":3,\"expected_checksum\":{},\"kernel_offset\":{KERNEL_OFFSET},\"kernel_end_offset\":{},\"kernel_sha256\":\"{KERNEL_SHA256}\",\"kernel_bytes\":{},\"program_hex\":\"",
            self.count,
            expected_checksum(self.count),
            KERNEL_OFFSET as usize + KERNEL.len(),
            KERNEL.len()
        );
        for byte in &self.program {
            write!(&mut json, "{byte:02x}").unwrap();
        }
        json.push_str("\",\"kernel_hex\":\"");
        for byte in KERNEL {
            write!(&mut json, "{byte:02x}").unwrap();
        }
        json.push_str("\",\"record_hex\":\"");
        for byte in record_bytes() {
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
            let instruction_count = if group.name == "main" { 27 } else { 31 };
            write!(&mut json, "{{\"name\":\"{}\",\"instruction_count\":{instruction_count},\"block_count\":{},\"offsets\":[", group.name, group.offsets.len()).unwrap();
            for (ordinal, offset) in group.offsets.iter().enumerate() {
                if ordinal != 0 {
                    json.push(',');
                }
                write!(&mut json, "{offset}").unwrap();
            }
            json.push_str("],\"entry_rvas\":[");
            for (ordinal, offset) in group.offsets.iter().enumerate() {
                if ordinal != 0 {
                    json.push(',');
                }
                write!(&mut json, "{}", TEXT_RVA + offset).unwrap();
            }
            json.push_str("]}");
        }
        json.push_str("],\"imports\":[");
        for (index, (name, id, rva, offset)) in [
            ("ExitProcess", 0x10003_u32, IAT_EXIT_RVA, GATE_OFFSETS[0]),
            ("VirtualAlloc", 0x10005, IAT_ALLOC_RVA, GATE_OFFSETS[1]),
            ("VirtualFree", 0x10006, IAT_FREE_RVA, GATE_OFFSETS[2]),
        ]
        .into_iter()
        .enumerate()
        {
            if index != 0 {
                json.push(',');
            }
            write!(
                &mut json,
                "{{\"name\":\"{name}\",\"id\":{id},\"iat_rva\":{rva},\"gate_offset\":{offset}}}"
            )
            .unwrap();
        }
        json.push_str("],\"milestones\":{\"alloc_return\":20,\"copy_start\":34,\"copy_complete\":47,\"leaf_entry\":512,\"leaf_call\":55,\"leaf_return\":60,\"caller_cleanup\":60,\"free_call\":76,\"free_return\":82,\"exit_call\":88,\"exit_return\":94},\"helper_imports\":[\"guard_resident\",\"read8\",\"read16\",\"read32\",\"store_resident32\"]}");
        json
    }
}
