use super::super::code_pages;
use super::{DispatchError, GuestMemory, MemoryError};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum CodePage {
    #[default]
    Ansi,
    Oem,
    SingleByte,
}

impl CodePage {
    pub(super) fn set(&mut self, selector: i32) -> Result<(), DispatchError> {
        let selected = match selector {
            -3 => Self::Ansi,
            -2 => Self::Oem,
            0 => Self::SingleByte,
            value if value == code_pages::ANSI.cast_signed() => Self::Ansi,
            value if value == code_pages::OEM.cast_signed() => Self::Oem,
            _ => return Err(DispatchError::Unsupported),
        };
        *self = selected;
        Ok(())
    }

    pub(super) fn increment(
        self,
        memory: &GuestMemory,
        pointer: u32,
    ) -> Result<u32, DispatchError> {
        if pointer == 0 {
            return Err(DispatchError::Unsupported);
        }
        memory.read(u64::from(pointer), &mut [0])?;
        let width = match self {
            Self::Ansi | Self::Oem | Self::SingleByte => 1,
        };
        Ok(pointer
            .checked_add(width)
            .ok_or(MemoryError::AddressOverflow)?)
    }

    pub(super) fn reverse_search(
        self,
        memory: &GuestMemory,
        pointer: u32,
        character: u32,
    ) -> Result<u32, DispatchError> {
        if pointer == 0 {
            return Err(DispatchError::Unsupported);
        }
        let mut found = 0;
        let character = match self {
            Self::Ansi | Self::Oem | Self::SingleByte => character.to_le_bytes()[0],
        };
        for offset in 0..65536 {
            let address = pointer
                .checked_add(offset)
                .ok_or(MemoryError::AddressOverflow)?;
            let mut byte = [0];
            memory.read(u64::from(address), &mut byte)?;
            if byte[0] == character {
                found = address;
            }
            if byte[0] == 0 {
                return Ok(found);
            }
        }
        Err(DispatchError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::windows::crt::Crt;

    #[test]
    fn crt_selection_is_owned_and_unsupported_values_leave_it_unchanged() {
        let mut first = Crt::default();
        let second = Crt::default();
        assert_eq!(first.multibyte, CodePage::Ansi);
        for (selector, expected) in [
            (-2, CodePage::Oem),
            (437, CodePage::Oem),
            (0, CodePage::SingleByte),
            (-3, CodePage::Ansi),
            (1252, CodePage::Ansi),
        ] {
            assert!(first.multibyte.set(selector).is_ok());
            assert_eq!(first.multibyte, expected);
            assert_eq!(second.multibyte, CodePage::Ansi);
            for invalid in [-4, -1, 1, 932, 65001, 20127, i32::MIN] {
                assert!(matches!(
                    first.multibyte.set(invalid),
                    Err(DispatchError::Unsupported)
                ));
                assert_eq!(first.multibyte, expected);
            }
        }
    }
}
