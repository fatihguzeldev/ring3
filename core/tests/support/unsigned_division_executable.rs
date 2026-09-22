use super::executable;

pub fn encoding(bits: u32, source: u8, address: Option<u32>) -> Vec<u8> {
    let mut code = Vec::new();
    if bits == 16 {
        code.push(0x66);
    }
    code.extend([if bits == 8 { 0xf6 } else { 0xf7 }, 0x30 | source]);
    if let Some(address) = address {
        code.extend(address.to_le_bytes());
    }
    code
}

pub fn pe32() -> Vec<u8> {
    executable::pe32(&[
        0xb8, 3, 0, 1, 0, 0xba, 1, 0, 0, 0, 0xbb, 17, 0, 0, 0, 0xf7, 0xf3, 0x89, 0xc6, 0x89, 0xd7,
        0xb8, 0xfd, 1, 0xcd, 0xab, 0xbb, 0, 2, 0, 0, 0xf6, 0xf7, 0x66, 0xb8, 3, 0, 0x66, 0xba, 1,
        0, 0x66, 0xbb, 1, 1, 0x66, 0xf7, 0xf3, 0xcc,
    ])
}
