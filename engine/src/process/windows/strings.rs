use crate::{
    loader::ModuleError32,
    memory::GuestAddress,
    process::{EngineInstance, HostError},
};

impl EngineInstance {
    pub(super) fn windows_string(
        &self,
        pointer: u32,
        wide: bool,
        limit: usize,
    ) -> Result<String, HostError> {
        if pointer == 0 {
            return Err(HostError::Module(ModuleError32::InvalidName));
        }
        let mut units = Vec::new();
        units
            .try_reserve(limit)
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        for index in 0..=limit {
            let address = pointer
                .checked_add((index * if wide { 2 } else { 1 }) as u32)
                .ok_or(HostError::Module(ModuleError32::InvalidName))?;
            let mut bytes = [0; 2];
            self.memory()?
                .read(
                    GuestAddress(address),
                    &mut bytes[..if wide { 2 } else { 1 }],
                )
                .map_err(HostError::Memory)?;
            let unit = u16::from_le_bytes(bytes);
            if unit == 0 {
                return String::from_utf16(&units)
                    .map_err(|_| HostError::Module(ModuleError32::InvalidName));
            }
            if index == limit {
                break;
            }
            units.push(unit);
        }
        Err(HostError::Module(ModuleError32::InvalidName))
    }
}
