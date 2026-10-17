use super::types::*;
use crate::{
    memory::{AddressSpace, PAGE_SIZE},
    process::windows::user32::{
        User32CallContext32, User32Context32, User32Outcome32,
        callback::Continuation32,
        memory::{self, Scratch32},
    },
    windows::{
        user32::{User32Error32 as E, menus::MenuHandle32},
        window::WindowHandle32,
    },
};

impl User32Context32 {
    pub(in crate::process::windows::user32) fn begin_menu_bar_frame(
        &mut self,
        raw: u32,
        assign: Option<u32>,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        let window = WindowHandle32::try_from(raw)?;
        self.windows().get(window)?;
        let mut data = self.state.record(window)?;
        if data.style & 0x4000_0000 != 0 || data.destroying {
            return Err(E::Win32(87));
        }
        let menu = assign.unwrap_or(data.menu);
        if menu != 0 {
            let handle = MenuHandle32::try_from(menu)?;
            self.menus.count(handle)?;
        }
        self.menu_bar_capacity()?;
        let range = ram.find_free_range(self.scratch_bounds, 1, PAGE_SIZE)?;
        let address = range.first as u32 * PAGE_SIZE;
        let mut bytes = [0; 128];
        for (index, value) in [
            raw,
            0,
            data.outer.left as u32,
            data.outer.top as u32,
            (data.outer.right - data.outer.left) as u32,
            (data.outer.bottom - data.outer.top) as u32,
            0x37,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[80..84].copy_from_slice(&address.to_le_bytes());
        let scratch = Scratch32::allocate(ram, self.scratch_bounds, &bytes)?;
        debug_assert_eq!(scratch.address.0, address);
        if assign.is_some() && data.menu != menu {
            data.menu = menu;
            self.state.update(data).expect("preflighted menu window");
            self.state
                .clear_menu_bar_cache(window)
                .expect("preflighted menu window");
        }
        self.start(Continuation32::MenuBar(MenuBar32 {
            window,
            menu,
            call,
            scratch,
            purpose: Purpose32::Frame,
            stage: Stage32::Changing,
            child: None,
            completed: None,
            measure_revision: 0,
            measure_padding: 0,
            dc: None,
        }))
    }

    pub(in crate::process::windows::user32) fn begin_menu_bar_default(
        &mut self,
        arguments: &[u32],
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<Option<User32Outcome32>, E> {
        if !matches!(arguments[1], 0x83 | 0x85) {
            return Ok(None);
        }
        let window = self.owned(arguments[0], call.thread_id)?;
        let data = self.state.record(window)?;
        if data.style & 0x4000_0000 != 0 || data.menu == 0 {
            return Ok(None);
        }
        let menu = MenuHandle32::try_from(data.menu)?;
        self.menus.count(menu)?;
        let purpose = if arguments[1] == 0x83 {
            let base = self.default_client_rect(window, memory::rect(ram, arguments[3])?)?;
            Purpose32::NonClient {
                output: RectOutput32::new(ram, arguments[3])?,
                base,
            }
        } else {
            Purpose32::Paint
        };
        self.menu_bar_capacity()?;
        let scratch = Scratch32::allocate(ram, self.scratch_bounds, &[0; 128])?;
        Ok(Some(self.start(Continuation32::MenuBar(MenuBar32 {
            window,
            menu: menu.get(),
            call,
            scratch,
            purpose,
            stage: Stage32::Measure(0),
            child: None,
            completed: None,
            measure_revision: 0,
            measure_padding: 0,
            dc: None,
        }))?))
    }

    fn menu_bar_capacity(&mut self) -> Result<(), E> {
        self.next_token.checked_add(2).ok_or(E::Capacity)?;
        self.prepare_thread_slots()?;
        if self.pending.len() + 2 > self.continuation_limit {
            return Err(E::Capacity);
        }
        Ok(())
    }

    pub(in crate::process::windows::user32) fn menu_item_rect(
        &self,
        args: &[u32],
        ram: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        let menu = MenuHandle32::try_from(args[1])?;
        self.menus.item(menu, args[2], true)?;
        let rect = if self.menus.popup(menu)? {
            self.popup_menu_item_rect(menu, args[2] as usize)?
        } else {
            let window = WindowHandle32::try_from(args[0])?;
            let data = self.state.record(window)?;
            let cache = self
                .state
                .menu_bar_cache(window)?
                .filter(|cache| {
                    data.menu == menu.get()
                        && cache.menu == menu
                        && cache.revision == self.menus.revision()
                })
                .ok_or(E::Win32(1401))?;
            cache
                .layout
                .as_ref()
                .ok_or(E::Win32(1401))?
                .screen_item(args[2] as usize, cache.origin)?
        };
        memory::write_rect(ram, args[3], rect)?;
        Ok(User32Outcome32::Return(1))
    }
}
