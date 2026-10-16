use super::{
    PreparedObjectHandle32, PreparedSignals32, SynchronizationContext32,
    SynchronizationError32 as Error,
    context::{ObjectKind32, set},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TimerApc32 {
    pub object: u64,
    pub generation: u64,
    pub thread: u32,
    pub procedure: u32,
    pub argument: u32,
    pub due_ns: u64,
}
impl PreparedSignals32 {
    pub(crate) fn timer_apcs(&self) -> &[TimerApc32] {
        &self.timer_apcs
    }
}
impl SynchronizationContext32 {
    pub fn prepare_timer(
        &self,
        manual: bool,
        inherit: bool,
        name: Option<&[u16]>,
    ) -> Result<(PreparedObjectHandle32, bool), Error> {
        self.prepare_object(
            ObjectKind32::Timer {
                due: None,
                period: 0,
                thread: 0,
                procedure: 0,
                argument: 0,
                generation: 0,
                queued: false,
            },
            manual,
            inherit,
            name,
            0x001f_0003,
        )
    }
    pub fn open_timer(&mut self, access: u32, inherit: bool, name: &[u16]) -> Result<u32, Error> {
        self.open_object(access, inherit, name, 3)
    }
    pub fn timer_identity(&self, handle: u32) -> Result<(u64, u64, u32), Error> {
        let slot = self.handles[self.handle_index(handle, 2)?].event;
        let object = self.objects[slot].as_ref().unwrap();
        let ObjectKind32::Timer {
            generation, thread, ..
        } = object.kind
        else {
            return Err(Error::InvalidHandle);
        };
        Ok((object.id, generation, thread))
    }
    pub(crate) fn timer_callback_identity(&self, id: u64) -> Option<(u64, u32, u32)> {
        self.objects
            .iter()
            .flatten()
            .find(|object| object.id == id)
            .and_then(|object| {
                if let ObjectKind32::Timer {
                    generation,
                    thread,
                    procedure,
                    due,
                    queued,
                    ..
                } = object.kind
                    && (due.is_some() || queued)
                {
                    Some((generation, thread, procedure))
                } else {
                    None
                }
            })
    }
    pub(crate) fn timer_capture_capacity(&self) -> usize {
        self.objects.len()
    }
    pub fn next_timer_deadline(&self) -> Option<u64> {
        self.objects
            .iter()
            .flatten()
            .filter_map(|object| match object.kind {
                ObjectKind32::Timer { due, .. } => due,
                _ => None,
            })
            .min()
    }
    pub fn prepare_set_timer(
        &self,
        handle: u32,
        due_ns: u64,
        period_ns: u64,
        procedure: u32,
        argument: u32,
    ) -> Result<PreparedSignals32, Error> {
        let slot = self.handles[self.handle_index(handle, 2)?].event;
        let ObjectKind32::Timer { generation, .. } = self.objects[slot].as_ref().unwrap().kind
        else {
            return Err(Error::InvalidHandle);
        };
        let mut plan = self.signal_plan_typed(self.now, None)?;
        *self.projected_kind_mut(&mut plan, slot) = ObjectKind32::Timer {
            due: Some(due_ns),
            period: period_ns,
            thread: self.current_thread,
            procedure,
            argument,
            generation: generation.checked_add(1).ok_or(Error::Exhausted)?,
            queued: false,
        };
        set(&mut plan.signaled, slot, false);
        Ok(plan)
    }
    pub fn prepare_cancel_timer(&self, handle: u32) -> Result<PreparedSignals32, Error> {
        let slot = self.handles[self.handle_index(handle, 2)?].event;
        let ObjectKind32::Timer {
            period,
            thread,
            procedure,
            argument,
            generation,
            ..
        } = self.objects[slot].as_ref().unwrap().kind
        else {
            return Err(Error::InvalidHandle);
        };
        let mut plan = self.signal_plan_typed(self.now, None)?;
        *self.projected_kind_mut(&mut plan, slot) = ObjectKind32::Timer {
            due: None,
            period,
            thread,
            procedure,
            argument,
            generation: generation.checked_add(1).ok_or(Error::Exhausted)?,
            queued: false,
        };
        Ok(plan)
    }
    pub(crate) fn prepare_timer_clock(&self, now: u64) -> Result<PreparedSignals32, Error> {
        self.prepare_time_with_messages(now, &[])
    }
    pub fn prepare_time_with_messages(
        &self,
        now: u64,
        rows: &[(u32, u32, u32, u64)],
    ) -> Result<PreparedSignals32, Error> {
        let expired =
            self.objects.iter().flatten().any(
                |object| matches!(object.kind,ObjectKind32::Timer {due:Some(due),..} if due<=now),
            );
        let mut plan = if expired {
            self.signal_plan_typed(now, None)?
        } else {
            self.signal_plan(now, None)?
        };
        self.project_message_rows(&mut plan, rows)?;
        if expired {
            let callbacks=plan.objects.iter().filter(|(_,kind)|matches!(kind,ObjectKind32::Timer {due:Some(due),procedure,queued:false,..} if *due<=now && *procedure!=0)).count();
            plan.timer_apcs
                .try_reserve_exact(callbacks)
                .map_err(|_| Error::Allocation)?;
            for (slot, kind) in &mut plan.objects {
                let ObjectKind32::Timer {
                    due,
                    period,
                    thread,
                    procedure,
                    argument,
                    generation,
                    queued,
                } = kind
                else {
                    continue;
                };
                let Some(expiration) = *due else {
                    continue;
                };
                if expiration > now {
                    continue;
                }
                set(&mut plan.signaled, *slot, true);
                *due = (now - expiration)
                    .checked_div(*period)
                    .and_then(|count| count.checked_add(1))
                    .and_then(|count| count.checked_mul(*period))
                    .and_then(|delta| expiration.checked_add(delta));
                if *procedure != 0 && !*queued {
                    plan.timer_apcs.push(TimerApc32 {
                        object: self.objects[*slot].as_ref().unwrap().id,
                        generation: *generation,
                        thread: *thread,
                        procedure: *procedure,
                        argument: *argument,
                        due_ns: expiration,
                    });
                    *queued = true;
                }
            }
            let mut threads = Vec::new();
            threads
                .try_reserve_exact(plan.timer_apcs.len())
                .map_err(|_| Error::Allocation)?;
            for firing in &plan.timer_apcs {
                if !threads.contains(&firing.thread) {
                    threads.push(firing.thread);
                }
            }
            self.project_pending_apcs(&mut plan, &threads)?;
        }
        self.resolve_waiters(&mut plan)?;
        Ok(plan)
    }
    pub(crate) fn prepare_timer_apc_delivery(
        &self,
        object: u64,
        generation: u64,
        thread: u32,
        remaining: bool,
    ) -> Result<PreparedSignals32, Error> {
        let mut plan = self.signal_plan_typed(self.now, None)?;
        if let Some(slot) = self
            .objects
            .iter()
            .position(|slot| slot.as_ref().is_some_and(|entry| entry.id == object))
            && let ObjectKind32::Timer {
                generation: actual,
                queued,
                ..
            } = self.projected_kind_mut(&mut plan, slot)
            && *actual == generation
        {
            *queued = false;
        }
        self.project_apc_state(&mut plan, thread, remaining)?;
        Ok(plan)
    }
}
