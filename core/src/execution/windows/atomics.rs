use super::{DispatchError, Process32, Register32, guest};

#[derive(Clone, Copy)]
pub(super) enum Call {
    Exchange,
    Increment,
    Decrement,
}

impl Call {
    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Exchange => 2,
            Self::Increment | Self::Decrement => 1,
        }
    }
}

impl Process32 {
    pub(super) fn interlocked(
        &mut self,
        call: Call,
        arguments: &[u32],
    ) -> Result<(), DispatchError> {
        let target = arguments[0];
        if !target.is_multiple_of(4) {
            return Err(DispatchError::Unsupported);
        }
        let mut previous = [0];
        guest::read_words(&self.memory, target, &mut previous)?;
        let value = match call {
            Call::Exchange => arguments[1],
            Call::Increment => previous[0].wrapping_add(1),
            Call::Decrement => previous[0].wrapping_sub(1),
        };
        guest::write_word(&mut self.memory, target, value)?;
        let result = if matches!(call, Call::Exchange) {
            previous[0]
        } else {
            value
        };
        self.cpu.set_register(Register32::Eax, result);
        Ok(())
    }
}
