use super::{DispatchError, Process32, Register32, synchronization, thread, threads};

impl Process32 {
    pub(super) fn resume_thread(&mut self, handle: u32) -> Result<(), DispatchError> {
        let waited = self.threads.wait_on_resume(handle);
        let event = waited
            .and_then(|handle| self.sync_objects.event(handle))
            .filter(|event| event.signaled);
        let mutex = waited
            .and_then(|handle| self.sync_objects.mutex(handle))
            .filter(|mutex| mutex.depth == 0);
        let completion = if event.is_some() || mutex.is_some() {
            Some(threads::Completion::Signaled)
        } else {
            self.threads
                .expired_on_resume(handle, self.elapsed_nanoseconds.cast_unsigned())
                .then_some(threads::Completion::TimedOut)
        };
        let previous = self.threads.resume(
            handle,
            thread::Teb(self.cpu.fs_base()),
            &self.sync_objects,
            &mut self.memory,
        )?;
        if previous == 1
            && let Some(completion) = completion
        {
            if let Some(mutex) = mutex {
                self.sync_objects
                    .acquire_mutex(mutex.id, self.threads.waiter_id(handle));
            }
            self.threads.complete_wait(handle, completion);
            if let Some(event) = event
                && !event.manual_reset
            {
                self.sync_objects.consume_event(event.id);
            }
        }
        self.cpu.set_register(Register32::Eax, previous);
        Ok(())
    }

    pub(super) fn wait_object(&mut self, args: &[u32]) -> Result<bool, DispatchError> {
        let actor = self
            .threads
            .id(thread::Teb(self.cpu.fs_base()))
            .ok_or(DispatchError::Unsupported)?;
        let blocked = self
            .sync_objects
            .event(args[0])
            .filter(|event| !event.signaled)
            .map(|event| event.id)
            .or_else(|| {
                self.sync_objects
                    .mutex(args[0])
                    .filter(|mutex| mutex.depth != 0 && mutex.owner != actor)
                    .map(|mutex| mutex.id)
            });
        if args[1] != 0
            && let Some(object) = blocked
        {
            if !self.startup.is_complete() || self.modules.loader_owner().is_some() {
                return Err(DispatchError::Unsupported);
            }
            let deadline = (args[1] != u32::MAX)
                .then(|| self.elapsed_nanoseconds.cast_unsigned() + u64::from(args[1]) * 1_000_000);
            self.threads
                .park_wait(&self.cpu, args[0], object, deadline)?;
            return Ok(true);
        }
        self.synchronization(synchronization::Call::Wait, args)?;
        Ok(false)
    }

    pub(super) fn synchronization(
        &mut self,
        call: synchronization::Call,
        args: &[u32],
    ) -> Result<(), DispatchError> {
        let teb = thread::Teb(self.cpu.fs_base());
        let actor = self.threads.id(teb).ok_or(DispatchError::Unsupported)?;
        if matches!(call, synchronization::Call::Close) && self.threads.uses_wait_handle(args[0]) {
            return Err(DispatchError::Unsupported);
        }
        let event = if matches!(call, synchronization::Call::SetEvent) {
            self.sync_objects.event(args[0])
        } else {
            None
        };
        let waiters = event
            .map(|event| self.threads.object_waiters(event.id, event.manual_reset))
            .unwrap_or_default();
        let transfer = if matches!(call, synchronization::Call::ReleaseMutex) {
            self.sync_objects
                .mutex(args[0])
                .filter(|mutex| mutex.owner == actor && mutex.depth == 1)
                .and_then(|mutex| {
                    self.threads
                        .object_waiters(mutex.id, false)
                        .first()
                        .map(|&waiter| (mutex.id, waiter, self.threads.waiter_id(waiter)))
                })
        } else {
            None
        };
        let value = self
            .sync_objects
            .dispatch(call, args, actor, teb, &mut self.memory)?;
        if value == 1
            && let Some((mutex, waiter, owner)) = transfer
        {
            self.sync_objects.acquire_mutex(mutex, owner);
            self.threads
                .complete_wait(waiter, threads::Completion::Signaled);
        }
        if !waiters.is_empty() {
            self.threads.release_waiters(&waiters);
            let event = event.unwrap();
            if !event.manual_reset {
                self.sync_objects.consume_event(event.id);
            }
        }
        self.cpu.set_register(Register32::Eax, value);
        Ok(())
    }
}
