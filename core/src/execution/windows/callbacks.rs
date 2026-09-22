use super::{Cpu32, DispatchError, GuestMemory, MemoryError, Register32, guest};

pub(super) const RETURN: u32 = 0x7000_0ff8;
const MAX_DEPTH: usize = 64;

#[derive(Default)]
pub(super) struct Callbacks {
    stacks: Vec<u32>,
}

impl Callbacks {
    pub(super) fn start(
        &mut self,
        cpu: &mut Cpu32,
        memory: &mut GuestMemory,
        arguments: &[u32],
    ) -> Result<(), DispatchError> {
        if self.stacks.len() == MAX_DEPTH || arguments[0] == RETURN {
            return Err(DispatchError::Unsupported);
        }
        let stack = cpu.register(Register32::Esp);
        let callback_stack = stack.checked_sub(20).ok_or(MemoryError::AddressOverflow)?;
        let mut frame = [0; 20];
        for (bytes, value) in frame
            .chunks_exact_mut(4)
            .zip(std::iter::once(&RETURN).chain(&arguments[1..]))
        {
            bytes.copy_from_slice(&value.to_le_bytes());
        }
        memory.write(u64::from(callback_stack), &frame)?;
        self.stacks.push(stack);
        cpu.set_register(Register32::Esp, callback_stack);
        cpu.eip = arguments[0];
        Ok(())
    }

    pub(super) fn finish(
        &mut self,
        cpu: &mut Cpu32,
        memory: &GuestMemory,
    ) -> Result<(), DispatchError> {
        let stack = *self.stacks.last().ok_or(DispatchError::Unsupported)?;
        if cpu.register(Register32::Esp) != stack {
            return Err(DispatchError::Unsupported);
        }
        let mut saved_return = [0];
        guest::read_words(memory, stack, &mut saved_return)?;
        self.stacks.pop();
        cpu.set_register(Register32::Esp, stack.wrapping_add(24));
        cpu.eip = saved_return[0];
        Ok(())
    }
}
