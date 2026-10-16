use super::context::Binding32;
use super::{
    EVENT_MODIFY_STATE, EventBinding32, SynchronizationContext32, SynchronizationError32 as Error,
};

pub struct PreparedIoSignals32 {
    signals: super::PreparedSignals32,
    release: Option<PreparedEventBindings32>,
    close: Option<super::PreparedCloseHandles32>,
}

pub struct PreparedEventBindings32 {
    epoch: u64,
    revision: u64,
    next_revision: u64,
    release: Vec<EventBinding32>,
    bindings: Vec<EventBinding32>,
    reset: Vec<usize>,
}
impl PreparedEventBindings32 {
    pub fn bindings(&self) -> &[EventBinding32] {
        &self.bindings
    }
}
impl SynchronizationContext32 {
    pub fn prepare_thread_exit_with_signals(
        &self,
        thread: u32,
        bindings: &[EventBinding32],
    ) -> Result<PreparedIoSignals32, Error> {
        self.prepare_thread_exit_with_events(thread, bindings, &[])
    }
    pub fn prepare_thread_exit_with_events(
        &self,
        thread: u32,
        bindings: &[EventBinding32],
        handles: &[u32],
    ) -> Result<PreparedIoSignals32, Error> {
        let signals = self.prepare_thread_exit(thread)?;
        self.prepare_exit_events(signals, bindings, handles)
    }
    pub fn prepare_process_exit_with_events(
        &self,
        process: u32,
        bindings: &[EventBinding32],
        handles: &[u32],
    ) -> Result<PreparedIoSignals32, Error> {
        let signals = self.prepare_process_exit(process)?;
        self.prepare_exit_events(signals, bindings, handles)
    }
    fn prepare_exit_events(
        &self,
        mut signals: super::PreparedSignals32,
        bindings: &[EventBinding32],
        handles: &[u32],
    ) -> Result<PreparedIoSignals32, Error> {
        for binding in bindings {
            self.prepare_occurrence(&mut signals, *binding, 1)?;
        }
        let release = if bindings.is_empty() {
            None
        } else {
            let mut release = self.prepare_event_bindings(&[], bindings)?;
            release.revision = release.next_revision;
            release.next_revision = release
                .next_revision
                .checked_add(1)
                .ok_or(Error::Exhausted)?;
            Some(release)
        };
        let close = if handles.is_empty() {
            None
        } else {
            let mut close = self.prepare_close_handles(handles)?;
            close.shift_revisions(1 + u64::from(release.is_some()))?;
            Some(close)
        };
        Ok(PreparedIoSignals32 {
            signals,
            release,
            close,
        })
    }
    pub fn prepare_signals_and_release(
        &self,
        bindings: &[EventBinding32],
    ) -> Result<PreparedIoSignals32, Error> {
        let signals = self.prepare_signals(bindings)?;
        if bindings.is_empty() {
            return Ok(PreparedIoSignals32 {
                signals,
                release: None,
                close: None,
            });
        }
        let mut release = self.prepare_event_bindings(&[], bindings)?;
        release.revision = release.next_revision;
        release.next_revision = release
            .next_revision
            .checked_add(1)
            .ok_or(Error::Exhausted)?;
        Ok(PreparedIoSignals32 {
            signals,
            release: Some(release),
            close: None,
        })
    }
    pub fn commit_signals_and_release(&mut self, prepared: PreparedIoSignals32) {
        self.commit_signals(prepared.signals);
        if let Some(release) = prepared.release {
            self.commit_event_bindings(release);
        }
        if let Some(close) = prepared.close {
            self.commit_close_handles(close);
        }
    }
    pub fn prepare_event_bindings(
        &self,
        handles: &[u32],
        release: &[EventBinding32],
    ) -> Result<PreparedEventBindings32, Error> {
        let mut old = Vec::new();
        old.try_reserve_exact(release.len())
            .map_err(|_| Error::Allocation)?;
        for binding in release {
            self.validate_binding(*binding)?;
            old.push(*binding);
        }
        old.sort_unstable_by_key(|binding| binding.slot);
        if old.windows(2).any(|pair| pair[0].slot == pair[1].slot) {
            return Err(Error::Stale);
        }
        let available = self
            .bindings
            .iter()
            .filter(|binding| binding.is_none())
            .count()
            + old.len();
        if handles.len() > available {
            return Err(Error::Capacity);
        }
        self.next_binding
            .checked_add(handles.len() as u64)
            .ok_or(Error::Exhausted)?;
        let mut bindings = Vec::new();
        bindings
            .try_reserve_exact(handles.len())
            .map_err(|_| Error::Allocation)?;
        let mut slots = self
            .bindings
            .iter()
            .enumerate()
            .filter(|(slot, binding)| {
                binding.is_none()
                    || old
                        .binary_search_by_key(slot, |binding| binding.slot)
                        .is_ok()
            })
            .map(|(slot, _)| slot);
        for (index, handle) in handles.iter().enumerate() {
            let event = self.handles[self.handle_index(*handle, EVENT_MODIFY_STATE)?].event;
            self.require_event(event)?;
            bindings.push(EventBinding32 {
                epoch: self.epoch,
                id: self.next_binding + index as u64 + 1,
                slot: slots.next().unwrap(),
                object: self.objects[event].as_ref().unwrap().id,
                event,
            });
        }
        Ok(PreparedEventBindings32 {
            epoch: self.epoch,
            revision: self.revision,
            next_revision: self.next_revision()?,
            release: old,
            bindings,
            reset: Vec::new(),
        })
    }
    pub fn prepare_io_events(
        &self,
        handles: &[u32],
        release: &[EventBinding32],
        reset: &[u32],
    ) -> Result<PreparedEventBindings32, Error> {
        let mut prepared = self.prepare_event_bindings(handles, release)?;
        prepared
            .reset
            .try_reserve_exact(reset.len())
            .map_err(|_| Error::Allocation)?;
        for handle in reset {
            let event = self.handles[self.handle_index(*handle, EVENT_MODIFY_STATE)?].event;
            self.require_event(event)?;
            if !prepared.reset.contains(&event) {
                prepared.reset.push(event);
            }
        }
        Ok(prepared)
    }
    pub fn commit_event_bindings(
        &mut self,
        prepared: PreparedEventBindings32,
    ) -> Vec<EventBinding32> {
        assert_eq!(
            (self.epoch, self.revision),
            (prepared.epoch, prepared.revision)
        );
        for old in &prepared.release {
            self.bindings[old.slot] = None;
            self.objects[old.event].as_mut().unwrap().bindings -= 1;
        }
        for binding in &prepared.bindings {
            self.bindings[binding.slot] = Some(Binding32 {
                id: binding.id,
                event: binding.event,
                object: binding.object,
            });
            self.objects[binding.event].as_mut().unwrap().bindings += 1;
            self.next_binding = binding.id;
        }
        for event in &prepared.reset {
            super::context::set(&mut self.signaled, *event, false);
        }
        for old in &prepared.release {
            self.reclaim_event(old.event);
        }
        self.revision = prepared.next_revision;
        prepared.bindings
    }
    pub fn retain_event(&mut self, handle: u32) -> Result<EventBinding32, Error> {
        let prepared = self.prepare_event_bindings(&[handle], &[])?;
        Ok(self.commit_event_bindings(prepared)[0])
    }
    pub fn release_event(&mut self, binding: EventBinding32) -> Result<(), Error> {
        let prepared = self.prepare_event_bindings(&[], &[binding])?;
        self.commit_event_bindings(prepared);
        Ok(())
    }
    pub fn validate_event_binding(&self, binding: EventBinding32) -> Result<(), Error> {
        self.validate_binding(binding).map(|_| ())
    }
    pub(super) fn validate_binding(&self, binding: EventBinding32) -> Result<usize, Error> {
        if binding.epoch != self.epoch {
            return Err(Error::Stale);
        }
        let record = self
            .bindings
            .get(binding.slot)
            .and_then(Option::as_ref)
            .ok_or(Error::Stale)?;
        if record.id != binding.id
            || record.object != binding.object
            || record.event != binding.event
            || self.objects[binding.event]
                .as_ref()
                .is_none_or(|event| event.id != binding.object)
        {
            return Err(Error::Stale);
        }
        Ok(record.event)
    }
}
