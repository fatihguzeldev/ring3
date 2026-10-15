use super::{
    User32CallContext32, User32Context32, User32Outcome32,
    callback::Continuation32,
    memory::{self, Scratch32},
};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::{
        user32::{Rect32, User32Api32, User32Error32},
        window::WindowHandle32,
    },
};

impl User32Context32 {
    pub(super) fn get_rect(
        &self,
        api: User32Api32,
        args: &[u32],
        memory: &mut AddressSpace,
    ) -> Result<User32Outcome32, User32Error32> {
        let handle = WindowHandle32::try_from(args[0])?;
        let rect = if api == User32Api32::GetClientRect {
            let extent = self.windows().get(handle)?.description.extent;
            Rect32 {
                left: 0,
                top: 0,
                right: extent.width as i32,
                bottom: extent.height as i32,
            }
        } else {
            self.state.record(handle)?.outer
        };
        memory::write_rect(memory, args[1], rect)?;
        Ok(User32Outcome32::Return(1))
    }

    pub(super) fn get_window_long(&self, args: &[u32]) -> Result<User32Outcome32, User32Error32> {
        let handle = WindowHandle32::try_from(args[0])?;
        let data = self.state.record(handle)?;
        let value = match args[1] as i32 {
            -4 => self.windows().get(handle)?.description.procedure.0,
            -6 => data.instance,
            -8 => super::raw(data.parent),
            -12 => data.menu,
            -16 => data.style,
            -20 => data.ex_style,
            -21 => data.user_data,
            index if index >= 0 => self.state.extra(handle, index as u32)?,
            _ => return Err(User32Error32::Win32(1413)),
        };
        Ok(User32Outcome32::Return(value))
    }

    pub(super) fn set_window_long(
        &mut self,
        args: &[u32],
        memory: &mut AddressSpace,
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        let handle = self.owned(args[0], context.thread_id)?;
        let mut data = self.state.record(handle)?;
        if data.destroying {
            return Err(User32Error32::Win32(1400));
        }
        let value = args[2];
        let old = match args[1] as i32 {
            -4 => {
                if value == 0 {
                    return Err(User32Error32::Win32(87));
                }
                let mut record = self.windows().get(handle)?;
                let old = record.description.procedure.0;
                record.description.procedure = GuestAddress(value);
                self.state
                    .windows_mut()
                    .update_description(handle, record.description)?;
                old
            }
            -6 => std::mem::replace(&mut data.instance, value),
            -12 => std::mem::replace(&mut data.menu, value),
            -21 => std::mem::replace(&mut data.user_data, value),
            index @ (-16 | -20) => {
                self.check_continuation()?;
                let old = if index == -16 {
                    data.style
                } else {
                    data.ex_style
                };
                let mut bytes = [0; 8];
                bytes[..4].copy_from_slice(&old.to_le_bytes());
                bytes[4..].copy_from_slice(&value.to_le_bytes());
                let scratch = Scratch32::allocate(memory, self.scratch_bounds, &bytes)?;
                return self.start(Continuation32::Style {
                    window: handle,
                    index,
                    old,
                    scratch,
                    changed: false,
                });
            }
            -8 => return Err(User32Error32::Win32(50)),
            index if index >= 0 => {
                return Ok(User32Outcome32::Return(self.state.set_extra(
                    handle,
                    index as u32,
                    value,
                )?));
            }
            _ => return Err(User32Error32::Win32(1413)),
        };
        self.state.update(data)?;
        Ok(User32Outcome32::Return(old))
    }

    pub(super) fn apply_style(
        &mut self,
        window: WindowHandle32,
        index: i32,
        value: u32,
    ) -> Result<(), User32Error32> {
        let mut data = self.state.record(window)?;
        let mut record = self.windows().get(window)?;
        if index == -16 {
            data.style = value;
            record.description.visible = value & 0x1000_0000 != 0;
            record.description.enabled = value & 0x0800_0000 == 0;
        } else {
            data.ex_style = value;
        }
        self.state
            .windows_mut()
            .update_description(window, record.description)?;
        self.state.update(data)?;
        Ok(())
    }
}
