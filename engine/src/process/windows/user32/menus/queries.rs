use super::super::User32Context32;
use crate::windows::user32::{User32Api32 as Api, User32Error32 as E, menus::MenuHandle32};

impl User32Context32 {
    pub(super) fn menu_query(&self, api: Api, args: &[u32]) -> Result<u32, E> {
        let menu = MenuHandle32::try_from(args[0])?;
        if api == Api::GetMenuItemCount {
            return Ok(self.menus.count(menu)? as u32);
        }
        let item = self.menus.item(
            menu,
            args[1],
            api != Api::GetMenuState || args[2] & 0x400 != 0,
        )?;
        Ok(match api {
            Api::GetSubMenu => item.submenu.map_or(0, MenuHandle32::get),
            Api::GetMenuItemID => {
                if item.submenu.is_some() {
                    u32::MAX
                } else {
                    item.id
                }
            }
            Api::GetMenuState => {
                let popup = if let Some(submenu) = item.submenu {
                    ((self.menus.count(submenu)? as u32 & 0xff) << 8) | 0x10
                } else {
                    0
                };
                item.kind | item.state | popup
            }
            _ => return Err(E::InvalidRequest),
        })
    }
}
