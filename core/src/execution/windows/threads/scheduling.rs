use super::super::{
    Cpu32, DispatchError, GuestMemory, Process32, synchronization::SyncObjects, thread,
};
use super::{State, Threads};

const QUANTUM: u64 = 4096;

#[derive(Default)]
pub(super) struct Schedule {
    active: u32,
    primary: Option<Cpu32>,
    resumed: u32,
    remaining: u64,
    requested: bool,
}

impl Threads {
    pub(super) fn active_teb(&self) -> u32 {
        if self.schedule.active == 0 {
            thread::BASE
        } else {
            self.children[&self.schedule.active].teb
        }
    }

    pub(super) fn scheduling_enabled(&self) -> bool {
        self.schedule.resumed != 0
    }

    pub(in super::super) fn scheduled_child(&self) -> bool {
        self.schedule.active != 0
    }

    pub(in super::super) fn resume(
        &mut self,
        handle: u32,
        teb: thread::Teb,
        handles: &SyncObjects,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if teb.0 != self.active_teb() {
            return Err(DispatchError::Unsupported);
        }
        if handle == u32::MAX - 1 {
            return Ok(0);
        }
        let Some(context) = self
            .children
            .get_mut(&handle)
            .filter(|_| handles.is_thread(handle))
        else {
            teb.set_last_error(memory, 6)?;
            return Ok(u32::MAX);
        };
        let previous = context.suspend_count;
        if previous == 0 {
            return Ok(0);
        }
        context.suspend_count -= 1;
        if previous == 1 {
            if !context.ever_resumed {
                context.ever_resumed = true;
                if self.schedule.resumed == 0 {
                    self.schedule.remaining = QUANTUM;
                }
                self.schedule.resumed += 1;
            }
            self.schedule.requested = true;
        }
        Ok(previous)
    }

    pub(super) fn contexts(&self) -> impl Iterator<Item = (u32, &State)> + Clone {
        std::iter::once((0, &self.primary)).chain(
            self.children
                .iter()
                .filter(|(_, context)| context.ever_resumed)
                .map(|(&handle, context)| (handle, &context.state)),
        )
    }

    fn ready(&self) -> impl Iterator<Item = (u32, &State)> + Clone {
        self.contexts().filter(|(handle, state)| {
            !self.suspended(*handle) && !state.wait.is_some_and(super::waiting::Wait::pending)
        })
    }

    pub(in super::super) fn slice(
        &mut self,
        cpu: &mut Cpu32,
        budget: u64,
        pinned: Option<u32>,
        now: u64,
    ) -> Result<Option<u64>, DispatchError> {
        if self.schedule.resumed == 0 {
            return Ok(Some(budget));
        }
        let teb = self.active_teb();
        if cpu.fs_base() != teb || pinned.is_some_and(|owner| owner != teb) {
            return Err(DispatchError::Unsupported);
        }
        let wait = self.state_mut(teb)?.wait;
        if let Some(wait) = wait {
            wait.check(cpu)?;
        }
        self.expire_waits(now);
        if pinned.is_some() {
            if self.schedule.remaining == 0 {
                self.schedule.remaining = QUANTUM;
            }
            return Ok(Some(budget.min(self.schedule.remaining)));
        }
        let Some(highest) = self
            .ready()
            .map(|(_, state)| state.priority.relative())
            .max()
        else {
            return Ok(None);
        };
        let current = if self.schedule.active == 0 {
            self.primary.priority.relative()
        } else {
            self.children[&self.schedule.active]
                .state
                .priority
                .relative()
        };
        if self.schedule.remaining == 0
            || self.schedule.requested
            || current < highest
            || wait.is_some_and(super::waiting::Wait::pending)
        {
            let next = {
                let mut candidates = self
                    .ready()
                    .filter(|(_, state)| state.priority.relative() == highest)
                    .map(|(handle, _)| handle);
                candidates
                    .clone()
                    .find(|&handle| handle > self.schedule.active)
                    .or_else(|| candidates.next())
                    .unwrap()
            };
            self.switch(cpu, next);
            self.schedule.remaining = QUANTUM;
            self.schedule.requested = false;
        }
        Ok(Some(budget.min(self.schedule.remaining)))
    }

    fn switch(&mut self, cpu: &mut Cpu32, next: u32) {
        if next == self.schedule.active {
            return;
        }
        if self.schedule.active == 0 {
            self.schedule.primary = Some(*cpu);
        } else {
            self.children.get_mut(&self.schedule.active).unwrap().cpu = *cpu;
        }
        *cpu = if next == 0 {
            self.schedule
                .primary
                .expect("primary was saved before switching to a child")
        } else {
            self.children[&next].cpu
        };
        self.schedule.active = next;
    }

    pub(in super::super) fn account(&mut self, count: u64) {
        if self.schedule.resumed != 0 {
            self.schedule.remaining -= count;
        }
    }
}

impl Process32 {
    pub(in super::super) fn schedule_slice(
        &mut self,
        budget: u64,
    ) -> Result<Option<u64>, DispatchError> {
        let pinned = if self.startup.is_complete() {
            self.modules.loader_owner()
        } else {
            Some(thread::BASE)
        };
        self.threads.slice(
            &mut self.cpu,
            budget,
            pinned,
            self.elapsed_nanoseconds.cast_unsigned(),
        )
    }
}
