use super::{DispatchError, Process32, Register32, guest};

impl Process32 {
    pub(super) fn interlocked_exchange(&mut self, arguments: &[u32]) -> Result<(), DispatchError> {
        let target = arguments[0];
        if !target.is_multiple_of(4) {
            return Err(DispatchError::Unsupported);
        }
        let mut previous = [0];
        guest::read_words(&self.memory, target, &mut previous)?;
        guest::write_word(&mut self.memory, target, arguments[1])?;
        self.cpu.set_register(Register32::Eax, previous[0]);
        Ok(())
    }
}
