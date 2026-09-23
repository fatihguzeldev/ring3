use super::{
    Cpu32, DispatchError, GuestMemory, MemoryError, Process32, Register32, creation, desktop,
    dialogs, guest, modules, thread,
};

pub(super) const RETURN: u32 = 0x7000_0ff8;
const MAX_DEPTH: usize = 64;

impl Process32 {
    pub(super) fn dispatch_message(&mut self, address: u32) -> Result<bool, DispatchError> {
        let mut message = [0; 8];
        guest::read_words(&self.memory, address, &mut message)?;
        if message[1] > u16::MAX.into() || (message[1] == 0x113 && message[3] != 0) {
            return Err(DispatchError::Unsupported);
        }
        if message[0] == 0 {
            self.cpu.set_register(Register32::Eax, 0);
            return Ok(false);
        }
        let Some(window) = self.desktop.window(message[0]) else {
            return Err(DispatchError::Unsupported);
        };
        if window.procedure == 0 {
            return Err(DispatchError::Unsupported);
        }
        let stack = self.cpu.register(Register32::Esp);
        self.callbacks.enter(
            &mut self.cpu,
            &mut self.memory,
            Frame {
                stack,
                caller: stack,
                cleanup: 8,
                creation: None,
                cbt_hook: None,
                module: None,
                dialog: None,
            },
            window.procedure,
            &message[..4],
        )?;
        Ok(true)
    }

    pub(super) fn send_message(&mut self, args: &[u32]) -> Result<bool, DispatchError> {
        if matches!(args[0], desktop::DESKTOP | 0xffff | u32::MAX) {
            return Err(DispatchError::Unsupported);
        }
        let Some(window) = self.desktop.window(args[0]) else {
            thread::set_last_error(&mut self.memory, 1400)?;
            self.cpu.set_register(Register32::Eax, 0);
            return Ok(false);
        };
        if window.procedure == 0 {
            if window.parent != 0
                && window.dialog_units.is_some()
                && (0x80..=0x85).contains(&window.class)
                && args[1] == 0x364
                && args[2..] == [0, 0]
            {
                self.cpu.set_register(Register32::Eax, 0);
                return Ok(false);
            }
            return Err(DispatchError::Unsupported);
        }
        let stack = self.cpu.register(Register32::Esp);
        self.callbacks.enter(
            &mut self.cpu,
            &mut self.memory,
            Frame {
                stack,
                caller: stack,
                cleanup: 20,
                creation: None,
                cbt_hook: None,
                module: None,
                dialog: None,
            },
            window.procedure,
            args,
        )?;
        Ok(true)
    }
}

#[derive(Default)]
pub(super) struct Callbacks {
    frames: Vec<Frame>,
}

#[derive(Clone, Copy)]
pub(super) struct Frame {
    pub(super) stack: u32,
    pub(super) caller: u32,
    pub(super) cleanup: u32,
    pub(super) creation: Option<creation::Pending>,
    pub(super) cbt_hook: Option<u32>,
    pub(super) module: Option<modules::Pending>,
    pub(super) dialog: Option<dialogs::Pending>,
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
                creation: None,
                cbt_hook: None,
                module: None,
                dialog: None,
            },
            arguments[0],
            &arguments[1..],
        )
    }

    pub(super) fn active_cbt(&self) -> Option<u32> {
        self.frames.iter().rev().find_map(|frame| frame.cbt_hook)
    }

    pub(super) fn enter(
        &mut self,
        cpu: &mut Cpu32,
        memory: &mut GuestMemory,
        frame: Frame,
        procedure: u32,
        arguments: &[u32],
    ) -> Result<(), DispatchError> {
        self.check_entry(procedure)?;
        let callback_stack = write_frame(memory, frame.stack, arguments)?;
        self.frames.push(frame);
        cpu.set_register(Register32::Esp, callback_stack);
        cpu.eip = procedure;
        Ok(())
    }

    pub(super) fn check_entry(&self, procedure: u32) -> Result<(), DispatchError> {
        if self.frames.len() == MAX_DEPTH || procedure == RETURN {
            return Err(DispatchError::Unsupported);
        }
        Ok(())
    }

    pub(super) fn replace(
        &mut self,
        cpu: &mut Cpu32,
        memory: &mut GuestMemory,
        frame: Frame,
        procedure: u32,
        arguments: &[u32],
    ) -> Result<(), DispatchError> {
        self.current(cpu)?;
        if procedure == RETURN {
            return Err(DispatchError::Unsupported);
        }
        let callback_stack = write_frame(memory, frame.stack, arguments)?;
        *self.frames.last_mut().expect("validated current frame") = frame;
        cpu.set_register(Register32::Esp, callback_stack);
        cpu.eip = procedure;
        Ok(())
    }

    pub(super) fn current(&self, cpu: &Cpu32) -> Result<Frame, DispatchError> {
        let frame = *self.frames.last().ok_or(DispatchError::Unsupported)?;
        if cpu.register(Register32::Esp) != frame.stack {
            return Err(DispatchError::Unsupported);
        }
        Ok(frame)
    }

    pub(super) fn finish(
        &mut self,
        cpu: &mut Cpu32,
        memory: &GuestMemory,
    ) -> Result<(), DispatchError> {
        let frame = self.current(cpu)?;
        let mut saved_return = [0];
        guest::read_words(memory, frame.caller, &mut saved_return)?;
        self.frames.pop();
        cpu.set_register(Register32::Esp, frame.caller.wrapping_add(frame.cleanup));
        cpu.eip = saved_return[0];
        Ok(())
    }
}

fn write_frame(
    memory: &mut GuestMemory,
    stack: u32,
    arguments: &[u32],
) -> Result<u32, DispatchError> {
    if arguments.len() > 4 {
        return Err(DispatchError::Unsupported);
    }
    let length = (arguments.len() + 1) * 4;
    let callback_stack = stack
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
    Ok(callback_stack)
}
