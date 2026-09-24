use super::super::{Cpu32, DispatchError, GuestMemory, Process32, Register32, modules};

pub(in super::super) const ENTER: u32 = 0x7000_0ff0;
pub(in super::super) const RETURN: u32 = 0x7000_0fec;

pub(super) struct Entry {
    target: u32,
    stack: u32,
    phase: Phase,
}

enum Phase {
    New,
    Active {
        calls: Vec<modules::Pending>,
        next: usize,
    },
    Complete,
}

impl Entry {
    pub(super) fn new(target: u32, stack: u32) -> Self {
        Self {
            target,
            stack,
            phase: Phase::New,
        }
    }

    fn advance(
        &mut self,
        cpu: &mut Cpu32,
        memory: &mut GuestMemory,
        modules: &mut modules::Modules,
    ) -> Result<(), DispatchError> {
        if cpu.register(Register32::Esp) != self.stack {
            return Err(DispatchError::Unsupported);
        }
        match &mut self.phase {
            Phase::New if cpu.eip == ENTER => {
                let calls = modules.thread_initializers()?;
                if let Some(next) = call_next(cpu, memory, modules, &calls, 0, self.stack)? {
                    modules.start_notifications(cpu.fs_base());
                    self.phase = Phase::Active { calls, next };
                    return Ok(());
                }
            }
            Phase::Active { calls, next } if cpu.eip == RETURN => {
                if modules.notification_owner() != Some(cpu.fs_base()) {
                    return Err(DispatchError::Unsupported);
                }
                if let Some(following) = call_next(cpu, memory, modules, calls, *next, self.stack)?
                {
                    *next = following;
                    return Ok(());
                }
                modules.finish_notifications(cpu.fs_base());
            }
            _ => return Err(DispatchError::Unsupported),
        }
        self.phase = Phase::Complete;
        cpu.eip = self.target;
        Ok(())
    }
}

fn call_next(
    cpu: &mut Cpu32,
    memory: &mut GuestMemory,
    modules: &modules::Modules,
    calls: &[modules::Pending],
    next: usize,
    stack: u32,
) -> Result<Option<usize>, DispatchError> {
    let Some((index, pending)) = calls
        .iter()
        .enumerate()
        .skip(next)
        .find(|(_, pending)| modules.thread_calls_enabled(pending.handle))
    else {
        return Ok(None);
    };
    let mut frame = [0; 16];
    for (bytes, value) in frame
        .chunks_exact_mut(4)
        .zip([RETURN, pending.handle, 2, 0])
    {
        bytes.copy_from_slice(&value.to_le_bytes());
    }
    memory.write(u64::from(stack - 16), &frame)?;
    cpu.set_register(Register32::Esp, stack - 16);
    cpu.eip = pending.entry;
    Ok(Some(index + 1))
}

impl Process32 {
    pub(in super::super) fn advance_thread_entry(&mut self) -> Result<(), DispatchError> {
        if !self.startup.is_complete() {
            return Err(DispatchError::Unsupported);
        }
        let context = self
            .threads
            .suspended
            .values_mut()
            .find(|context| context.teb == self.cpu.fs_base())
            .ok_or(DispatchError::Unsupported)?;
        context
            .entry
            .advance(&mut self.cpu, &mut self.memory, &mut self.modules)
    }
}
