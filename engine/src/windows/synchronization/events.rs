use super::context::{Event32, Handle32, set};
use super::types::WaitResult32;
use super::{
    EVENT_ALL_ACCESS, EVENT_MODIFY_STATE, SynchronizationContext32,
    SynchronizationError32 as Error, WAIT_FAILED,
};

pub struct PreparedDuplicate32 {
    epoch: u64,
    revision: u64,
    next_revision: u64,
    source: u32,
    duplicate: Option<Handle32>,
    close: bool,
}
pub struct PreparedCloseHandles32 {
    closes: Vec<PreparedDuplicate32>,
}

impl PreparedDuplicate32 {
    pub fn handle(&self) -> Option<u32> {
        self.duplicate.map(|handle| handle.value)
    }
}
impl SynchronizationContext32 {
    pub fn validate_wait_handle(&self, handle: u32) -> Result<(), Error> {
        self.handle_index(handle, super::SYNCHRONIZE).map(|_| ())
    }
    pub fn ensure_unnamed_event_capacity(&self, count: usize) -> Result<(), Error> {
        if self.events.iter().filter(|event| event.is_none()).count() < count
            || self
                .handles
                .len()
                .checked_add(count)
                .is_none_or(|total| total > self.handle_limit)
        {
            return Err(Error::Capacity);
        }
        let count = u64::try_from(count).map_err(|_| Error::Exhausted)?;
        self.next_object
            .checked_add(count)
            .ok_or(Error::Exhausted)?;
        self.revision.checked_add(count).ok_or(Error::Exhausted)?;
        self.next_handle
            .checked_add(
                u32::try_from(count.checked_mul(4).ok_or(Error::Exhausted)?)
                    .map_err(|_| Error::Exhausted)?,
            )
            .filter(|value| *value < 0x8000_0000)
            .ok_or(Error::Exhausted)?;
        Ok(())
    }
    pub fn prepare_close_handles(&self, handles: &[u32]) -> Result<PreparedCloseHandles32, Error> {
        self.revision
            .checked_add(handles.len() as u64)
            .ok_or(Error::Exhausted)?;
        let mut closes = Vec::new();
        closes
            .try_reserve_exact(handles.len())
            .map_err(|_| Error::Allocation)?;
        for (index, &handle) in handles.iter().enumerate() {
            if handles[..index].contains(&handle) {
                return Err(Error::InvalidHandle);
            }
            let mut prepared = self.prepare_duplicate(handle, 0, false, true, true, false)?;
            prepared.revision = self.revision + index as u64;
            prepared.next_revision = prepared.revision + 1;
            closes.push(prepared);
        }
        Ok(PreparedCloseHandles32 { closes })
    }
    pub fn commit_close_handles(&mut self, prepared: PreparedCloseHandles32) {
        for close in prepared.closes {
            self.commit_duplicate(close);
        }
    }
    pub fn has_named_event(&self, name: &[u16]) -> Result<bool, Error> {
        let name = normalize_name(name)?;
        Ok(self
            .events
            .iter()
            .flatten()
            .any(|event| event.name.as_deref() == Some(name)))
    }
    pub fn create_event(
        &mut self,
        manual: bool,
        initial: bool,
        inherit: bool,
        name: Option<&[u16]>,
    ) -> Result<(u32, bool), Error> {
        let name = name.map(normalize_name).transpose()?;
        let value = self.handle_value()?;
        let revision = self.next_revision()?;
        let existing = name.as_ref().and_then(|name| {
            self.events.iter().position(|event| {
                event
                    .as_ref()
                    .is_some_and(|event| event.name.as_deref() == Some(*name))
            })
        });
        let (slot, existed) = if let Some(slot) = existing {
            (slot, true)
        } else {
            let slot = self
                .events
                .iter()
                .position(Option::is_none)
                .ok_or(Error::Capacity)?;
            let id = self.next_object.checked_add(1).ok_or(Error::Exhausted)?;
            let owned_name = name
                .map(|name| {
                    let mut owned = Vec::new();
                    owned
                        .try_reserve_exact(name.len())
                        .map_err(|_| Error::Allocation)?;
                    owned.extend_from_slice(name);
                    Ok::<_, Error>(owned)
                })
                .transpose()?;
            self.events[slot] = Some(Event32 {
                id,
                name: owned_name,
                manual,
                handles: 0,
                bindings: 0,
            });
            self.next_object = id;
            set(&mut self.signaled, slot, initial);
            (slot, false)
        };
        self.events[slot].as_mut().unwrap().handles += 1;
        self.handles.push(Handle32 {
            value,
            event: slot,
            access: EVENT_ALL_ACCESS,
            inherit,
        });
        self.next_handle = value;
        self.revision = revision;
        Ok((value, existed))
    }
    pub fn open_event(&mut self, access: u32, inherit: bool, name: &[u16]) -> Result<u32, Error> {
        let access = event_access(access)?;
        let name = normalize_name(name)?;
        let slot = self
            .events
            .iter()
            .position(|event| {
                event
                    .as_ref()
                    .is_some_and(|event| event.name.as_deref() == Some(name))
            })
            .ok_or(Error::NotFound)?;
        let value = self.handle_value()?;
        let revision = self.next_revision()?;
        self.handles.push(Handle32 {
            value,
            event: slot,
            access,
            inherit,
        });
        self.events[slot].as_mut().unwrap().handles += 1;
        self.next_handle = value;
        self.revision = revision;
        Ok(value)
    }
    pub fn event_is_signaled(&self, handle: u32) -> Result<bool, Error> {
        let index = self.handle_index(handle, 0)?;
        Ok(super::context::bit(
            &self.signaled,
            self.handles[index].event,
        ))
    }
    pub fn handle_inheritable(&self, handle: u32) -> Result<bool, Error> {
        Ok(self.handles[self.handle_index(handle, 0)?].inherit)
    }
    pub fn close_handle(&mut self, handle: u32) -> Result<(), Error> {
        let index = self.handle_index(handle, 0)?;
        let revision = self.next_revision()?;
        self.close_validated(index);
        self.revision = revision;
        Ok(())
    }
    fn close_validated(&mut self, index: usize) {
        let handle = self.handles.remove(index);
        for wait in self.waits.iter_mut().flatten() {
            if wait.result.is_none() && wait.handles[..wait.count].contains(&handle.value) {
                wait.result = Some(WaitResult32 {
                    value: WAIT_FAILED,
                    last_error: Some(6),
                });
            }
        }
        self.events[handle.event].as_mut().unwrap().handles -= 1;
        self.reclaim_event(handle.event);
    }
    pub fn prepare_duplicate(
        &self,
        source: u32,
        desired_access: u32,
        inherit: bool,
        same_access: bool,
        close_source: bool,
        target: bool,
    ) -> Result<PreparedDuplicate32, Error> {
        let source_handle = self.handles[self.handle_index(source, 0)?];
        let access = if same_access {
            source_handle.access
        } else {
            event_access(desired_access)?
        };
        if access & !source_handle.access != 0 {
            return Err(Error::AccessDenied);
        }
        let duplicate = if target {
            Some(Handle32 {
                value: self.handle_value()?,
                event: source_handle.event,
                access,
                inherit,
            })
        } else {
            if !close_source {
                return Err(Error::InvalidHandle);
            }
            None
        };
        Ok(PreparedDuplicate32 {
            epoch: self.epoch,
            revision: self.revision,
            next_revision: self.next_revision()?,
            source,
            duplicate,
            close: close_source,
        })
    }
    pub fn commit_duplicate(&mut self, prepared: PreparedDuplicate32) {
        assert_eq!(
            (self.epoch, self.revision),
            (prepared.epoch, prepared.revision)
        );
        if let Some(handle) = prepared.duplicate {
            self.events[handle.event].as_mut().unwrap().handles += 1;
            self.handles.push(handle);
            self.next_handle = handle.value;
        }
        if prepared.close {
            self.close_validated(self.handle_index(prepared.source, 0).unwrap());
        }
        self.revision = prepared.next_revision;
    }
    pub fn set_event(&mut self, handle: u32) -> Result<(), Error> {
        let index = self.handle_index(handle, EVENT_MODIFY_STATE)?;
        let slot = self.handles[index].event;
        let prepared = self.prepare_signal_slots(&[slot], &[], self.now, None)?;
        self.commit_signals(prepared);
        Ok(())
    }
    pub fn reset_event(&mut self, handle: u32) -> Result<(), Error> {
        let index = self.handle_index(handle, EVENT_MODIFY_STATE)?;
        let revision = self.next_revision()?;
        set(&mut self.signaled, self.handles[index].event, false);
        self.revision = revision;
        Ok(())
    }
    pub fn pulse_event(&mut self, handle: u32) -> Result<(), Error> {
        let index = self.handle_index(handle, EVENT_MODIFY_STATE)?;
        let slot = self.handles[index].event;
        let prepared = self.prepare_signal_slots(&[slot], &[slot], self.now, None)?;
        self.commit_signals(prepared);
        Ok(())
    }
}
fn event_access(access: u32) -> Result<u32, Error> {
    let mut mapped = access & 0x0fff_ffff;
    if access & 0x8000_0000 != 0 {
        mapped |= 0x0002_0001;
    }
    if access & 0x4000_0000 != 0 {
        mapped |= 0x0002_0002;
    }
    if access & 0x2000_0000 != 0 {
        mapped |= 0x0012_0000;
    }
    if access & 0x1000_0000 != 0 {
        mapped |= EVENT_ALL_ACCESS;
    }
    if mapped & !EVENT_ALL_ACCESS != 0 {
        return Err(Error::AccessDenied);
    }
    Ok(mapped)
}
fn normalize_name(name: &[u16]) -> Result<&[u16], Error> {
    if name.is_empty() || name.len() > 260 || name.contains(&0) {
        return Err(Error::InvalidName);
    }
    let local = [
        b'L' as u16,
        b'o' as u16,
        b'c' as u16,
        b'a' as u16,
        b'l' as u16,
        b'\\' as u16,
    ];
    let global = [
        b'G' as u16,
        b'l' as u16,
        b'o' as u16,
        b'b' as u16,
        b'a' as u16,
        b'l' as u16,
        b'\\' as u16,
    ];
    if name.starts_with(&global) {
        return Err(Error::Unsupported);
    }
    let name = name.strip_prefix(&local).unwrap_or(name);
    if name.is_empty() || name.contains(&(b'\\' as u16)) {
        return Err(Error::InvalidName);
    }
    Ok(name)
}
