use super::support_pe32 as base_pe;

pub const PREFERRED_BASE: u32 = 0x0040_0000;
pub const ACTUAL_BASE: u32 = 0x0050_0000;
pub const IMAGE_SIZE: u32 = 0x6000;
pub const MAIN_RVA: u32 = 0x1000;
pub const KERNEL_RVA: u32 = 0x2000;
pub const DATA_RVA: u32 = 0x3000;
pub const TABLE_RVA: u32 = 0x4000;
pub const FIXUP_RVA: u32 = 0x5000;
pub const MAIN_RAW: usize = 0x400;
pub const KERNEL_RAW: usize = 0x600;
pub const DATA_RAW: usize = 0x800;
pub const TABLE_RAW: usize = 0xa00;
pub const FIXUP_RAW: usize = 0xc00;
pub const STATE_RVA: u32 = DATA_RVA;
pub const OUTPUT_RVA: u32 = DATA_RVA + 4;
pub const CANARY_RVA: u32 = DATA_RVA + 0x34;
pub const IAT_EXIT_RVA: u32 = 0x3150;
pub const GATE_BASE: u32 = 0x8000;
pub const GATE_OFFSET: u32 = 32;
pub const STACK_BASE: u32 = 0x70000;
pub const INITIAL_STATE: u32 = 0xffff_fffc;
pub const INITIAL_SALT: u32 = 0xdead_beef;
pub const SELECTORS: [u32; 6] = [0, 1, 2, 3, 4, u32::MAX];
pub const EXPECTED_STATES: [u32; 6] = [
    3,
    0xa5a5_5a59,
    0xb4ab_4b34,
    0x1e01_e19d,
    0x1e01_e199,
    0x1e01_e19a,
];
pub const EXPECTED_RETURNS: [u32; 6] = [
    0xdead_beec,
    0x7b08_e4b5,
    0xcfa3_af81,
    0xd1a2_4e1c,
    0xcfa3_af85,
    0xd1a2_4e1f,
];
pub const KERNEL_ENTRIES: [u32; 8] = [0, 0x14, 0x1b, 0x20, 0x25, 0x2b, 0x33, 0x35];
pub const TABLE_TARGETS: [u32; 4] = [0x1b, 0x2b, 0x20, 0x25];
pub const MAIN_BLOCK_LENGTHS: [u32; 7] = [34, 25, 25, 25, 25, 25, 23];
pub const MAIN_BLOCK_INSTRUCTIONS: [u32; 7] = [9, 8, 8, 8, 8, 8, 6];
pub const KERNEL_BLOCK_LENGTHS: [u32; 8] = [20, 7, 5, 5, 6, 8, 2, 6];
pub const KERNEL_BLOCK_INSTRUCTIONS: [u32; 8] = [7, 1, 2, 2, 3, 2, 1, 4];
pub const KERNEL_RAW_TEXT: &[u8] =
    include_bytes!("../../programs/dispatch/fixtures/p2-compiled-switch/kernel.text.raw.bin");
pub const KERNEL_RAW_TABLE: &[u8] =
    include_bytes!("../../programs/dispatch/fixtures/p2-compiled-switch/kernel.rdata.raw.bin");
pub const KERNEL: &[u8] =
    include_bytes!("../../programs/dispatch/fixtures/p2-compiled-switch/kernel.text.bin");
pub const TABLE: &[u8] =
    include_bytes!("../../programs/dispatch/fixtures/p2-compiled-switch/kernel.rdata.bin");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fixup {
    pub rva: u32,
    pub target_rva: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fixture {
    pub image: Vec<u8>,
    pub main: Vec<u8>,
    pub main_entries: Vec<u32>,
    pub call_offsets: Vec<u32>,
    pub return_offsets: Vec<u32>,
    pub authored_fixups: Vec<Fixup>,
    pub compiler_fixups: Vec<Fixup>,
}

fn absolute(main: &mut Vec<u8>, fixups: &mut Vec<Fixup>, opcode: &[u8], target_rva: u32) {
    main.extend_from_slice(opcode);
    fixups.push(Fixup {
        rva: MAIN_RVA + main.len() as u32,
        target_rva,
    });
    main.extend_from_slice(&(PREFERRED_BASE + target_rva).to_le_bytes());
}

pub fn fixture() -> Fixture {
    assert_eq!(KERNEL.len(), 59);
    assert_eq!(TABLE.len(), 16);
    let mut main = Vec::new();
    let mut authored_fixups = Vec::new();
    let mut main_entries = vec![0];
    let mut call_offsets = Vec::new();
    let mut return_offsets = Vec::new();
    for (opcode, value) in [
        (0xbb, 0xbbbb_0001_u32),
        (0xbd, 0xbbbb_0002),
        (0xbe, 0xbbbb_0003),
    ] {
        main.push(opcode);
        main.extend_from_slice(&value.to_le_bytes());
    }
    absolute(&mut main, &mut authored_fixups, &[0xbf], STATE_RVA);
    main.push(0xb8);
    main.extend_from_slice(&INITIAL_SALT.to_le_bytes());
    for (index, selector) in SELECTORS.into_iter().enumerate() {
        main.push(0x50);
        if selector <= 127 || selector == u32::MAX {
            main.extend_from_slice(&[0x6a, selector as u8]);
        } else {
            main.push(0x68);
            main.extend_from_slice(&selector.to_le_bytes());
        }
        main.push(0x57);
        let call = main.len() as u32;
        call_offsets.push(call);
        main.push(0xe8);
        main.extend_from_slice(&(KERNEL_RVA.wrapping_sub(MAIN_RVA + call + 5)).to_le_bytes());
        return_offsets.push(main.len() as u32);
        main_entries.push(main.len() as u32);
        main.extend_from_slice(&[0x83, 0xc4, 12]);
        absolute(
            &mut main,
            &mut authored_fixups,
            &[0xa3],
            OUTPUT_RVA + index as u32 * 8,
        );
        main.extend_from_slice(&[0x8b, 0x0f]);
        absolute(
            &mut main,
            &mut authored_fixups,
            &[0x89, 0x0d],
            OUTPUT_RVA + index as u32 * 8 + 4,
        );
    }
    main.push(0x50);
    absolute(&mut main, &mut authored_fixups, &[0xff, 0x15], IAT_EXIT_RVA);
    let compiler_fixups = std::iter::once(Fixup {
        rva: KERNEL_RVA + 23,
        target_rva: TABLE_RVA,
    })
    .chain(
        TABLE_TARGETS
            .into_iter()
            .enumerate()
            .map(|(index, target)| Fixup {
                rva: TABLE_RVA + index as u32 * 4,
                target_rva: KERNEL_RVA + target,
            }),
    )
    .collect::<Vec<_>>();
    let mut image = base_pe::image();
    image.resize(0xe00, 0);
    image[base_pe::SECTIONS..].fill(0);
    base_pe::put16(&mut image, base_pe::COFF + 2, 5);
    base_pe::put16(&mut image, base_pe::COFF + 18, 0x0102);
    base_pe::put32(&mut image, base_pe::OPTIONAL + 4, 1024);
    base_pe::put32(&mut image, base_pe::OPTIONAL + 8, 1536);
    base_pe::put32(&mut image, base_pe::OPTIONAL + 12, 0);
    base_pe::put32(&mut image, base_pe::OPTIONAL + 16, MAIN_RVA);
    base_pe::put32(&mut image, base_pe::OPTIONAL + 20, MAIN_RVA);
    base_pe::put32(&mut image, base_pe::OPTIONAL + 24, DATA_RVA);
    base_pe::put32(&mut image, base_pe::OPTIONAL + 56, IMAGE_SIZE);
    base_pe::put32(&mut image, base_pe::OPTIONAL + 60, 1024);
    for (index, name, rva, raw, size, flags) in [
        (
            0,
            *b".main\0\0\0",
            MAIN_RVA,
            MAIN_RAW,
            main.len(),
            0x6000_0020,
        ),
        (
            1,
            *b".text\0\0\0",
            KERNEL_RVA,
            KERNEL_RAW,
            KERNEL.len(),
            0x6000_0020,
        ),
        (2, *b".data\0\0\0", DATA_RVA, DATA_RAW, 4096, 0xc000_0040),
        (
            3,
            *b".rdata\0\0",
            TABLE_RVA,
            TABLE_RAW,
            TABLE.len(),
            0x4000_0040,
        ),
        (4, *b".reloc\0\0", FIXUP_RVA, FIXUP_RAW, 64, 0x4000_0040),
    ] {
        let at = base_pe::section(index);
        image[at..at + 8].copy_from_slice(&name);
        base_pe::put32(&mut image, at + 8, size as u32);
        base_pe::put32(&mut image, at + 12, rva);
        base_pe::put32(&mut image, at + 16, 512);
        base_pe::put32(&mut image, at + 20, raw as u32);
        base_pe::put32(&mut image, at + 36, flags);
    }
    image[MAIN_RAW..MAIN_RAW + 512].fill(0xcc);
    image[MAIN_RAW..MAIN_RAW + main.len()].copy_from_slice(&main);
    image[KERNEL_RAW..KERNEL_RAW + 512].fill(0xcc);
    image[KERNEL_RAW..KERNEL_RAW + KERNEL.len()].copy_from_slice(KERNEL);
    image[TABLE_RAW..TABLE_RAW + TABLE.len()].copy_from_slice(TABLE);
    base_pe::put32(&mut image, DATA_RAW, INITIAL_STATE);
    base_pe::put32(&mut image, DATA_RAW + 0x34, 0x1357_9bdf);
    for (offset, value) in [
        (0x100, 0x3140),
        (0x10c, 0x3160),
        (0x110, IAT_EXIT_RVA),
        (0x140, 0x3170),
        (0x150, 0x3170),
    ] {
        base_pe::put32(&mut image, DATA_RAW + offset, value);
    }
    image[DATA_RAW + 0x160..DATA_RAW + 0x16d].copy_from_slice(b"KeRnEl32.dLl\0");
    base_pe::put16(&mut image, DATA_RAW + 0x170, 0x1234);
    image[DATA_RAW + 0x172..DATA_RAW + 0x17e].copy_from_slice(b"ExitProcess\0");
    let mut reloc = Vec::new();
    for page in [MAIN_RVA, KERNEL_RVA, TABLE_RVA] {
        let fields = authored_fixups
            .iter()
            .chain(&compiler_fixups)
            .filter(|fixup| fixup.rva & !4095 == page)
            .map(|fixup| 0x3000 | (fixup.rva & 4095) as u16)
            .collect::<Vec<_>>();
        let block_size = 8 + (fields.len() + fields.len() % 2) * 2;
        reloc.extend_from_slice(&page.to_le_bytes());
        reloc.extend_from_slice(&(block_size as u32).to_le_bytes());
        for field in fields {
            reloc.extend_from_slice(&field.to_le_bytes());
        }
        if reloc.len() % 4 != 0 {
            reloc.extend_from_slice(&[0, 0]);
        }
    }
    assert_eq!(reloc.len(), 64);
    image[FIXUP_RAW..FIXUP_RAW + reloc.len()].copy_from_slice(&reloc);
    for (index, rva, length) in [
        (1, 0x3100, 40),
        (5, FIXUP_RVA, reloc.len() as u32),
        (12, IAT_EXIT_RVA, 8),
    ] {
        base_pe::put32(&mut image, base_pe::OPTIONAL + 96 + index * 8, rva);
        base_pe::put32(&mut image, base_pe::OPTIONAL + 100 + index * 8, length);
    }
    Fixture {
        image,
        main,
        main_entries,
        call_offsets,
        return_offsets,
        authored_fixups,
        compiler_fixups,
    }
}

impl Fixture {
    pub fn fixups(&self) -> impl Iterator<Item = &Fixup> {
        self.authored_fixups.iter().chain(&self.compiler_fixups)
    }

    pub fn manifest_json(&self) -> String {
        use std::fmt::Write as _;
        fn hex(bytes: &[u8]) -> String {
            bytes.iter().map(|byte| format!("{byte:02x}")).collect()
        }
        let mut json = format!(
            "{{\"preferred_base\":{PREFERRED_BASE},\"actual_base\":{ACTUAL_BASE},\"image_size\":{IMAGE_SIZE},\"main_rva\":{MAIN_RVA},\"kernel_rva\":{KERNEL_RVA},\"data_rva\":{DATA_RVA},\"table_rva\":{TABLE_RVA},\"fixup_rva\":{FIXUP_RVA},\"state_rva\":{STATE_RVA},\"output_rva\":{OUTPUT_RVA},\"canary_rva\":{CANARY_RVA},\"iat_exit_rva\":{IAT_EXIT_RVA},\"gate_base\":{GATE_BASE},\"gate_offset\":{GATE_OFFSET},\"stack_base\":{STACK_BASE},\"initial_state\":{INITIAL_STATE},\"initial_salt\":{INITIAL_SALT},\"selectors\":{SELECTORS:?},\"expected_states\":{EXPECTED_STATES:?},\"expected_returns\":{EXPECTED_RETURNS:?},\"main_entries\":{:?},\"call_offsets\":{:?},\"return_offsets\":{:?},\"block_lengths\":{MAIN_BLOCK_LENGTHS:?},\"block_instructions\":{MAIN_BLOCK_INSTRUCTIONS:?},\"kernel_entries\":{KERNEL_ENTRIES:?},\"kernel_block_lengths\":{KERNEL_BLOCK_LENGTHS:?},\"kernel_block_instructions\":{KERNEL_BLOCK_INSTRUCTIONS:?},\"table_targets\":{TABLE_TARGETS:?},\"main_hex\":\"{}\",\"kernel_raw_hex\":\"{}\",\"kernel_linked_hex\":\"{}\",\"table_raw_hex\":\"{}\",\"table_linked_hex\":\"{}\",\"compiler_highlow_rvas\":[8215,16384,16388,16392,16396],\"authored_fixups\":[",
            self.main_entries,
            self.call_offsets,
            self.return_offsets,
            hex(&self.main),
            hex(KERNEL_RAW_TEXT),
            hex(KERNEL),
            hex(KERNEL_RAW_TABLE),
            hex(TABLE)
        );
        for (index, fixup) in self.authored_fixups.iter().enumerate() {
            if index != 0 {
                json.push(',');
            }
            write!(
                &mut json,
                "{{\"rva\":{},\"target_rva\":{}}}",
                fixup.rva, fixup.target_rva
            )
            .unwrap();
        }
        json.push_str("],\"fixups\":[");
        for (index, fixup) in self.fixups().enumerate() {
            if index != 0 {
                json.push(',');
            }
            write!(
                &mut json,
                "{{\"rva\":{},\"target_rva\":{}}}",
                fixup.rva, fixup.target_rva
            )
            .unwrap();
        }
        json.push_str("]}");
        json
    }
}
