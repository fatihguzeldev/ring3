use crate::{
    abi::arena::TRANSFER_OFFSET,
    process::{EngineInstance, HostError},
};

impl EngineInstance {
    pub(in crate::process) fn read_cr2_request(&mut self) -> Result<(), HostError> {
        self.memory()?;
        let value = self.cr2().ok_or(HostError::InvalidRequest)?;
        self.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
            .copy_from_slice(&value.0.to_le_bytes());
        Ok(())
    }
}

#[cfg(test)]
mod tests;
