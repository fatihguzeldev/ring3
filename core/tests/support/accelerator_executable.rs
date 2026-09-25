use super::imported_executable;

use super::resource_executable;

pub fn pe32(entries: &[[u16; 4]]) -> Vec<u8> {
    let mut bytes = resource_executable::guest();
    let imports = imported_executable::pe32(
        &[
            0x6a, 1, 0x6a, 0, 0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x89, 0xc3, 0x6a, 0, 0x6a, 0, 0x50,
            0xff, 0x15, 0x64, 0x20, 0x40, 0, 0x89, 0xc1, 0x6a, 2, 0x68, 0x80, 0x21, 0x40, 0, 0x53,
            0xff, 0x15, 0x64, 0x20, 0x40, 0, 0xcc,
        ],
        "USER32.dll",
        &["LoadAcceleratorsA", "CopyAcceleratorTableA"],
    );
    bytes[0x200..0x600].copy_from_slice(&imports[0x200..0x600]);
    resource_executable::put(&mut bytes, 0x104, 40);
    resource_executable::put(&mut bytes, 0x710, 9);
    let size = u32::try_from(entries.len() * 8).unwrap();
    resource_executable::put(&mut bytes, 0x74c, size);
    resource_executable::put(&mut bytes, 0x10c, 88 + size);
    for (index, entry) in entries.iter().enumerate() {
        for (word, value) in entry.iter().enumerate() {
            let offset = 0x758 + index * 8 + word * 2;
            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

pub fn guest() -> Vec<u8> {
    pe32(&[[9, 65, 100, 0xdead], [0x80, 120, 200, 0xbeef]])
}
