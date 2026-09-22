use super::{Cpu32, DispatchError, GuestMemory, MemoryError, Register32, guest};

pub(super) const RETURN: u32 = 0x7000_0ff8;
const MAX_DEPTH: usize = 64;

#[derive(Default)]
pub(super) struct Callbacks {
    frames: Vec<Frame>,
}

#[derive(Clone, Copy)]
pub(super) struct Frame {
    pub(super) stack: u32,
    pub(super) caller: u32,
    pub(super) cleanup: u32,
}

impl Callbacks {
    pub(super) fn start(
        &mut self,
        cpu: &mut Cpu32,
        memory: &mut GuestMemory,
        arguments: &[u32],
    ) -> Result<(), DispatchError> {
        let stack = cpu.register(Register32::Esp);
        self.enter(
            cpu,
            memory,
            Frame {
                stack,
                caller: stack,
                cleanup: 24,
            },
            arguments[0],
            &arguments[1..],
        )
    }

    pub(super) fn enter(
        &mut self,
        cpu: &mut Cpu32,
        memory: &mut GuestMemory,
        frame: Frame,
        procedure: u32,
        arguments: &[u32],
    ) -> Result<(), DispatchError> {
        if self.frames.len() == MAX_DEPTH || procedure == RETURN || arguments.len() > 4 {
            return Err(DispatchError::Unsupported);
        }
        let length = (arguments.len() + 1) * 4;
        let callback_stack = frame
            .stack
            .checked_sub(u32::try_from(length).expect("small callback frame"))
            .ok_or(MemoryError::AddressOverflow)?;
        let mut data = [0; 20];
        for (bytes, value) in data[..length]
            .chunks_exact_mut(4)
            .zip(std::iter::once(&RETURN).chain(arguments))
        {
            bytes.copy_from_slice(&value.to_le_bytes());
        }
        memory.write(u64::from(callback_stack), &data[..length])?;
        self.frames.push(frame);
        cpu.set_register(Register32::Esp, callback_stack);
        cpu.eip = procedure;
        Ok(())
    }

    pub(super) fn finish(
        &mut self,
        cpu: &mut Cpu32,
        memory: &GuestMemory,
    ) -> Result<(), DispatchError> {
        let frame = *self.frames.last().ok_or(DispatchError::Unsupported)?;
        if cpu.register(Register32::Esp) != frame.stack {
            return Err(DispatchError::Unsupported);
        }
        let mut saved_return = [0];
        guest::read_words(memory, frame.caller, &mut saved_return)?;
        self.frames.pop();
        cpu.set_register(Register32::Esp, frame.caller.wrapping_add(frame.cleanup));
        cpu.eip = saved_return[0];
        Ok(())
    }
}
