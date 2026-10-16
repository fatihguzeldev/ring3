use super::*;
type Error = SynchronizationError32;

pub struct PreparedObjectHandle32 {
    epoch: u64,
    revision: u64,
    next_revision: u64,
    slot: usize,
    object: Option<Object32>,
    handle: Handle32,
}
impl PreparedObjectHandle32 {
    pub fn handle(&self) -> u32 {
        self.handle.value
    }
}
impl SynchronizationContext32 {
    pub fn set_thread_handle_limit(&mut self, limit: usize) -> Result<(), Error> {
        if limit == 0 || limit > 65536 {
            return Err(Error::InvalidLimits);
        }
        if self.thread_handle_count() > limit {
            return Err(Error::Capacity);
        }
        self.thread_handle_limit = limit;
        Ok(())
    }
    pub fn thread_handle_count(&self) -> usize {
        self.handles
            .iter()
            .filter(|handle| {
                matches!(
                    self.objects[handle.event].as_ref().unwrap().kind,
                    ObjectKind32::Thread { .. }
                )
            })
            .count()
    }
    pub fn create_mutex(
        &mut self,
        initial_owner: bool,
        inherit: bool,
        name: Option<&[u16]>,
    ) -> Result<(u32, bool), Error> {
        let (prepared, existed) = self.prepare_mutex(initial_owner, inherit, name)?;
        Ok((self.commit_object_handle(prepared), existed))
    }
    pub fn prepare_mutex(
        &self,
        initial_owner: bool,
        inherit: bool,
        name: Option<&[u16]>,
    ) -> Result<(PreparedObjectHandle32, bool), Error> {
        let owner = if initial_owner {
            self.current_thread
        } else {
            0
        };
        self.prepare_object(
            ObjectKind32::Mutex {
                owner,
                depth: u32::from(initial_owner),
                abandoned: false,
            },
            false,
            inherit,
            name,
            0x001f_0001,
        )
    }
    pub fn open_mutex(&mut self, access: u32, inherit: bool, name: &[u16]) -> Result<u32, Error> {
        self.open_object(access, inherit, name, 1)
    }
    pub fn create_semaphore(
        &mut self,
        initial: i32,
        maximum: i32,
        inherit: bool,
        name: Option<&[u16]>,
    ) -> Result<(u32, bool), Error> {
        let (prepared, existed) = self.prepare_semaphore(initial, maximum, inherit, name)?;
        Ok((self.commit_object_handle(prepared), existed))
    }
    pub fn prepare_semaphore(
        &self,
        initial: i32,
        maximum: i32,
        inherit: bool,
        name: Option<&[u16]>,
    ) -> Result<(PreparedObjectHandle32, bool), Error> {
        if maximum <= 0 || initial < 0 || initial > maximum {
            return Err(Error::InvalidWait);
        }
        self.prepare_object(
            ObjectKind32::Semaphore {
                count: initial as u32,
                maximum: maximum as u32,
            },
            false,
            inherit,
            name,
            0x001f_0003,
        )
    }
    pub fn open_semaphore(
        &mut self,
        access: u32,
        inherit: bool,
        name: &[u16],
    ) -> Result<u32, Error> {
        self.open_object(access, inherit, name, 2)
    }
    pub fn has_thread_object(&self, thread: u32) -> bool {
        self.objects
            .iter()
            .flatten()
            .any(|object| matches!(object.kind,ObjectKind32::Thread {id,..} if id==thread))
    }
    pub fn prepare_thread_handle(
        &self,
        thread: u32,
        inherit: bool,
    ) -> Result<PreparedObjectHandle32, Error> {
        self.prepare_open_thread(thread, 0x001f_ffff, inherit, false)
    }
    pub fn prepare_open_thread(
        &self,
        thread: u32,
        access: u32,
        inherit: bool,
        terminated: bool,
    ) -> Result<PreparedObjectHandle32, Error> {
        if thread == 0 {
            return Err(Error::InvalidHandle);
        }
        if self.thread_handle_count() >= self.thread_handle_limit {
            return Err(Error::Capacity);
        }
        if let Some(slot) = self.objects.iter().position(|object| {
            object.as_ref().is_some_and(
                |object| matches!(object.kind,ObjectKind32::Thread {id,..} if id==thread),
            )
        }) {
            let access = object_access(slot, self, access)?;
            return self.prepare_existing_object(slot, access, inherit);
        }
        let access = super::thread_access(access)?;
        Ok(self
            .prepare_object(
                ObjectKind32::Thread {
                    id: thread,
                    terminated,
                },
                true,
                inherit,
                None,
                access,
            )?
            .0)
    }
    pub fn validate_thread_access(&self, handle: u32, access: u32) -> Result<u32, Error> {
        let slot = self.handles[self.handle_index(handle, access)?].event;
        match self.objects[slot].as_ref().unwrap().kind {
            ObjectKind32::Thread { id, .. } => Ok(id),
            _ => Err(Error::InvalidHandle),
        }
    }
    pub fn open_thread(&mut self, thread: u32, access: u32, inherit: bool) -> Result<u32, Error> {
        let slot = self
            .objects
            .iter()
            .position(|object| {
                object.as_ref().is_some_and(
                    |object| matches!(object.kind,ObjectKind32::Thread {id,..} if id==thread),
                )
            })
            .ok_or(Error::InvalidHandle)?;
        let access = object_access(slot, self, access)?;
        let prepared = self.prepare_existing_object(slot, access, inherit)?;
        Ok(self.commit_object_handle(prepared))
    }
    pub fn commit_object_handle(&mut self, prepared: PreparedObjectHandle32) -> u32 {
        assert_eq!(
            (self.epoch, self.revision),
            (prepared.epoch, prepared.revision)
        );
        if let Some(object) = prepared.object {
            self.typed_objects += 1;
            self.next_object = object.id;
            let signaled = match object.kind {
                ObjectKind32::Event => false,
                ObjectKind32::Timer { .. } => false,
                ObjectKind32::Mutex { owner, .. } => owner == 0,
                ObjectKind32::Semaphore { count, .. } => count != 0,
                ObjectKind32::Thread { terminated, .. } => terminated,
            };
            set(&mut self.signaled, prepared.slot, signaled);
            self.objects[prepared.slot] = Some(object);
        }
        self.objects[prepared.slot].as_mut().unwrap().handles += 1;
        self.next_handle = prepared.handle.value;
        self.handles.push(prepared.handle);
        self.revision = prepared.next_revision;
        self.next_handle
    }
    pub(in crate::windows::synchronization) fn prepare_object(
        &self,
        kind: ObjectKind32,
        manual: bool,
        inherit: bool,
        name: Option<&[u16]>,
        access: u32,
    ) -> Result<(PreparedObjectHandle32, bool), Error> {
        let name = name.map(normalize_name).transpose()?;
        if let Some(slot) = name.and_then(|name| {
            self.objects.iter().position(|object| {
                object
                    .as_ref()
                    .is_some_and(|object| object.name.as_deref() == Some(name))
            })
        }) {
            if std::mem::discriminant(&self.objects[slot].as_ref().unwrap().kind)
                != std::mem::discriminant(&kind)
            {
                return Err(Error::InvalidHandle);
            }
            return Ok((self.prepare_existing_object(slot, access, inherit)?, true));
        }
        let slot = self
            .objects
            .iter()
            .position(Option::is_none)
            .ok_or(Error::Capacity)?;
        let id = self.next_object.checked_add(1).ok_or(Error::Exhausted)?;
        let mut owned = None;
        if let Some(name) = name {
            let mut units = Vec::new();
            units
                .try_reserve_exact(name.len())
                .map_err(|_| Error::Allocation)?;
            units.extend_from_slice(name);
            owned = Some(units);
        }
        let handle = Handle32 {
            value: self.handle_value()?,
            event: slot,
            access,
            inherit,
        };
        Ok((
            PreparedObjectHandle32 {
                epoch: self.epoch,
                revision: self.revision,
                next_revision: self.next_revision()?,
                slot,
                object: Some(Object32 {
                    id,
                    name: owned,
                    manual,
                    handles: 0,
                    bindings: 0,
                    kind,
                }),
                handle,
            },
            false,
        ))
    }
    fn prepare_existing_object(
        &self,
        slot: usize,
        access: u32,
        inherit: bool,
    ) -> Result<PreparedObjectHandle32, Error> {
        self.objects[slot]
            .as_ref()
            .ok_or(Error::InvalidHandle)?
            .handles
            .checked_add(1)
            .ok_or(Error::Exhausted)?;
        Ok(PreparedObjectHandle32 {
            epoch: self.epoch,
            revision: self.revision,
            next_revision: self.next_revision()?,
            slot,
            object: None,
            handle: Handle32 {
                value: self.handle_value()?,
                event: slot,
                access,
                inherit,
            },
        })
    }
    pub(in crate::windows::synchronization) fn open_object(
        &mut self,
        access: u32,
        inherit: bool,
        name: &[u16],
        kind: u8,
    ) -> Result<u32, Error> {
        let name = normalize_name(name)?;
        let slot = self
            .objects
            .iter()
            .position(|object| {
                object
                    .as_ref()
                    .is_some_and(|object| object.name.as_deref() == Some(name))
            })
            .ok_or(Error::NotFound)?;
        let correct = matches!(
            (kind, self.objects[slot].as_ref().unwrap().kind),
            (1, ObjectKind32::Mutex { .. })
                | (2, ObjectKind32::Semaphore { .. })
                | (3, ObjectKind32::Timer { .. })
        );
        if !correct {
            return Err(Error::InvalidHandle);
        }
        let access = object_access(slot, self, access)?;
        let prepared = self.prepare_existing_object(slot, access, inherit)?;
        Ok(self.commit_object_handle(prepared))
    }
}
