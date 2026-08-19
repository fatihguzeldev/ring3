/// a 32-bit byte address in a guest virtual address space.
///
/// does not prove that memory is mapped, accessible, executable, or aligned.
/// carries no host or wasm pointer, address-space identity, or endianness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GuestAddress32(u32);

impl GuestAddress32 {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// returns `None` on 32-bit overflow.
    #[must_use]
    pub const fn checked_add(self, byte_count: u32) -> Option<Self> {
        match self.0.checked_add(byte_count) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// returns `None` on underflow.
    #[must_use]
    pub const fn checked_sub(self, byte_count: u32) -> Option<Self> {
        match self.0.checked_sub(byte_count) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// a 32-bit control-flow value for the next guest instruction.
///
/// does not prove that the location is mapped, executable, aligned, or decodable.
/// this is not a [`GuestAddress32`].
///
/// ```compile_fail
/// use ring3_core::{GuestAddress32, ProgramCounter32};
///
/// fn read_guest_address(_address: GuestAddress32) {}
///
/// read_guest_address(ProgramCounter32::new(0x0040_1000));
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramCounter32(u32);

impl ProgramCounter32 {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// returns `None` on 32-bit overflow.
    #[must_use]
    pub const fn checked_add(self, byte_count: u32) -> Option<Self> {
        match self.0.checked_add(byte_count) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// returns `None` on underflow.
    #[must_use]
    pub const fn checked_sub(self, byte_count: u32) -> Option<Self> {
        match self.0.checked_sub(byte_count) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GuestAddress32, ProgramCounter32};

    #[test]
    fn guest_address_preserves_every_representative_value() {
        for value in [0, 0x0040_1000, u32::MAX] {
            assert_eq!(GuestAddress32::new(value).get(), value);
        }
    }

    #[test]
    fn program_counter_preserves_every_representative_value() {
        for value in [0, 0x0040_1000, u32::MAX] {
            assert_eq!(ProgramCounter32::new(value).get(), value);
        }
    }

    #[test]
    fn guest_address_addition_is_checked() {
        assert_eq!(
            GuestAddress32::new(0x0040_1000).checked_add(0),
            Some(GuestAddress32::new(0x0040_1000))
        );
        assert_eq!(
            GuestAddress32::new(0x0040_1000).checked_add(5),
            Some(GuestAddress32::new(0x0040_1005))
        );
        assert_eq!(
            GuestAddress32::new(u32::MAX - 1).checked_add(1),
            Some(GuestAddress32::new(u32::MAX))
        );
        assert_eq!(GuestAddress32::new(u32::MAX).checked_add(1), None);
    }

    #[test]
    fn program_counter_addition_is_checked() {
        assert_eq!(
            ProgramCounter32::new(0x0040_1000).checked_add(0),
            Some(ProgramCounter32::new(0x0040_1000))
        );
        assert_eq!(
            ProgramCounter32::new(0x0040_1000).checked_add(5),
            Some(ProgramCounter32::new(0x0040_1005))
        );
        assert_eq!(
            ProgramCounter32::new(u32::MAX - 1).checked_add(1),
            Some(ProgramCounter32::new(u32::MAX))
        );
        assert_eq!(ProgramCounter32::new(u32::MAX).checked_add(1), None);
    }

    #[test]
    fn guest_address_subtraction_is_checked() {
        assert_eq!(
            GuestAddress32::new(0x0040_1000).checked_sub(0),
            Some(GuestAddress32::new(0x0040_1000))
        );
        assert_eq!(
            GuestAddress32::new(0x0040_1005).checked_sub(5),
            Some(GuestAddress32::new(0x0040_1000))
        );
        assert_eq!(
            GuestAddress32::new(1).checked_sub(1),
            Some(GuestAddress32::new(0))
        );
        assert_eq!(GuestAddress32::new(0).checked_sub(1), None);
    }

    #[test]
    fn program_counter_subtraction_is_checked() {
        assert_eq!(
            ProgramCounter32::new(0x0040_1000).checked_sub(0),
            Some(ProgramCounter32::new(0x0040_1000))
        );
        assert_eq!(
            ProgramCounter32::new(0x0040_1005).checked_sub(5),
            Some(ProgramCounter32::new(0x0040_1000))
        );
        assert_eq!(
            ProgramCounter32::new(1).checked_sub(1),
            Some(ProgramCounter32::new(0))
        );
        assert_eq!(ProgramCounter32::new(0).checked_sub(1), None);
    }
}
