use super::super::{User32CallContext32, User32Context32, User32Outcome32, memory};
use crate::{
    memory::{AddressSpace, GuestAddress},
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
        encode_text(ram, output, capacity, self.state.text(window)?, wide, call)
    }
}
pub(super) fn encode_text(
    ram: &mut AddressSpace,
    output: u32,
    capacity: usize,
    text: &[u16],
    wide: bool,
    call: User32CallContext32,
) -> Result<u32, User32Error32> {
    if capacity == 0 {
        return Ok(0);
    }
    let (bytes, count) = if wide {
        let count = text.len().min(capacity - 1);
        (
            text[..count]
                .iter()
                .copied()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
            count,
        )
    } else {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(text.len().min(capacity - 1) + 1)
            .map_err(|_| User32Error32::Allocation)?;
        for unit in char::decode_utf16(text.iter().copied()) {
            let value = unit.unwrap_or(char::REPLACEMENT_CHARACTER);
            let mut units = [0; 2];
            let encoded = call
                .ansi
                .encode(value.encode_utf16(&mut units), false)
                .map_err(|_| User32Error32::Win32(1113))?
                .0;
            if bytes.len() + encoded.len() >= capacity {
                break;
            }
            bytes.extend_from_slice(&encoded);
        }
        let count = bytes.len();
        bytes.push(0);
        (bytes, count)
    };
    ram.write(GuestAddress(output), &bytes)?;
    Ok(count as u32)
}
