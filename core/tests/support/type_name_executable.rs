use super::imported_executable;

pub fn pe32() -> Vec<u8> {
    let mut bytes = imported_executable::pe32(
        &[
            0xb9, 0x80, 0x21, 0x40, 0, 0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x89, 0xc6, 0xff, 0x15,
            0x60, 0x20, 0x40, 0, 0xcc,
        ],
        "mSvCrT.dll",
        &["?name@type_info@@QBEPBDXZ"],
    );
    let name = b".?AVWidget@engine@@\0";
    bytes[0x588..0x588 + name.len()].copy_from_slice(name);
    bytes
}
