use super::super::{
    User32Context32,
    callback::CreateStage32,
    memory::{self, Scratch32},
};
use super::*;
use crate::{
    memory::AddressSpace,
    windows::{
        controls::{ControlKind32, static_image_kind},
        user32::{Point32, Rect32},
        window::WindowHandle32,
    },
};
impl User32Context32 {
    pub(in crate::process::windows::user32) fn creation_hook(
        &self,
        thread: u32,
        window: WindowHandle32,
        scratch: Scratch32,
    ) -> Option<HookCall32> {
        self.hooks
            .first(thread, HookKind32::Cbt)
            .map(|hook| HookCall32 {
                kind: HookKind32::Cbt,
                handle: hook.handle,
                thread,
                procedure: hook.procedure,
                arguments: [3, window.get(), scratch.address.0 + 64],
            })
    }
    pub(in crate::process::windows::user32) fn apply_creation_hook(
        &mut self,
        window: WindowHandle32,
        thread: u32,
        scratch: Scratch32,
        memory: &mut AddressSpace,
    ) -> Result<CreateStage32, User32Error32> {
        scratch.check(memory)?;
        let [structure, insert_after] = memory::words(memory, scratch.address.0 + 64)?;
        if structure != scratch.address.0 {
            return Err(User32Error32::Win32(87));
        }
        if insert_after != 0 {
            return Err(User32Error32::Win32(50));
        }
        let values: [u32; 12] = memory::words(memory, structure)?;
        let mut data = self.state.record(window)?;
        if values[2] != data.menu || (values[8] ^ data.style) & 0x4000_0000 != 0 {
            return Err(User32Error32::Win32(50));
        }
        if values[1] != data.instance {
            return Err(User32Error32::Win32(50));
        }
        let class = self.state.window_class(window)?;
        let (name, atom) = super::super::classes::class_name(
            memory,
            values[10],
            data.unicode,
            self.runtime_input.ansi,
        )?;
        if !atom.is_some_and(|atom| atom == class.atom)
            && crate::windows::text::ordinal_key(&name)
                != crate::windows::text::ordinal_key(&class.name)
        {
            return Err(User32Error32::Win32(50));
        }
        let child = values[8] & 0x4000_0000 != 0;
        let parent = if values[3] == 0 {
            None
        } else {
            let parent = if child {
                self.owned(values[3], thread)?
            } else {
                let parent = WindowHandle32::try_from(values[3])?;
                self.windows().get(parent)?;
                parent
            };
            if parent == window || self.state.destroy_tree_contains(parent, window) {
                return Err(User32Error32::Win32(87));
            }
            Some(if child {
                parent
            } else {
                self.state.top_level(parent)?
            })
        };
        if parent.is_some_and(|parent| {
            self.pending_destroy_contains(parent)
                || self.state.record(parent).is_ok_and(|data| data.destroying)
        }) {
            return Err(User32Error32::Win32(1400));
        }
        if child && parent.is_none() {
            return Err(User32Error32::Win32(1406));
        }
        if !child
            && values[2] != 0
            && !self
                .menus
                .contains(crate::windows::user32::menus::MenuHandle32::try_from(
                    values[2],
                )?)
        {
            return Err(User32Error32::Win32(1401));
        }
        let origin = if child {
            self.state.record(parent.unwrap())?.client_origin
        } else {
            Point32::default()
        };
        let x = (values[7] as i32)
            .checked_add(origin.x)
            .ok_or(User32Error32::Win32(87))?;
        let y = (values[6] as i32)
            .checked_add(origin.y)
            .ok_or(User32Error32::Win32(87))?;
        let width = values[5] as i32;
        let height = values[4] as i32;
        if width < 0 || height < 0 {
            return Err(User32Error32::Win32(87));
        }
        let outer = Rect32 {
            left: x,
            top: y,
            right: x.checked_add(width).ok_or(User32Error32::Win32(87))?,
            bottom: y.checked_add(height).ok_or(User32Error32::Win32(87))?,
        };
        let image_control = self.state.standard_kind(data.class_atom)
            == Some(ControlKind32::Static)
            && static_image_kind(values[8]).is_some();
        if self.state.standard_kind(data.class_atom) == Some(ControlKind32::Static)
            && values[8] & 31 == 15
        {
            return Err(User32Error32::Win32(50));
        }
        let caption = if values[9] == 0 || image_control {
            Vec::new()
        } else {
            memory::string(
                memory,
                values[9],
                data.unicode,
                32767,
                self.runtime_input.ansi,
            )?
        };
        let delta = self.state.check_move_children(window, Point32 { x, y })?;
        memory::write_rect(memory, scratch.address.0 + 48, outer)?;
        let mut description = self.windows().get(window)?.description;
        description.extent = outer.extent()?;
        description.enabled = values[8] & 0x0800_0000 == 0;
        data.parent = parent;
        data.menu = values[2];
        data.style = values[8];
        data.ex_style = values[11];
        data.outer = outer;
        data.client_origin = Point32 { x, y };
        self.state
            .windows_mut()
            .update_description(window, description)?;
        self.state.update(data)?;
        self.state.set_text(window, caption)?;
        self.state.move_children(window, delta);
        Ok(CreateStage32::NonClientCreate)
    }
}
