use super::support_pe32_compiled_records as readonly;

pub use readonly::{
    ACTUAL_BASE, CANARY_RVA, COUNTS, DATA_RAW, DATA_RVA, EntryGroup, FIXUP_RAW, FIXUP_RVA, Fixup,
    GATE_BASE, GATE_OFFSETS, IAT_ALLOC_RVA, IAT_EXIT_RVA, IAT_FREE_RVA, IMAGE_SIZE, PREFERRED_BASE,
    RECORD_BYTES, RECORD_RVA, RECORD_SHA256, SEED, STACK_BASE, TEXT_RAW, TEXT_RVA, record_bytes,
};

pub const CHECKSUM1_RVA: u32 = DATA_RVA;
pub const CHECKSUM2_RVA: u32 = DATA_RVA + 4;
pub const KERNEL_OFFSET: u32 = 0x200;
pub const KERNEL: &[u8] = include_bytes!("../fixtures/p2-mutable-records/kernel.text.bin");
pub const KERNEL_SHA256: &str = "600b2c46774458e4e3b1b96f032270388195c0afa65b563c9c63731623e48c5a";
pub const MAIN_ENTRIES: [u32; 6] = [0, 20, 47, 60, 77, 99];
pub const KERNEL_ENTRIES: [u32; 4] = [0x200, 0x210, 0x212, 0x260];
pub const LEAF_CALLS: [u32; 2] = [55, 72];
pub const LEAF_RETURNS: [u32; 2] = [60, 77];
pub const FAULT_STORE_OFFSET: u32 = 0x22e;

// both calls use the same allocation; returned eax becomes the second seed.
const MAIN: [u8; 123] = [
    0x6a, 0x04, 0x68, 0x00, 0x30, 0x00, 0x00, 0x68, 0x80, 0x00, 0x00, 0x00, 0x6a, 0x00, 0xff, 0x15,
    0x54, 0x31, 0x40, 0x00, 0x89, 0xc7, 0xbe, 0x20, 0x30, 0x40, 0x00, 0x89, 0xc2, 0xb9, 0x20, 0x00,
    0x00, 0x00, 0x8b, 0x06, 0x89, 0x02, 0x83, 0xc6, 0x04, 0x83, 0xc2, 0x04, 0x49, 0x75, 0xf3, 0x68,
    0xef, 0xbe, 0xad, 0xde, 0x6a, 0x10, 0x57, 0xe8, 0xc4, 0x01, 0x00, 0x00, 0x83, 0xc4, 0x0c, 0xa3,
    0x00, 0x30, 0x40, 0x00, 0x50, 0x6a, 0x10, 0x57, 0xe8, 0xb3, 0x01, 0x00, 0x00, 0x83, 0xc4, 0x0c,
    0xa3, 0x04, 0x30, 0x40, 0x00, 0x68, 0x00, 0x80, 0x00, 0x00, 0x6a, 0x00, 0x57, 0xff, 0x15, 0x50,
    0x31, 0x40, 0x00, 0xff, 0x35, 0x04, 0x30, 0x40, 0x00, 0xff, 0x15, 0x58, 0x31, 0x40, 0x00, 0xc7,
    0x05, 0x0c, 0x30, 0x40, 0x00, 0xa5, 0xa5, 0xa5, 0xa5, 0x0f, 0x0b,
];

pub fn expected_checksums(count: u8) -> [u32; 2] {
    match count {
        0 => [0xdeadbeef, 0xdeadbeef],
        1 => [0xd6c9676d, 0xd999551c],
        2 => [0xd9b234a4, 0xd1f058a0],
        15 => [0x5b99a2d9, 0x2ef1e62c],
        16 => [0x8cf0b199, 0x4438b326],
        _ => panic!("undeclared record count"),
    }
}

pub fn expected_record_hex(count: u8, pass: usize) -> &'static str {
    assert!(pass < 2);
    match count {
        0 => [
            "80000080ffff01a07f01ff7fffff02a0ff02ff7f000003a001030080010004a000ffffff008005a07ffe0080010006a0807fff7fffff07a001800100ff7f08a081050180020009a07e06fe7ffeff0aa0fe07feff03000ba00208020000800ca0c0090000ffff0da0400affff00000ea0ff0b010001800fa0010cffffff7f10a0",
            "80000080ffff01a07f01ff7fffff02a0ff02ff7f000003a001030080010004a000ffffff008005a07ffe0080010006a0807fff7fffff07a001800100ff7f08a081050180020009a07e06fe7ffeff0aa0fe07feff03000ba00208020000800ca0c0090000ffff0da0400affff00000ea0ff0b010001800fa0010cffffff7f10a0",
        ][pass],
        1 => [
            "80fb0080b6d501a07f01ff7fffff02a0ff02ff7f000003a001030080010004a000ffffff008005a07ffe0080010006a0807fff7fffff07a001800100ff7f08a081050180020009a07e06fe7ffeff0aa0fe07feff03000ba00208020000800ca0c0090000ffff0da0400affff00000ea0ff0b010001800fa0010cffffff7f10a0",
            "80410080e2ae01a07f01ff7fffff02a0ff02ff7f000003a001030080010004a000ffffff008005a07ffe0080010006a0807fff7fffff07a001800100ff7f08a081050180020009a07e06fe7ffeff0aa0fe07feff03000ba00208020000800ca0c0090000ffff0da0400affff00000ea0ff0b010001800fa0010cffffff7f10a0",
        ][pass],
        2 => [
            "80fb0080b6d501a07fbbff7f2bd902a0ff02ff7f000003a001030080010004a000ffffff008005a07ffe0080010006a0807fff7fffff07a001800100ff7f08a081050180020009a07e06fe7ffeff0aa0fe07feff03000ba00208020000800ca0c0090000ffff0da0400affff00000ea0ff0b010001800fa0010cffffff7f10a0",
            "80600080fc0b01a07fddff7f8daa02a0ff02ff7f000003a001030080010004a000ffffff008005a07ffe0080010006a0807fff7fffff07a001800100ff7f08a081050180020009a07e06fe7ffeff0aa0fe07feff03000ba00208020000800ca0c0090000ffff0da0400affff00000ea0ff0b010001800fa0010cffffff7f10a0",
        ][pass],
        15 => [
            "80fb0080b6d501a07fbbff7f2bd902a0ff99ff7f463603a0011a0080313b04a00098ffffc2e605a07f12008065e906a080a2ff7fb22607a001a401007e5108a081bf0180803609a07e60fe7f4fcd0aa0fe5efeffb9bb0ba0027f02006dff0ca0c0c6000093fd0da04035ffff06cb0ea0ffad010080240fa0010cffffff7f10a0",
            "80d00080ea4801a07fd5ff7f1e4f02a0ff37ff7f0bf803a0019d0080350904a0009effffe22605a07f5a008081f006a080e2ff7fd50c07a001f801005f0f08a0810801807ff409a07e77fe7f29960aa0fe07feff11e10ba002fb02004aab0ca0c0130000408e0da04087ffff08e20ea0fff0010023530fa0010cffffff7f10a0",
        ][pass],
        16 => [
            "80fb0080b6d501a07fbbff7f2bd902a0ff99ff7f463603a0011a0080313b04a00098ffffc2e605a07f12008065e906a080a2ff7fb22607a001a401007e5108a081bf0180803609a07e60fe7f4fcd0aa0fe5efeffb9bb0ba0027f02006dff0ca0c0c6000093fd0da04035ffff06cb0ea0ffad010080240fa00127ffff33f310a0",
            "80ca0080cc7301a07f88ff7f5daf02a0ff03ff7f231003a0019e00807fff04a00040ffff388405a07f0100804a9c06a0800bff7f80cb07a0018f010073b408a081530180c0a109a07ecdfe7fc7430aa0fed0feff4f8b0ba00225020057000ca0c0e60000d71a0da04076ffff9d760ea0ff980100429e0fa001a8ffff473710a0",
        ][pass],
        _ => panic!("undeclared record count"),
    }
}

pub fn expected_records(count: u8, pass: usize) -> [u8; RECORD_BYTES] {
    let text = expected_record_hex(count, pass);
    std::array::from_fn(|index| u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).unwrap())
}

pub fn leaf_retired(count: u8) -> u32 {
    if count == 0 {
        13
    } else {
        14 + 26 * u32::from(count)
    }
}

pub fn expected_retired(count: u8) -> u32 {
    219 + 2 * leaf_retired(count)
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
    assert_eq!(KERNEL.len(), 101);
    let mut program = vec![0xcc; KERNEL_OFFSET as usize + KERNEL.len()];
    program[..MAIN.len()].copy_from_slice(&MAIN);
    program[53] = count;
    program[70] = count;
    program[KERNEL_OFFSET as usize..].copy_from_slice(KERNEL);
    let fixups = [
        (16, IAT_ALLOC_RVA),
        (23, RECORD_RVA),
        (64, CHECKSUM1_RVA),
        (81, CHECKSUM2_RVA),
        (95, IAT_FREE_RVA),
        (101, CHECKSUM2_RVA),
        (107, IAT_EXIT_RVA),
        (113, CANARY_RVA),
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
    let mut image = readonly::fixture(0).image;
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
        let [checksum1, checksum2] = expected_checksums(self.count);
        let mut json = format!(
            "{{\"preferred_base\":{PREFERRED_BASE},\"actual_base\":{ACTUAL_BASE},\"image_size\":{IMAGE_SIZE},\"text_rva\":{TEXT_RVA},\"data_rva\":{DATA_RVA},\"fixup_rva\":{FIXUP_RVA},\"text_raw\":{TEXT_RAW},\"data_raw\":{DATA_RAW},\"fixup_raw\":{FIXUP_RAW},\"gate_base\":{GATE_BASE},\"stack_base\":{STACK_BASE},\"count\":{},\"seed\":{SEED},\"record_rva\":{RECORD_RVA},\"record_bytes\":{RECORD_BYTES},\"record_sha256\":\"{RECORD_SHA256}\",\"checksum1_rva\":{CHECKSUM1_RVA},\"checksum2_rva\":{CHECKSUM2_RVA},\"canary_rva\":{CANARY_RVA},\"allocation_bytes\":128,\"cdecl_arguments\":3,\"expected_checksum1\":{checksum1},\"expected_checksum2\":{checksum2},\"expected_records1_hex\":\"{}\",\"expected_records2_hex\":\"{}\",\"expected_retired\":{},\"leaf_retired\":{},\"kernel_offset\":{KERNEL_OFFSET},\"kernel_end_offset\":{},\"kernel_sha256\":\"{KERNEL_SHA256}\",\"kernel_bytes\":{},\"program_hex\":\"",
            self.count,
            expected_record_hex(self.count, 0),
            expected_record_hex(self.count, 1),
            expected_retired(self.count),
            leaf_retired(self.count),
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
            let instruction_count = if group.name == "main" { 33 } else { 40 };
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
        json.push_str("],\"milestones\":{\"alloc_return\":20,\"copy_start\":34,\"copy_complete\":47,\"leaf_entry\":512,\"leaf_calls\":[55,72],\"leaf_returns\":[60,77],\"caller_cleanups\":[60,77],\"seed2_push\":68,\"free_call\":93,\"free_return\":99,\"exit_call\":105,\"exit_return\":111},\"fault_store\":{\"offset\":558,\"kernel_offset\":46,\"width\":2,\"base_register\":5,\"index_register\":6,\"scale\":8,\"displacement\":4,\"record_index\":3,\"pass\":2},\"helper_imports\":[\"guard_resident\",\"read8\",\"read16\",\"read32\",\"store_resident8\",\"store_resident16\",\"store_resident32\"]}");
        json
    }
}
