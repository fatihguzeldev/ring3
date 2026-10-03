use super::pe;

pub const DIRECTORY_RVA: u32 = 0x5000;
pub const DIRECTORY_OFFSET: usize = pe::DATA_RAW + 0x2000;
pub const DIRECTORY_FIELD: usize = pe::OPTIONAL + 96 + 5 * 8;
pub const WORDS: [(u32, u32); 4] = [
    (0x1001, 0x0040_3000),
    (0x3003, 0xffff_fffc),
    (0x3ffe, 0x0000_3000),
    (0x4104, 0x0000_0002),
];

pub fn directory(blocks: &[(u32, &[u16])]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (page, entries) in blocks {
        bytes.extend_from_slice(&page.to_le_bytes());
        bytes.extend_from_slice(&(8 + entries.len() as u32 * 2).to_le_bytes());
        for entry in *entries {
            bytes.extend_from_slice(&entry.to_le_bytes());
        }
    }
    bytes
}

pub fn image(directory: &[u8]) -> Vec<u8> {
    let mut bytes = pe::image();
    let raw_size = (0x2000 + directory.len()).max(0x3000).div_ceil(512) * 512;
    bytes.resize(pe::DATA_RAW + raw_size, 0);
    pe::put16(&mut bytes, pe::COFF + 18, 0x0102);
    pe::put32(&mut bytes, pe::section(1) + 8, raw_size as u32);
    pe::put32(&mut bytes, pe::section(1) + 16, raw_size as u32);
    let bss = 0x3000 + (raw_size as u32).div_ceil(4096) * 4096 + 4096;
    pe::put32(&mut bytes, pe::section(2) + 12, bss);
    pe::put32(&mut bytes, pe::OPTIONAL + 56, bss + 8192);
    pe::put32(&mut bytes, DIRECTORY_FIELD, DIRECTORY_RVA);
    pe::put32(&mut bytes, DIRECTORY_FIELD + 4, directory.len() as u32);
    for (rva, value) in WORDS {
        let offset = if rva < 0x3000 {
            pe::TEXT_RAW + (rva - 0x1000) as usize
        } else {
            pe::DATA_RAW + (rva - 0x3000) as usize
        };
        pe::put32(&mut bytes, offset, value);
    }
    bytes[DIRECTORY_OFFSET..DIRECTORY_OFFSET + directory.len()].copy_from_slice(directory);
    bytes
}

pub fn full_image() -> Vec<u8> {
    // both page and target ordering are deliberately nonascending.
    image(&directory(&[
        (0x4000, &[0x3104, 0x0fff]),
        (0x1000, &[0x3001, 0x0fff]),
        (0x3000, &[0x3ffe, 0x3003]),
    ]))
}
