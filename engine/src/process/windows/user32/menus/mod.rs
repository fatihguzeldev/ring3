mod layout;
mod popup;
mod visual;
pub(super) use popup::PopupContext32;
mod information;
mod legacy;
mod queries;
mod resources;
pub(super) use resources::PreparedResource32;

use super::{User32CallContext32, User32Context32, User32Outcome32};
use crate::{
    memory::AddressSpace,
    windows::user32::{User32Api32 as Api, User32Error32 as E, menus::MenuHandle32},
};

impl User32Context32 {
    pub(super) fn menu_call(
        &mut self,
        api: Api,
        args: &[u32],
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        use Api::*;
        let value = match api {
            CreateMenu | CreatePopupMenu => self.menus.create(api == CreatePopupMenu)?.get(),
            DestroyMenu => {
                self.menus.destroy(MenuHandle32::try_from(args[0])?)?;
                1
            }
            IsMenu => u32::from(
                MenuHandle32::try_from(args[0]).is_ok_and(|menu| self.menus.contains(menu)),
            ),
            AppendMenuA | AppendMenuW => {
                let menu = MenuHandle32::try_from(args[0])?;
                let position = self.menus.count(menu)?;
                let item =
                    self.legacy_menu_item(args[1], args[2], args[3], api.wide(), ram, call)?;
                self.menus.insert(menu, position, item)?;
                1
            }
            InsertMenuA | InsertMenuW | ModifyMenuA | ModifyMenuW => {
                let menu = MenuHandle32::try_from(args[0])?;
                let item =
                    self.legacy_menu_item(args[2], args[3], args[4], api.wide(), ram, call)?;
                if matches!(api, ModifyMenuA | ModifyMenuW) {
                    self.menus
                        .replace(menu, args[1], args[2] & 0x400 != 0, item)?;
                } else {
                    let (menu, offset) =
                        match self.menus.location(menu, args[1], args[2] & 0x400 != 0) {
                            Ok(location) => location,
                            Err(E::Win32(1456)) => (menu, self.menus.count(menu)?),
                            Err(error) => return Err(error),
                        };
                    self.menus.insert(menu, offset, item)?;
                }
                1
            }
            GetMenu | SetMenu => {
                let window = crate::windows::window::WindowHandle32::try_from(args[0])?;
                self.windows().get(window)?;
                let mut data = self.state.record(window)?;
                if data.style & 0x4000_0000 != 0 {
                    return Err(E::Win32(87));
                }
                if api == GetMenu {
                    data.menu
                } else {
                    if args[1] != 0 && !self.menus.contains(MenuHandle32::try_from(args[1])?) {
                        return Err(E::Win32(1401));
                    }
                    data.menu = args[1];
                    self.state.update(data)?;
                    1
                }
            }
            GetMenuContextHelpId => self.menus.help(MenuHandle32::try_from(args[0])?)?,
            SetMenuContextHelpId => {
                self.menus
                    .set_help(MenuHandle32::try_from(args[0])?, args[1])?;
                1
            }
            GetMenuStringA | GetMenuStringW => {
                let item = self.menus.item(
                    MenuHandle32::try_from(args[0])?,
                    args[1],
                    args[4] & 0x400 != 0,
                )?;
                if item.kind & (4 | 0x100 | 0x800) != 0 {
                    0
                } else if args[2] == 0 || args[3] == 0 {
                    if api.wide() {
                        item.text.len() as u32
                    } else {
                        call.ansi
                            .encode(&item.text, false)
                            .map_err(|_| E::Win32(1113))?
                            .0
                            .len() as u32
                    }
                } else {
                    if args[3] as i32 <= 0 {
                        return Err(E::Win32(87));
                    }
                    super::memory::encode_text(
                        ram,
                        args[2],
                        args[3] as usize,
                        &item.text,
                        api.wide(),
                        call,
                    )?
                }
            }
            GetMenuItemInfoA | GetMenuItemInfoW | SetMenuItemInfoA | SetMenuItemInfoW
            | InsertMenuItemA | InsertMenuItemW => {
                self.menu_item_information(api, args, ram, call)?
            }
            RemoveMenu | DeleteMenu => {
                self.menus.remove(
                    MenuHandle32::try_from(args[0])?,
                    args[1],
                    args[2] & 0x400 != 0,
                    api == DeleteMenu,
                )?;
                1
            }
            EnableMenuItem | CheckMenuItem => {
                let mask = if api == EnableMenuItem { 3 } else { 8 };
                self.menus.set_state(
                    MenuHandle32::try_from(args[0])?,
                    args[1],
                    args[2] & 0x400 != 0,
                    mask,
                    args[2],
                )? & mask
            }
            GetMenuItemCount | GetMenuItemID | GetSubMenu | GetMenuState => {
                self.menu_query(api, args)?
            }
            _ => return Err(E::InvalidRequest),
        };
        Ok(User32Outcome32::Return(value))
    }
}
