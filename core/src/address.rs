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

/// a byte offset from a pe image base.
///
/// carries no image identity and does not prove that the location is mapped or
/// file-backed.
///
/// ```compile_fail
/// use ring3_core::{GuestAddress32, RelativeVirtualAddress};
///
/// fn read_guest_address(_address: GuestAddress32) {}
///
/// read_guest_address(RelativeVirtualAddress::new(0x1000));
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RelativeVirtualAddress(u32);

impl RelativeVirtualAddress {
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

/// a byte offset in a file.
///
/// carries no file identity and does not prove that the byte exists.
///
/// ```compile_fail
/// use ring3_core::{FileOffset, GuestAddress32};
///
/// fn read_guest_address(_address: GuestAddress32) {}
///
/// read_guest_address(FileOffset::new(0x400));
/// ```
///
/// ```compile_fail
/// use ring3_core::{FileOffset, RelativeVirtualAddress};
///
/// fn read_rva(_address: RelativeVirtualAddress) {}
///
/// read_rva(FileOffset::new(0x400));
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileOffset(u64);

impl FileOffset {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// returns `None` on 64-bit overflow.
    #[must_use]
    pub const fn checked_add(self, byte_count: u64) -> Option<Self> {
        match self.0.checked_add(byte_count) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// returns `None` on underflow.
    #[must_use]
    pub const fn checked_sub(self, byte_count: u64) -> Option<Self> {
        match self.0.checked_sub(byte_count) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FileOffset, GuestAddress32, ProgramCounter32, RelativeVirtualAddress};

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

    #[test]
    fn relative_virtual_address_preserves_every_representative_value() {
        for value in [0, 0x1000, u32::MAX] {
            assert_eq!(RelativeVirtualAddress::new(value).get(), value);
        }
    }

    #[test]
    fn relative_virtual_address_addition_is_checked() {
        assert_eq!(
            RelativeVirtualAddress::new(0x1000).checked_add(0),
            Some(RelativeVirtualAddress::new(0x1000))
        );
        assert_eq!(
            RelativeVirtualAddress::new(0x1000).checked_add(5),
            Some(RelativeVirtualAddress::new(0x1005))
        );
        assert_eq!(
            RelativeVirtualAddress::new(u32::MAX - 1).checked_add(1),
            Some(RelativeVirtualAddress::new(u32::MAX))
        );
        assert_eq!(RelativeVirtualAddress::new(u32::MAX).checked_add(1), None);
    }

    #[test]
    fn relative_virtual_address_subtraction_is_checked() {
        assert_eq!(
            RelativeVirtualAddress::new(0x1000).checked_sub(0),
            Some(RelativeVirtualAddress::new(0x1000))
        );
        assert_eq!(
            RelativeVirtualAddress::new(0x1005).checked_sub(5),
            Some(RelativeVirtualAddress::new(0x1000))
        );
        assert_eq!(
            RelativeVirtualAddress::new(1).checked_sub(1),
            Some(RelativeVirtualAddress::new(0))
        );
        assert_eq!(RelativeVirtualAddress::new(0).checked_sub(1), None);
    }

    #[test]
    fn file_offset_preserves_every_representative_value() {
        for value in [0, 0x400, 0x1_0000_0000, u64::MAX] {
            assert_eq!(FileOffset::new(value).get(), value);
        }
    }

    #[test]
    fn file_offset_addition_is_checked() {
        assert_eq!(
            FileOffset::new(0x1_0000_0000).checked_add(0),
            Some(FileOffset::new(0x1_0000_0000))
        );
        assert_eq!(
            FileOffset::new(0x1_0000_0000).checked_add(5),
            Some(FileOffset::new(0x1_0000_0005))
        );
        assert_eq!(
            FileOffset::new(u64::MAX - 1).checked_add(1),
            Some(FileOffset::new(u64::MAX))
        );
        assert_eq!(FileOffset::new(u64::MAX).checked_add(1), None);
    }

    #[test]
    fn file_offset_subtraction_is_checked() {
        assert_eq!(
            FileOffset::new(0x1_0000_0000).checked_sub(0),
            Some(FileOffset::new(0x1_0000_0000))
        );
        assert_eq!(
            FileOffset::new(0x1_0000_0005).checked_sub(5),
            Some(FileOffset::new(0x1_0000_0000))
        );
        assert_eq!(FileOffset::new(1).checked_sub(1), Some(FileOffset::new(0)));
        assert_eq!(FileOffset::new(0).checked_sub(1), None);
    }
}
