use super::*;
type Error = SynchronizationError32;
pub struct PreparedMutexRelease32 {
    epoch: u64,
    revision: u64,
    next: u64,
    slot: usize,
    kind: ObjectKind32,
    plan: Option<PreparedSignals32>,
}
impl PreparedMutexRelease32 {
    pub fn owner_depth(&self) -> (u32, u32) {
        let kind = self
            .plan
            .as_ref()
            .and_then(|plan| {
                plan.objects
                    .iter()
                    .find(|(slot, _)| *slot == self.slot)
                    .map(|(_, kind)| *kind)
            })
            .unwrap_or(self.kind);
        let ObjectKind32::Mutex { owner, depth, .. } = kind else {
            unreachable!();
        };
        (owner, depth)
    }
}

impl SynchronizationContext32 {
    pub fn prepare_release_semaphore(
        &self,
        handle: u32,
        count: i32,
    ) -> Result<(PreparedSignals32, u32), Error> {
        let slot = self.handles[self.handle_index(handle, 2)?].event;
        let ObjectKind32::Semaphore {
            count: previous,
            maximum,
        } = self.objects[slot].as_ref().unwrap().kind
        else {
            return Err(Error::InvalidHandle);
        };
        if count <= 0 {
            return Err(Error::InvalidWait);
        }
        if previous
            .checked_add(count as u32)
            .is_none_or(|next| next > maximum)
        {
            return Err(Error::TooManyPosts);
        }
        let mut prepared = self.signal_plan_typed(self.now, None)?;
        *self.projected_kind_mut(&mut prepared, slot) = ObjectKind32::Semaphore {
            count: previous + count as u32,
            maximum,
        };
        set(&mut prepared.signaled, slot, true);
        self.resolve_waiters(&mut prepared)?;
        Ok((prepared, previous))
    }
    pub fn release_semaphore(&mut self, handle: u32, count: i32) -> Result<u32, Error> {
        let (prepared, previous) = self.prepare_release_semaphore(handle, count)?;
        self.commit_signals(prepared);
        Ok(previous)
    }
    pub fn release_mutex(&mut self, handle: u32) -> Result<(), Error> {
        let prepared = self.prepare_release_mutex(handle)?;
        self.commit_release_mutex(prepared);
        Ok(())
    }
    pub fn mutex_state(&self, handle: u32) -> Result<(u32, u32), Error> {
        let slot = self.handles[self.handle_index(handle, 0)?].event;
        let ObjectKind32::Mutex { owner, depth, .. } = self.objects[slot].as_ref().unwrap().kind
        else {
            return Err(Error::InvalidHandle);
        };
        Ok((owner, depth))
    }
    pub fn object_waiters(&self, handle: u32) -> Result<usize, Error> {
        let slot = self.handles[self.handle_index(handle, 0)?].event;
        Ok(self
            .waits
            .iter()
            .flatten()
            .filter(|wait| wait.result.is_none() && wait.events[..wait.count].contains(&slot))
            .count())
    }
    pub fn try_acquire_mutex(&mut self, handle: u32) -> Result<bool, Error> {
        let slot = self.handles[self.handle_index(handle, SYNCHRONIZE)?].event;
        let ObjectKind32::Mutex { owner, depth, .. } = self.objects[slot].as_ref().unwrap().kind
        else {
            return Err(Error::InvalidHandle);
        };
        if owner != 0 && owner != self.current_thread {
            return Ok(false);
        }
        let depth = depth.checked_add(1).ok_or(Error::Exhausted)?;
        let next = self.next_revision()?;
        self.objects[slot].as_mut().unwrap().kind = ObjectKind32::Mutex {
            owner: self.current_thread,
            depth,
            abandoned: false,
        };
        set(&mut self.signaled, slot, false);
        self.revision = next;
        Ok(true)
    }
    pub fn prepare_release_mutex(&self, handle: u32) -> Result<PreparedMutexRelease32, Error> {
        let slot = self.handles[self.handle_index(handle, 1)?].event;
        let ObjectKind32::Mutex {
            owner,
            depth,
            abandoned,
        } = self.objects[slot].as_ref().unwrap().kind
        else {
            return Err(Error::InvalidHandle);
        };
        if owner != self.current_thread {
            return Err(Error::NotOwner);
        }
        let kind = ObjectKind32::Mutex {
            owner: if depth == 1 { 0 } else { owner },
            depth: depth - 1,
            abandoned,
        };
        let plan = if depth == 1 && self.object_waiters(handle)? != 0 {
            let mut prepared = self.signal_plan_typed(self.now, None)?;
            *self.projected_kind_mut(&mut prepared, slot) = kind;
            set(&mut prepared.signaled, slot, true);
            self.resolve_waiters(&mut prepared)?;
            Some(prepared)
        } else {
            None
        };
        Ok(PreparedMutexRelease32 {
            epoch: self.epoch,
            revision: self.revision,
            next: self.next_revision()?,
            slot,
            kind,
            plan,
        })
    }
    pub fn commit_release_mutex(&mut self, prepared: PreparedMutexRelease32) {
        assert_eq!(
            (self.epoch, self.revision),
            (prepared.epoch, prepared.revision)
        );
        if let Some(plan) = prepared.plan {
            self.commit_signals(plan);
        } else {
            self.objects[prepared.slot].as_mut().unwrap().kind = prepared.kind;
            set(
                &mut self.signaled,
                prepared.slot,
                matches!(prepared.kind, ObjectKind32::Mutex { owner: 0, .. }),
            );
            self.revision = prepared.next;
        }
    }
    pub fn prepare_thread_exit(&self, thread: u32) -> Result<PreparedSignals32, Error> {
        let mut prepared = self.signal_plan_typed(self.now, None)?;
        prepared.cancel_thread = Some(thread);
        for (slot, kind) in &mut prepared.objects {
            match *kind {
                ObjectKind32::Mutex { owner, .. } if owner == thread => {
                    *kind = ObjectKind32::Mutex {
                        owner: 0,
                        depth: 0,
                        abandoned: true,
                    };
                    set(&mut prepared.signaled, *slot, true);
                }
                ObjectKind32::Thread { id, .. } if id == thread => {
                    *kind = ObjectKind32::Thread {
                        id,
                        terminated: true,
                    };
                    set(&mut prepared.signaled, *slot, true);
                }
                ObjectKind32::Timer {
                    thread: owner,
                    procedure,
                    ..
                } if owner == thread && procedure != 0 => {
                    if let ObjectKind32::Timer { due, queued, .. } = kind {
                        *due = None;
                        *queued = false;
                    }
                }
                _ => {}
            }
        }
        self.resolve_waiters(&mut prepared)?;
        Ok(prepared)
    }
    pub fn terminate_thread_object(&mut self, thread: u32) -> Result<(), Error> {
        let prepared = self.prepare_thread_exit(thread)?;
        self.commit_signals(prepared);
        Ok(())
    }
    pub(in crate::windows::synchronization) fn prepare_process_exit(
        &self,
        process: u32,
    ) -> Result<PreparedSignals32, Error> {
        let mut prepared = self.signal_plan_typed(self.now, None)?;
        for (slot, kind) in &mut prepared.objects {
            match *kind {
                ObjectKind32::Process { id, .. } if id == process => {
                    *kind = ObjectKind32::Process {
                        id,
                        terminated: true,
                    };
                    set(&mut prepared.signaled, *slot, true);
                }
                ObjectKind32::Thread { id, .. } => {
                    *kind = ObjectKind32::Thread {
                        id,
                        terminated: true,
                    };
                    set(&mut prepared.signaled, *slot, true);
                }
                ObjectKind32::Mutex { owner, .. } if owner != 0 => {
                    *kind = ObjectKind32::Mutex {
                        owner: 0,
                        depth: 0,
                        abandoned: true,
                    };
                    set(&mut prepared.signaled, *slot, true);
                }
                ObjectKind32::Timer { .. } => {
                    if let ObjectKind32::Timer { due, queued, .. } = kind {
                        *due = None;
                        *queued = false;
                    }
                }
                _ => {}
            }
        }
        self.resolve_waiters(&mut prepared)?;
        Ok(prepared)
    }
}
