use super::super::{User32CallContext32, User32Context32, memory};
use crate::{
    memory::AddressSpace,
    windows::user32::{
        User32Error32 as E,
        menus::{MenuHandle32, MenuItem32},
    },
};

impl User32Context32 {
    pub(super) fn legacy_menu_item(
        &self,
        flags: u32,
        id: u32,
        value: u32,
        wide: bool,
        ram: &AddressSpace,
        call: User32CallContext32,
    ) -> Result<MenuItem32, E> {
        let submenu = if flags & 0x10 != 0 {
            let submenu = MenuHandle32::try_from(id)?;
            if !self.menus.contains(submenu) {
                return Err(E::Win32(1401));
            }
            Some(submenu)
        } else {
            None
        };
        let special = flags & (4 | 0x100 | 0x800) != 0;
        let text = if value == 0 || special {
            Vec::new()
        } else {
            memory::string(ram, value, wide, 32_767, call.ansi)?
        };
        Ok(MenuItem32 {
            id: if flags & 0x800 != 0 { 0 } else { id },
            kind: flags & 0x6b64,
            state: flags & 0x108b,
            submenu,
            data: if flags & 0x100 != 0 { value } else { 0 },
            bitmap: if flags & 4 != 0 { value } else { 0 },
            ..MenuItem32::command(0, text)
        })
    }
}
