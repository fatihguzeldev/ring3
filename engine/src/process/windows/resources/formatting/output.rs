use super::{E, Outcome};
use crate::{
    memory::GuestAddress,
    process::{EngineInstance, HostError},
};

impl EngineInstance {
    pub(super) fn publish_message(
        &mut self,
        wide: bool,
        flags: u32,
        output: u32,
        capacity: u32,
        units: &[u16],
    ) -> Result<Outcome, E> {
        let width = if wide { 2 } else { 1 };
        let mut bytes = if wide {
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(units.len() * 2 + 2)
                .map_err(|_| E::Allocation)?;
            bytes.extend(units.iter().flat_map(|unit| unit.to_le_bytes()));
            bytes
        } else {
            self.windows_ansi_code_page()
                .encode_windows(units, 0, b'?')
                .map_err(|_| E::InvalidParameter)?
                .0
        };
        let length = bytes.len() / width;
        bytes.try_reserve(width).map_err(|_| E::Allocation)?;
        bytes.resize(bytes.len() + width, 0);
        if flags & 0x100 != 0 {
            super::super::memory::output(self.memory.as_ref().ok_or(E::NotConfigured)?, output, 4)?;
            let size = (capacity as usize)
                .max(length + 1)
                .checked_mul(width)
                .filter(|&n| n <= 131072)
                .ok_or(E::Win32(234))?;
            let prepared = self
                .prepare_windows_local_initialized(size as u32)
                .map_err(heap_error)?;
            let pointer = prepared.address().to_le_bytes();
            self.commit_windows_local_initialized(
                prepared,
                &bytes,
                &[(GuestAddress(output), &pointer)],
            )
            .map_err(heap_error)?;
        } else {
            if capacity == 0 {
                return Err(E::Win32(122));
            }
            let count = length.min(capacity as usize - 1);
            bytes.truncate((count + 1) * width);
            bytes[count * width..].fill(0);
            super::super::memory::output(
                self.memory.as_ref().ok_or(E::NotConfigured)?,
                output,
                bytes.len(),
            )?;
            self.memory
                .as_mut()
                .ok_or(E::NotConfigured)?
                .write(GuestAddress(output), &bytes)?;
            if count != length {
                return Ok(Outcome::Return {
                    value: 0,
                    last_error: Some(122),
                });
            }
        }
        Ok((length as u32).into())
    }
}
fn heap_error(error: HostError) -> E {
    match error {
        HostError::WindowsMemory(error) => E::Win32(error.win32()),
        HostError::Memory(error) => E::Memory(error),
        _ => E::Allocation,
    }
}
