const BASE: u32 = 0x0040_0000;
const PE: usize = 0x80;
const COFF: usize = PE + 4;
const OPTIONAL: usize = COFF + 20;
const SECTIONS: usize = OPTIONAL + 224;
const IMAGE_SIZE: u32 = 0x7000;
const TEXT_RAW: usize = 0x200;
const DATA_RAW: usize = 0x400;
const RX: u32 = 0x6000_0020;
const RW: u32 = 0xc000_0040;

fn put16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

const fn section(index: usize) -> usize {
    SECTIONS + index * 40
}

fn base_image() -> Vec<u8> {
    let mut bytes = vec![0; 0x600];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 0x3c, PE as u32);
    bytes[PE..PE + 4].copy_from_slice(b"PE\0\0");
    put16(&mut bytes, COFF, 0x14c);
    put16(&mut bytes, COFF + 2, 3);
    put16(&mut bytes, COFF + 16, 224);
    put16(&mut bytes, COFF + 18, 0x0103);
    put16(&mut bytes, OPTIONAL, 0x10b);
    put32(&mut bytes, OPTIONAL + 4, 0x200);
    put32(&mut bytes, OPTIONAL + 8, 0x200);
    put32(&mut bytes, OPTIONAL + 12, 0x1000);
    put32(&mut bytes, OPTIONAL + 16, 0x1000);
    put32(&mut bytes, OPTIONAL + 20, 0x1000);
    put32(&mut bytes, OPTIONAL + 24, 0x3000);
    put32(&mut bytes, OPTIONAL + 28, BASE);
    put32(&mut bytes, OPTIONAL + 32, 4096);
    put32(&mut bytes, OPTIONAL + 36, 512);
    put32(&mut bytes, OPTIONAL + 56, IMAGE_SIZE);
    put32(&mut bytes, OPTIONAL + 60, 512);
    put16(&mut bytes, OPTIONAL + 68, 3);
    put16(&mut bytes, OPTIONAL + 70, 0x0100);
    put32(&mut bytes, OPTIONAL + 72, 0x100000);
    put32(&mut bytes, OPTIONAL + 76, 4096);
    put32(&mut bytes, OPTIONAL + 80, 0x100000);
    put32(&mut bytes, OPTIONAL + 84, 4096);
    put32(&mut bytes, OPTIONAL + 92, 16);
    for (index, name, virtual_size, rva, raw_size, raw_pointer, flags) in [
        (0, *b".text\0\0\0", 16, 0x1000, 512, TEXT_RAW as u32, RX),
        (1, *b".data\0\0\0", 1536, 0x3000, 512, DATA_RAW as u32, RW),
        (2, *b".bss\0\0\0\0", 4096, 0x5000, 0, 0, 0xc000_0080),
    ] {
        let offset = section(index);
        bytes[offset..offset + 8].copy_from_slice(&name);
        put32(&mut bytes, offset + 8, virtual_size);
        put32(&mut bytes, offset + 12, rva);
        put32(&mut bytes, offset + 16, raw_size);
        put32(&mut bytes, offset + 20, raw_pointer);
        put32(&mut bytes, offset + 36, flags);
    }
    bytes[TEXT_RAW..TEXT_RAW + 512].fill(0xa5);
    bytes[TEXT_RAW..TEXT_RAW + 5].copy_from_slice(&[0xb8, 42, 0, 0, 0]);
    for (index, byte) in bytes[DATA_RAW..DATA_RAW + 512].iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(17).wrapping_add(9);
    }
    bytes
}

pub const EXE_BASE: u32 = 0x0040_0000;
pub const DLL_BASE: u32 = 0x0060_0000;

pub fn image(base: u32, dll: bool, entry: bool) -> Vec<u8> {
    let mut bytes = base_image();
    put16(&mut bytes, COFF + 18, if dll { 0x2103 } else { 0x0103 });
    put32(&mut bytes, OPTIONAL + 28, base);
    put32(&mut bytes, OPTIONAL + 16, if entry { 0x1000 } else { 0 });
    bytes[TEXT_RAW..TEXT_RAW + 16].copy_from_slice(&[
        0xb8, 1, 0, 0, 0, 0xc2, 12, 0, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0xc3,
    ]);
    bytes
}

pub fn tls_image(base: u32, dll: bool) -> Vec<u8> {
    let mut bytes = image(base, dll, true);
    put32(&mut bytes, section(0) + 8, 32);
    put32(&mut bytes, OPTIONAL + 96 + 9 * 8, 0x3000);
    put32(&mut bytes, OPTIONAL + 100 + 9 * 8, 24);
    for (index, value) in [
        base + 0x3060,
        base + 0x3064,
        base + 0x5000,
        base + 0x3040,
        12,
        0x0030_0000,
    ]
    .into_iter()
    .enumerate()
    {
        put32(&mut bytes, DATA_RAW + index * 4, value);
    }
    put32(&mut bytes, DATA_RAW + 0x40, base + 0x1010);
    put32(&mut bytes, DATA_RAW + 0x44, 0);
    bytes[DATA_RAW + 0x60..DATA_RAW + 0x64].copy_from_slice(&[0xaa, 0x11, 0x22, 0x33]);
    bytes[TEXT_RAW + 0x10..TEXT_RAW + 0x18].copy_from_slice(&[0xb8, 1, 0, 0, 0, 0xc2, 12, 0]);
    bytes
}

pub fn linked_image(base: u32, dll: bool, imports: &[&str]) -> Vec<u8> {
    let mut bytes = image(base, dll, !dll);
    bytes.resize(0xa00, 0);
    put32(&mut bytes, section(1) + 16, 1536);
    bytes[DATA_RAW..].fill(0);
    put32(&mut bytes, OPTIONAL + 96, 0x3000);
    put32(&mut bytes, OPTIONAL + 100, 0x80);
    for (offset, value) in [
        (12, 0x3040),
        (16, 1),
        (20, 1),
        (24, 1),
        (28, 0x3030),
        (32, 0x3034),
        (36, 0x3038),
    ] {
        put32(&mut bytes, DATA_RAW + offset, value);
    }
    put32(&mut bytes, DATA_RAW + 0x30, 0x1000);
    put32(&mut bytes, DATA_RAW + 0x34, 0x3050);
    bytes[DATA_RAW + 0x40..DATA_RAW + 0x4c].copy_from_slice(b"fixture.dll\0");
    bytes[DATA_RAW + 0x50..DATA_RAW + 0x57].copy_from_slice(b"answer\0");
    if !imports.is_empty() {
        assert!(imports.len() <= 4);
        put32(&mut bytes, OPTIONAL + 104, 0x3100);
        put32(&mut bytes, OPTIONAL + 108, (imports.len() as u32 + 1) * 20);
        for (index, name) in imports.iter().enumerate() {
            let slot = index as u32 * 16;
            let ilt = 0x3180 + slot;
            let iat = 0x3200 + slot;
            let name_rva = 0x3280 + index as u32 * 32;
            let hint = 0x3380 + slot;
            let descriptor = DATA_RAW + 0x100 + index * 20;
            for (offset, value) in [(0, ilt), (12, name_rva), (16, iat)] {
                put32(&mut bytes, descriptor + offset, value);
            }
            put32(&mut bytes, DATA_RAW + (ilt - 0x3000) as usize, hint);
            put32(&mut bytes, DATA_RAW + (iat - 0x3000) as usize, hint);
            let at = DATA_RAW + (name_rva - 0x3000) as usize;
            bytes[at..at + name.len()].copy_from_slice(name.as_bytes());
            let at = DATA_RAW + (hint - 0x3000) as usize;
            bytes[at + 2..at + 9].copy_from_slice(b"answer\0");
        }
    }
    bytes
}

pub fn entry_library(base: u32, imports: &[&str]) -> Vec<u8> {
    let mut bytes = linked_image(base, true, imports);
    put32(&mut bytes, OPTIONAL + 16, 0x1000);
    bytes
}

pub fn forwarding_exe(base: u32, forwarder: &str) -> Vec<u8> {
    assert!(forwarder.len() < 31);
    let mut bytes = linked_image(base, false, &[]);
    put32(&mut bytes, DATA_RAW + 0x30, 0x3060);
    bytes[DATA_RAW + 0x60..DATA_RAW + 0x60 + forwarder.len()].copy_from_slice(forwarder.as_bytes());
    bytes
}
