use crate::execution::Access;

use super::super::{DispatchError, GuestMemory, guest, system};

const HANDLE_BASE: u32 = 0x5000_0000;

#[derive(Default)]
pub(super) struct Brushes {
    colors: [Option<u32>; 31],
}

impl Brushes {
    pub(super) fn system(&mut self, index: u32) -> Result<u32, DispatchError> {
        let Some(slot) = self.colors.get_mut(index as usize) else {
            return Ok(0);
        };
        let color = system::color(index)?;
        *slot = Some(color);
        Ok(HANDLE_BASE + index * 4)
    }

    pub(super) fn color(&self, handle: u32) -> Option<u32> {
        let offset = handle.checked_sub(HANDLE_BASE)?;
        if !offset.is_multiple_of(4) {
            return None;
        }
        self.colors.get((offset / 4) as usize).copied().flatten()
    }

    pub(super) fn get(
        &self,
        handle: u32,
        count: u32,
        output: u32,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        let Some(color) = self.color(handle) else {
            return Ok(0);
        };
        if output == 0 {
            return Ok(12);
        }
        if count > i32::MAX as u32 {
            return Err(DispatchError::Unsupported);
        }
        let size = count.min(12);
        if size != 0 {
            let mut fields = [0; 12];
            fields[4..8].copy_from_slice(&color.to_le_bytes());
            guest::check(memory, output, size as usize, Access::Write)?;
            memory.write(u64::from(output), &fields[..size as usize])?;
        }
        Ok(size)
    }
}
