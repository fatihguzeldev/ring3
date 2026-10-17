use super::*;
use crate::memory::GuestAddress;

impl User32Context32 {
    pub(super) fn toolbar_insert(
        &mut self,
        window: WindowHandle32,
        index: usize,
        count: usize,
        address: u32,
        wide: bool,
        ram: &AddressSpace,
        call: User32CallContext32,
    ) -> Result<u32, E> {
        if count > MAX_BUTTONS {
            return Err(E::Capacity);
        }
        let mut buttons = Vec::new();
        let mut strings = Vec::new();
        buttons
            .try_reserve_exact(count)
            .map_err(|_| E::Allocation)?;
        strings
            .try_reserve_exact(count)
            .map_err(|_| E::Allocation)?;
        let model = self.toolbar(window)?;
        if model
            .buttons
            .len()
            .checked_add(count)
            .is_none_or(|n| n > MAX_BUTTONS)
        {
            return Err(E::Capacity);
        }
        let mut text_units = model.buttons.iter().map(|b| b.text.len()).sum::<usize>();
        let mut pool_units = model.strings.iter().map(Vec::len).sum::<usize>();
        for i in 0..count {
            let at = address
                .checked_add((i as u32).checked_mul(20).ok_or(E::Capacity)?)
                .ok_or(E::Win32(87))?;
            let [bitmap, command, flags, data, value] = memory::words::<5>(ram, at)?;
            let style = (flags >> 8) as u8;
            let (string, text) = if style & 1 != 0 || value == u32::MAX {
                (u32::MAX, Vec::new())
            } else if value < 0x10000 {
                let text = model
                    .strings
                    .get(value as usize)
                    .ok_or(E::Win32(87))?
                    .clone();
                (value, text)
            } else {
                let text = memory::string(ram, value, wide, 1 << 20, call.ansi)?;
                let value =
                    u32::try_from(model.strings.len() + strings.len()).map_err(|_| E::Capacity)?;
                pool_units = pool_units
                    .checked_add(text.len())
                    .filter(|n| *n <= 1 << 20)
                    .ok_or(E::Capacity)?;
                strings.push(text.clone());
                (value, text)
            };
            text_units = text_units
                .checked_add(text.len())
                .filter(|n| *n <= 1 << 20)
                .ok_or(E::Capacity)?;
            buttons.push(ToolbarButton32 {
                key: 0,
                bitmap: bitmap as i32,
                command,
                state: flags as u8,
                style,
                data,
                string,
                text,
            });
        }
        self.state.check_paint_change()?;
        self.toolbar_mut(window)?
            .insert_with_strings(index, buttons, strings)?;
        self.state
            .invalidate_control(window)
            .expect("toolbar mutation preflighted");
        Ok(1)
    }
    pub(super) fn toolbar_add_strings(
        &mut self,
        window: WindowHandle32,
        address: u32,
        wide: bool,
        ram: &AddressSpace,
        call: User32CallContext32,
    ) -> Result<u32, E> {
        let mut strings = Vec::new();
        let mut bytes = Vec::new();
        let mut offset = 0u32;
        // advance by captured bytes, never by re-encoding decoded dbcs text.
        loop {
            if offset >= 2 * (1 << 20) {
                return Err(E::Capacity);
            }
            let at = address.checked_add(offset).ok_or(E::Win32(87))?;
            let mut unit = [0u8; 2];
            let width = if wide { 2 } else { 1 };
            ram.read(GuestAddress(at), &mut unit[..width])?;
            offset += width as u32;
            if unit == [0, 0] {
                if bytes.is_empty() {
                    break;
                }
                let text = if wide {
                    bytes
                        .chunks_exact(2)
                        .map(|b| u16::from_le_bytes([b[0], b[1]]))
                        .collect()
                } else {
                    call.ansi
                        .decode(&bytes, false)
                        .map_err(|_| E::Win32(1113))?
                };
                if strings.len() == MAX_BUTTONS {
                    return Err(E::Capacity);
                }
                strings.try_reserve(1).map_err(|_| E::Allocation)?;
                strings.push(text);
                bytes.clear();
            } else {
                bytes.try_reserve(width).map_err(|_| E::Allocation)?;
                bytes.extend_from_slice(&unit[..width]);
            }
        }
        Ok(self.toolbar_mut(window)?.add_strings(strings)? as u32)
    }
}
