use super::super::{User32CallContext32, User32Context32, User32Outcome32, memory};
use crate::{
    memory::AddressSpace,
    windows::{user32::User32Error32, window::WindowHandle32},
};
impl User32Context32 {
    pub(in crate::process::windows::user32) fn window_text_message(
        &mut self,
        args: &[u32],
        wide: bool,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<Option<User32Outcome32>, User32Error32> {
        if !matches!(args[1], 0x0c..=0x0e) {
            return Ok(None);
        }
        let window = self.owned(args[0], call.thread_id)?;
        let value = match args[1] {
            0x0c => {
                let text = if args[3] == 0 {
                    Vec::new()
                } else {
                    memory::string(ram, args[3], wide, 32_767, call.ansi)?
                };
                self.state.set_text(window, text)?;
                let state = self.state.control(window)?;
                state.selection = (0, 0);
                state.modified = false;
                1
            }
            0x0d => self.copy_control_text(window, args[3], args[2] as usize, wide, ram, call)?,
            _ => {
                let text = self.state.text(window)?;
                if wide {
                    text.len() as u32
                } else {
                    call.ansi
                        .encode(text, false)
                        .map_err(|_| User32Error32::Win32(1113))?
                        .0
                        .len() as u32
                }
            }
        };
        Ok(Some(User32Outcome32::Return(value)))
    }
    pub(super) fn copy_control_text(
        &self,
        window: WindowHandle32,
        output: u32,
        capacity: usize,
        wide: bool,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<u32, User32Error32> {
        memory::encode_text(ram, output, capacity, self.state.text(window)?, wide, call)
    }
}
