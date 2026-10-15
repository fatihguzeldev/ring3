use super::context::Binding32;
use super::{
    EVENT_MODIFY_STATE, EventBinding32, SynchronizationContext32, SynchronizationError32 as Error,
};

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
            bindings.push(EventBinding32 {
                epoch: self.epoch,
                id: self.next_binding + index as u64 + 1,
                slot: slots.next().unwrap(),
                object: self.events[event].as_ref().unwrap().id,
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
            self.events[old.event].as_mut().unwrap().bindings -= 1;
        }
        for binding in &prepared.bindings {
            self.bindings[binding.slot] = Some(Binding32 {
                id: binding.id,
                event: binding.event,
                object: binding.object,
            });
            self.events[binding.event].as_mut().unwrap().bindings += 1;
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
            || self.events[binding.event]
                .as_ref()
                .is_none_or(|event| event.id != binding.object)
        {
            return Err(Error::Stale);
        }
        Ok(record.event)
    }
}
