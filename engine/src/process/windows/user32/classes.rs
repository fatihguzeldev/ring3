use super::{User32CallContext32, User32Context32, User32Outcome32, memory};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::user32::{User32Api32, User32Error32, WindowClass32},
};

impl User32Context32 {
    pub(super) fn register_class(
        &mut self,
        api: User32Api32,
        address: u32,
        memory: &AddressSpace,
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        let extended = matches!(
            api,
            User32Api32::RegisterClassExA | User32Api32::RegisterClassExW
        );
        let values = if extended {
            let input: [u32; 12] = memory::words(memory, address)?;
            if input[0] != 48 {
                return Err(User32Error32::Win32(87));
            }
            [
                input[1], input[2], input[3], input[4], input[5], input[6], input[7], input[8],
                input[9], input[10],
            ]
        } else {
            memory::words::<10>(memory, address)?
        };
        if values[2] != 0 || values[3] > 4096 || values[1] == 0 {
            return Err(User32Error32::Win32(87));
        }
        let class = WindowClass32 {
            atom: 0,
            name: memory::string(memory, values[9], api.wide(), 255)?,
            instance: if values[4] == 0 {
                context.instance
            } else {
                values[4]
            },
            procedure: GuestAddress(values[1]),
            unicode: api.wide(),
            style: values[0],
            extra_bytes: values[3],
        };
        Ok(User32Outcome32::Return(u32::from(
            self.state.register_class(class)?,
        )))
    }

    pub(super) fn unregister_class(
        &mut self,
        api: User32Api32,
        args: &[u32],
        memory: &AddressSpace,
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        let (name, atom) = class_name(memory, args[0], api.wide())?;
        self.state.unregister_class(
            if args[1] == 0 {
                context.instance
            } else {
                args[1]
            },
            &name,
            atom,
        )?;
        Ok(User32Outcome32::Return(1))
    }
}

pub(super) fn class_name(
    memory: &AddressSpace,
    address: u32,
    wide: bool,
) -> Result<(Vec<u16>, Option<u16>), User32Error32> {
    if address <= 0xffff {
        if address == 0 {
            return Err(User32Error32::Win32(87));
        }
        Ok((Vec::new(), Some(address as u16)))
    } else {
        Ok((memory::string(memory, address, wide, 255)?, None))
    }
}
