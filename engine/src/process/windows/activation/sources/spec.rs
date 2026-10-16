use super::super::memory;
use crate::{
    loader::ResourceIdentifier32,
    memory::AddressSpace,
    windows::{activation::ActivationError32 as E, text::CodePage32},
};

pub(in crate::process::windows::activation) struct CreateSource32 {
    pub(in crate::process::windows::activation) source: Option<Vec<u16>>,
    pub(in crate::process::windows::activation) resource: Option<ResourceIdentifier32>,
    pub(in crate::process::windows::activation) module: Option<u32>,
    pub(in crate::process::windows::activation) language: u16,
    pub(in crate::process::windows::activation) assembly_directory: Option<Vec<u16>>,
    pub(in crate::process::windows::activation) application: Option<Vec<u16>>,
    pub(in crate::process::windows::activation) process_default: bool,
}
impl CreateSource32 {
    pub(in crate::process::windows::activation) fn read(
        memory: &AddressSpace,
        address: u32,
        wide: bool,
        cp: CodePage32,
        language: u16,
    ) -> Result<Self, E> {
        let size = u32::from_le_bytes(memory::read(memory, address)?) as usize;
        if !(12..=32).contains(&size) {
            return Err(E::InvalidParameter);
        }
        let mut bytes = [0; 32];
        memory
            .read(crate::memory::GuestAddress(address), &mut bytes[..size])
            .map_err(|_| E::NoAccess)?;
        let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let flags = word(4);
        if flags & !0xbf != 0
            || [(1, 14), (2, 16), (4, 20), (8, 24), (0x20, 28), (0x80, 32)]
                .iter()
                .any(|&(flag, required)| flags & flag != 0 && size < required)
        {
            return Err(E::InvalidParameter);
        }
        if flags & 1 != 0 && u16::from_le_bytes(bytes[12..14].try_into().unwrap()) != 0 {
            return Err(E::IdentityMismatch);
        }
        let source = if word(8) == 0 {
            None
        } else {
            Some(memory::string(memory, word(8), wide, cp)?)
        };
        let module = if flags & 0x80 != 0 {
            if flags & 8 == 0 {
                return Err(E::Win32(1813));
            }
            if word(28) == 0 {
                return Err(E::InvalidHandle);
            }
            Some(word(28))
        } else {
            None
        };
        if source.is_none() && module.is_none() {
            return Err(E::InvalidParameter);
        }
        let optional = |offset| {
            if word(offset) == 0 {
                Err(E::InvalidParameter)
            } else {
                memory::string(memory, word(offset), wide, cp)
            }
        };
        Ok(Self {
            source,
            resource: if flags & 8 != 0 {
                if word(20) == 0 {
                    return Err(E::InvalidParameter);
                }
                Some(memory::identifier(memory, word(20), wide, cp)?)
            } else {
                None
            },
            module,
            language: if flags & 2 != 0 {
                u16::from_le_bytes(bytes[14..16].try_into().unwrap())
            } else {
                language
            },
            assembly_directory: if flags & 4 != 0 {
                Some(optional(16)?)
            } else {
                None
            },
            application: if flags & 0x20 != 0 {
                Some(optional(24)?)
            } else {
                None
            },
            process_default: flags & 0x10 != 0,
        })
    }
}
