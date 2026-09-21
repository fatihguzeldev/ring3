use super::{Cpu32, Register32};

impl Cpu32 {
    pub(super) fn identify(&mut self) {
        let values = match self.register(Register32::Eax) {
            0 => [
                1,
                u32::from_le_bytes(*b"Ring"),
                u32::from_le_bytes(*b"Core"),
                u32::from_le_bytes(*b"3CPU"),
            ],
            0x8000_0000 => [0x8000_0000, 0, 0, 0],
            // leaf 1 has no optional features; out-of-range inputs return that leaf.
            _ => [0; 4],
        };
        for (register, value) in [
            Register32::Eax,
            Register32::Ebx,
            Register32::Ecx,
            Register32::Edx,
        ]
        .into_iter()
        .zip(values)
        {
            self.set_register(register, value);
        }
    }
}
