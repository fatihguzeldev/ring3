#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{Access, LoadError, MemoryError, load_pe32};

#[test]
fn maps_headers_sections_and_zero_fill_with_final_permissions() {
    let bytes = executable::pe32(&[0xb8, 42, 0, 0, 0, 0xcc]);
    let mut image = load_pe32(&bytes, 3).unwrap();
    assert_eq!(
        (image.image_base, image.entry_point),
        (0x40_0000, 0x40_1000)
    );
    let mut header = [0; 2];
    image.memory.read(0x40_0000, &mut header).unwrap();
    assert_eq!(&header, b"MZ");
    let mut code = [0; 6];
    image.memory.fetch(0x40_1000, &mut code).unwrap();
    assert_eq!(code, [0xb8, 42, 0, 0, 0, 0xcc]);
    let mut data = [0; 2];
    image.memory.read(0x40_2000, &mut data).unwrap();
    assert_eq!(data, [17, 0]);
    image.memory.read(0x40_2ffe, &mut data).unwrap();
    assert_eq!(data, [0, 0]);
    image.memory.write(0x40_2fff, &[9]).unwrap();
    assert_eq!(
        image.memory.write(0x40_1000, &[0]),
        Err(MemoryError::PermissionDenied {
            address: 0x40_1000,
            access: Access::Write
        })
    );
    assert!(image.memory.fetch(0x40_2000, &mut data).is_err());
}

#[test]
fn rejects_unsupported_process_requirements_and_architectures() {
    for index in [1, 9, 10, 12, 13, 14] {
        let mut bytes = executable::pe32(&[0xcc]);
        let slot = 0xf8 + usize::from(index) * 8;
        bytes[slot..slot + 4].copy_from_slice(&0x2000_u32.to_le_bytes());
        assert!(
            matches!(load_pe32(&bytes, 3), Err(LoadError::UnsupportedDirectory { index: actual }) if actual == index)
        );
    }
    let mut bytes = executable::pe32(&[0xcc]);
    bytes[0x84..0x86].copy_from_slice(&0x8664_u16.to_le_bytes());
    assert!(load_pe32(&bytes, 3).is_err());
    bytes[0x84..0x86].copy_from_slice(&0x14c_u16.to_le_bytes());
    bytes[0x96..0x98].copy_from_slice(&0x2102_u16.to_le_bytes());
    assert!(matches!(
        load_pe32(&bytes, 3),
        Err(LoadError::UnsupportedImage)
    ));
}

#[test]
fn rejects_bad_layout_entry_and_allocation_requests() {
    let bytes = executable::pe32(&[0xcc]);
    assert!(matches!(
        load_pe32(&bytes, 2),
        Err(LoadError::Memory(MemoryError::PageLimitExceeded))
    ));
    for (offset, value) in [
        (0xd4, 128_u32),
        (0xd0, 4096),
        (0xb4, 0xffff_0000),
        (0x1ac, 0x1000),
        (0x184, 0x1001),
        (0xa8, 0x2000),
    ] {
        let mut bad = bytes.clone();
        bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        if offset == 0xb4 {
            bad[0xd0..0xd4].copy_from_slice(&0x2_0000_u32.to_le_bytes());
        }
        assert!(load_pe32(&bad, 32).is_err(), "offset {offset:x}");
    }
    assert!(load_pe32(&bytes[..1100], 3).is_err());
}
