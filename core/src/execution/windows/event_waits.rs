use super::{DispatchError, Process32, Register32, synchronization, thread};

impl Process32 {
    pub(super) fn wait_event(&mut self, args: &[u32]) -> Result<bool, DispatchError> {
        if args[1] == u32::MAX
            && let Some(event) = self.sync_objects.event(args[0])
            && !event.signaled
        {
            if !self.startup.is_complete() || self.modules.loader_owner().is_some() {
                return Err(DispatchError::Unsupported);
            }
            self.threads.park_event(&self.cpu, args[0], event.id)?;
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
            .map(|event| self.threads.event_waiters(event.id, event.manual_reset))
            .unwrap_or_default();
        let value = self
            .sync_objects
            .dispatch(call, args, actor, teb, &mut self.memory)?;
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
