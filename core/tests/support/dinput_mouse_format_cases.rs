use ring3_core::execution::GuestMemory;

pub use super::dinput_format_cases::FORMAT;
pub const OBJECTS: u32 = 0x3000_0101;
pub const GUIDS: u32 = 0x3000_0041;

pub fn standard(memory: &mut GuestMemory, explicit_buttons: bool) {
    let header: Vec<_> = [24_u32, 16, 2, 20, 11, OBJECTS]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    memory.write(u64::from(FORMAT), &header).unwrap();
    for (index, first) in [0xe0, 0xe1, 0xe2, 0xf0].into_iter().enumerate() {
        let guid = [
            first, 0x02, 0x6d, 0xa3, 0xf3, 0xc9, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0,
            0,
        ];
        memory
            .write(u64::from(GUIDS) + index as u64 * 16, &guid)
            .unwrap();
    }
    for index in 0..11 {
        let (guid, offset, kind) = if index < 3 {
            (
                GUIDS + index * 16,
                index * 4,
                0x00ff_ff03 | if index == 2 { 0x8000_0000 } else { 0 },
            )
        } else {
            (
                if explicit_buttons { GUIDS + 48 } else { 0 },
                index + 9,
                0x00ff_ff0c | if index >= 5 { 0x8000_0000 } else { 0 },
            )
        };
        let object: Vec<_> = [guid, offset, kind, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        memory
            .write(u64::from(OBJECTS + index * 16), &object)
            .unwrap();
    }
}

pub fn imported_standard_mouse_format_across_budgets() {
    super::dinput_format_cases::imported_format_across_budgets(
        super::dinput_mouse_cases::MOUSE,
        standard,
    );
}
