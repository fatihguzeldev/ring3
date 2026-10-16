use super::{super::User32Context32, memory};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::{
        rawinput::{KEYBOARD, MOUSE},
        user32::User32Error32 as E,
    },
};
impl User32Context32 {
    pub(super) fn raw_input_device_list(
        &self,
        args: &[u32],
        mem: &mut AddressSpace,
    ) -> Result<u32, E> {
        if args[2] != 8 {
            return Err(E::Win32(87));
        }
        let available = memory::dword(mem, args[1])?;
        if args[0] == 0 {
            memory::size(mem, args[1], 2)?;
            return Ok(0);
        }
        if available < 2 {
            memory::size(mem, args[1], 2)?;
            return Err(E::Win32(122));
        }
        memory::output(mem, args[0], 16)?;
        let mut bytes = [0; 16];
        for (index, value) in [MOUSE, 0, KEYBOARD, 1].into_iter().enumerate() {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        mem.write(GuestAddress(args[0]), &bytes)?;
        Ok(2)
    }
    pub(super) fn raw_input_device_info(
        &self,
        args: &[u32],
        mem: &mut AddressSpace,
        wide: bool,
    ) -> Result<u32, E> {
        let keyboard = match args[0] {
            KEYBOARD => true,
            MOUSE => false,
            _ => return Err(E::Win32(6)),
        };
        let available = memory::dword(mem, args[3])? as usize;
        let mut fixed = [0; 64];
        let (length, required) = match args[1] {
            0x2000_0007 => {
                let name = if keyboard {
                    r"\\?\RING3#Keyboard#0"
                } else {
                    r"\\?\RING3#Mouse#0"
                };
                if wide {
                    for (index, unit) in name.encode_utf16().chain([0]).enumerate() {
                        fixed[index * 2..index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
                    }
                    (2 * (name.len() + 1), name.len() + 1)
                } else {
                    fixed[..name.len()].copy_from_slice(name.as_bytes());
                    (name.len() + 1, name.len() + 1)
                }
            }
            0x2000_000b => {
                let values = if keyboard {
                    [32, 1, 4, 0, 1, 12, 3, 101]
                } else {
                    [32, 0, 1, 5, 0, 0, 0, 0]
                };
                for (index, value) in values.into_iter().enumerate() {
                    fixed[index * 4..index * 4 + 4].copy_from_slice(&u32::to_le_bytes(value));
                }
                (32, 32)
            }
            0x2000_0005 => return Err(E::Win32(87)),
            _ => return Err(E::Win32(87)),
        };
        if args[2] == 0 {
            memory::size(mem, args[3], required as u32)?;
            return Ok(0);
        }
        if available < required {
            memory::size(mem, args[3], required as u32)?;
            return Err(E::Win32(122));
        }
        memory::output(mem, args[2], length)?;
        if args[1] == 0x2000_000b && memory::dword(mem, args[2])? != 32 {
            return Err(E::Win32(87));
        }
        mem.write(GuestAddress(args[2]), &fixed[..length])?;
        Ok(required as u32)
    }
}
