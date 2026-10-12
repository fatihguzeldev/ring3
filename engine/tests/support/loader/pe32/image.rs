pub const BASE: u32 = 0x0040_0000;
pub const PE: usize = 0x80;
pub const COFF: usize = PE + 4;
pub const OPTIONAL: usize = COFF + 20;
pub const SECTIONS: usize = OPTIONAL + 224;
pub const IMAGE_SIZE: u32 = 0x7000;
pub const MAPPED_PAGES: u32 = 4;
pub const TEXT_RAW: usize = 0x200;
pub const DATA_RAW: usize = 0x400;
pub const RX: u32 = 0x6000_0020;
pub const RW: u32 = 0xc000_0040;

pub fn put16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

pub fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

pub const fn section(index: usize) -> usize {
    SECTIONS + index * 40
}

// authored independently of the loader; file padding deliberately has nonzero bytes.
pub fn image() -> Vec<u8> {
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

pub fn boundary_image() -> Vec<u8> {
    let original = image();
    let mut bytes = vec![0; 0x1600];
    bytes[..512].copy_from_slice(&original[..512]);
    bytes[0x1200..0x1400].copy_from_slice(&original[TEXT_RAW..TEXT_RAW + 512]);
    bytes[0x1400..0x1600].copy_from_slice(&original[DATA_RAW..DATA_RAW + 512]);
    put32(&mut bytes, OPTIONAL + 16, 0x2000);
    put32(&mut bytes, OPTIONAL + 20, 0x2000);
    put32(&mut bytes, OPTIONAL + 28, 0xff00_0000);
    put32(&mut bytes, OPTIONAL + 56, 0x0100_0000);
    put32(&mut bytes, OPTIONAL + 60, 0x1200);
    put32(&mut bytes, section(0) + 12, 0x2000);
    put32(&mut bytes, section(0) + 20, 0x1200);
    put32(&mut bytes, section(1) + 20, 0x1400);
    put32(&mut bytes, section(2) + 8, 0x00ff_c000);
    put32(&mut bytes, section(2) + 12, 0x4000);
    bytes[0x1100] = 0x6d;
    bytes[0x11ff] = 0xb3;
    bytes
}
