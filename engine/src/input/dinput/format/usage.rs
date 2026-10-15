use super::{DeviceDescription32, DeviceObject32};
pub fn object_usage(object: DeviceObject32) -> (u16, u16) {
    match object {
        DeviceObject32::Key(scan) => (
            7,
            (4..=0xe7)
                .find(|usage| keyboard_usage(*usage) == Some(scan))
                .unwrap_or(0),
        ),
        DeviceObject32::Button(index) => (9, index + 1),
        DeviceObject32::Axis { index, .. } => (1, 0x30 + u16::from(index)),
        DeviceObject32::Pov(_) => (1, 0x39),
    }
}
pub fn usage_object(description: DeviceDescription32, packed: u32) -> Option<DeviceObject32> {
    let page = (packed >> 16) as u16;
    let usage = packed as u16;
    description
        .objects()
        .find(|object| object_usage(*object) == (page, usage))
}
fn keyboard_usage(usage: u16) -> Option<u16> {
    let virtual_key = match usage {
        4..=29 => u32::from(usage - 4) + 0x41,
        30..=38 => u32::from(usage - 30) + 0x31,
        39 => 0x30,
        40 => 0x0d,
        41 => 0x1b,
        42 => 8,
        43 => 9,
        44 => 0x20,
        45 => 0xbd,
        46 => 0xbb,
        47 => 0xdb,
        48 => 0xdd,
        49 => 0xdc,
        51 => 0xba,
        52 => 0xde,
        53 => 0xc0,
        54 => 0xbc,
        55 => 0xbe,
        56 => 0xbf,
        57 => 0x14,
        58..=69 => u32::from(usage - 58) + 0x70,
        70 => 0x2c,
        71 => 0x91,
        72 => 0x13,
        73 => 0x2d,
        74 => 0x24,
        75 => 0x21,
        76 => 0x2e,
        77 => 0x23,
        78 => 0x22,
        79 => 0x27,
        80 => 0x25,
        81 => 0x28,
        82 => 0x26,
        83 => 0x90,
        84 => 0x6f,
        85 => 0x6a,
        86 => 0x6d,
        87 => 0x6b,
        88 => return Some(0x9c),
        89..=97 => u32::from(usage - 89) + 0x61,
        98 => 0x60,
        99 => 0x6e,
        100 => 0xe2,
        101 => 0x5d,
        0xe0 => 0xa2,
        0xe1 => 0xa0,
        0xe2 => 0xa4,
        0xe3 => 0x5b,
        0xe4 => 0xa3,
        0xe5 => 0xa1,
        0xe6 => 0xa5,
        0xe7 => 0x5c,
        _ => return None,
    };
    crate::input::keyboard::scan_code(virtual_key)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hid_keyboard_usages_resolve_physical_keys_and_extended_modifiers() {
        assert_eq!(
            usage_object(DeviceDescription32::keyboard(), 0x0007_0004),
            Some(DeviceObject32::Key(30))
        );
        assert_eq!(
            usage_object(DeviceDescription32::keyboard(), 0x0007_00e4),
            Some(DeviceObject32::Key(0x9d))
        );
        assert_eq!(object_usage(DeviceObject32::Pov(0)), (1, 0x39));
    }
}
