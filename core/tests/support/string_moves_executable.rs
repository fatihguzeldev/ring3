use super::executable;

pub fn pe32() -> Vec<u8> {
    let mut bytes = executable::pe32(&[
        0xbe, 0x80, 0x21, 0x40, 0, 0xbf, 0xc0, 0x21, 0x40, 0, 0xa4, 0x66, 0xa5, 0xa5, 0xcc,
    ]);
    bytes[1408..1415].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7]);
    bytes
}
