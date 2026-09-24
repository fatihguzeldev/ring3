use super::super::{API_BASE, Cpu32, DispatchError, GuestMemory, MemoryError, Register32, guest};
use super::Threads;

#[derive(Clone, Copy)]
pub(super) struct Wait {
    handle: u32,
    event: u32,
    stack: u32,
    deadline: Option<u64>,
    completion: Option<Completion>,
}

#[derive(Clone, Copy)]
pub(in super::super) enum Completion {
    Signaled,
    TimedOut,
}

impl Completion {
    fn value(self) -> u32 {
        match self {
            Self::Signaled => 0,
            Self::TimedOut => 258,
        }
    }
}

impl Wait {
    pub(super) fn pending(self) -> bool {
        self.completion.is_none()
    }

    fn expired(self, now: u64) -> bool {
        self.pending() && self.deadline.is_some_and(|deadline| now >= deadline)
    }

    pub(super) fn check(self, cpu: &Cpu32) -> Result<(), DispatchError> {
        if cpu.eip != API_BASE + 0x214 || cpu.register(Register32::Esp) != self.stack {
            return Err(DispatchError::Unsupported);
        }
        self.stack
            .checked_add(12)
            .ok_or(MemoryError::AddressOverflow)?;
        Ok(())
    }
}

impl Threads {
    pub(in super::super) fn park_event(
        &mut self,
        cpu: &Cpu32,
        handle: u32,
        event: u32,
        deadline: Option<u64>,
    ) -> Result<(), DispatchError> {
        if !self.scheduling_enabled() || cpu.fs_base() != self.active_teb() {
            return Err(DispatchError::Unsupported);
        }
        let wait = Wait {
            handle,
            event,
            stack: cpu.register(Register32::Esp),
            deadline,
            completion: None,
        };
        wait.check(cpu)?;
        let state = self.state_mut(cpu.fs_base())?;
        if state.wait.is_some() {
            return Err(DispatchError::Unsupported);
        }
        state.wait = Some(wait);
        Ok(())
    }

    pub(in super::super) fn event_waiters(&self, event: u32, manual: bool) -> Vec<u32> {
        self.contexts()
            .filter(|(handle, state)| {
                !self.suspended(*handle)
                    && state
                        .wait
                        .is_some_and(|wait| wait.event == event && wait.pending())
            })
            .map(|(handle, _)| handle)
            .take(if manual { usize::MAX } else { 1 })
            .collect()
    }

    pub(in super::super) fn event_on_resume(&self, handle: u32) -> Option<u32> {
        let context = self.children.get(&handle)?;
        let wait = context.state.wait?;
        (context.suspend_count == 1 && wait.pending()).then_some(wait.handle)
    }

    pub(in super::super) fn expired_on_resume(&self, handle: u32, now: u64) -> bool {
        self.children.get(&handle).is_some_and(|context| {
            context.suspend_count == 1 && context.state.wait.is_some_and(|wait| wait.expired(now))
        })
    }

    pub(super) fn expire_waits(&mut self, now: u64) {
        for state in std::iter::once(&mut self.primary).chain(
            self.children
                .values_mut()
                .filter(|context| context.ever_resumed && context.suspend_count == 0)
                .map(|context| &mut context.state),
        ) {
            if let Some(wait) = &mut state.wait
                && wait.expired(now)
            {
                wait.completion = Some(Completion::TimedOut);
            }
        }
    }

    pub(in super::super) fn release_waiters(&mut self, waiters: &[u32]) {
        for &handle in waiters {
            self.complete_wait(handle, Completion::Signaled);
        }
    }

    pub(in super::super) fn complete_wait(&mut self, handle: u32, completion: Completion) {
        let state = if handle == 0 {
            &mut self.primary
        } else {
            &mut self.children.get_mut(&handle).unwrap().state
        };
        let wait = state.wait.as_mut().expect("prepared waiter is retained");
        debug_assert!(wait.pending());
        wait.completion = Some(completion);
    }

    pub(in super::super) fn uses_wait_handle(&self, handle: u32) -> bool {
        self.contexts()
            .any(|(_, state)| state.wait.is_some_and(|wait| wait.handle == handle))
    }

    pub(in super::super) fn finish_wait(
        &mut self,
        cpu: &mut Cpu32,
        memory: &GuestMemory,
    ) -> Result<bool, DispatchError> {
        if !self.scheduling_enabled() {
            return Ok(false);
        }
        let state = self.state_mut(cpu.fs_base())?;
        let Some(wait) = state.wait else {
            return Ok(false);
        };
        wait.check(cpu)?;
        let completion = wait.completion.ok_or(DispatchError::Unsupported)?;
        let mut saved_return = [0];
        guest::read_words(memory, wait.stack, &mut saved_return)?;
        cpu.set_register(Register32::Eax, completion.value());
        cpu.set_register(Register32::Esp, wait.stack + 12);
        cpu.eip = saved_return[0];
        state.wait = None;
        Ok(true)
    }
}
