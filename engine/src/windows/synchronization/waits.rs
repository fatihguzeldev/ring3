use super::context::{ObjectKind32, QueueObservation32, Wait32, bit, set};
use super::types::{MAX_WAITS, STATE_WORDS};
use super::{
    EventBinding32, INFINITE, MAX_WAIT_OBJECTS, MessageWait32, SYNCHRONIZE,
    SynchronizationContext32, SynchronizationError32 as Error, WAIT_TIMEOUT, WaitMode32,
    WaitOutcome32, WaitResult32, WaitToken32,
};

pub struct WaitHandles32<'a> {
    pub original: &'a [u32],
    pub projected: &'a [u32],
}

pub struct PreparedSignals32 {
    epoch: u64,
    revision: u64,
    next_revision: u64,
    now: u64,
    pub(super) signaled: [u64; STATE_WORDS],
    completed: [Option<WaitResult32>; MAX_WAITS],
    queue: Option<QueueObservation32>,
    extra_queues: Vec<QueueObservation32>,
    pub(super) cancel_thread: Option<u32>,
    apc: Option<(u32, bool)>,
    extra_apcs: Vec<u32>,
    clear_apc_thread: Option<u32>,
    pub(super) timer_apcs: Vec<super::timers::TimerApc32>,
    pub(super) objects: Vec<(usize, ObjectKind32)>,
}
impl SynchronizationContext32 {
    pub fn prepare_signal_handles(&self, handles: &[u32]) -> Result<PreparedSignals32, Error> {
        if handles.len() > MAX_WAIT_OBJECTS {
            return Err(Error::InvalidWait);
        }
        let mut slots = [0usize; MAX_WAIT_OBJECTS];
        for (index, handle) in handles.iter().enumerate() {
            slots[index] =
                self.handles[self.handle_index(*handle, super::EVENT_MODIFY_STATE)?].event;
            self.require_event(slots[index])?;
        }
        self.prepare_signal_slots(&slots[..handles.len()], &[], self.now, None)
    }
    pub fn prepare_signals(&self, bindings: &[EventBinding32]) -> Result<PreparedSignals32, Error> {
        let mut prepared = self.signal_plan(self.now, None)?;
        for binding in bindings {
            self.prepare_occurrence(&mut prepared, *binding, 1)?;
        }
        if bindings.is_empty() {
            self.resolve_waiters(&mut prepared)?;
        }
        Ok(prepared)
    }
    pub fn prepare_signal_occurrences(
        &self,
        occurrences: &[(EventBinding32, u64)],
    ) -> Result<PreparedSignals32, Error> {
        self.prepare_signal_occurrences_with_messages(occurrences, None)
    }
    pub fn prepare_signal_occurrences_with_messages(
        &self,
        occurrences: &[(EventBinding32, u64)],
        queue: Option<(u32, u32, u64)>,
    ) -> Result<PreparedSignals32, Error> {
        if queue
            .is_some_and(|(_, _, generation)| generation < self.queue_state(self.current_thread).2)
        {
            return Err(Error::Stale);
        }
        let mut prepared = self.signal_plan(self.now, queue)?;
        for &(binding, count) in occurrences {
            self.prepare_occurrence(&mut prepared, binding, count)?;
        }
        if occurrences.is_empty() {
            self.resolve_waiters(&mut prepared)?;
        }
        Ok(prepared)
    }

    pub(super) fn prepare_occurrence(
        &self,
        prepared: &mut PreparedSignals32,
        binding: EventBinding32,
        count: u64,
    ) -> Result<(), Error> {
        let slot = self.validate_binding(binding)?;
        let count = count.min(if self.objects[slot].as_ref().unwrap().manual {
            1
        } else {
            self.waits.len() as u64 + 1
        });
        for _ in 0..count {
            set(&mut prepared.signaled, slot, true);
            self.resolve_waiters(prepared)?;
        }
        Ok(())
    }
    pub(super) fn prepare_signal_slots(
        &self,
        slots: &[usize],
        pulse: &[usize],
        now: u64,
        queue: Option<(u32, u32, u64)>,
    ) -> Result<PreparedSignals32, Error> {
        let mut prepared = self.signal_plan(now, queue)?;
        for slot in slots {
            set(&mut prepared.signaled, *slot, true);
        }
        self.resolve_waiters(&mut prepared)?;
        for slot in pulse {
            set(&mut prepared.signaled, *slot, false);
        }
        Ok(prepared)
    }
    pub(super) fn signal_plan(
        &self,
        now: u64,
        queue: Option<(u32, u32, u64)>,
    ) -> Result<PreparedSignals32, Error> {
        let typed = self.typed_objects != 0
            && self.waits.iter().flatten().any(|wait| {
                wait.result.is_none()
                    && wait.events[..wait.count].iter().any(|&slot| {
                        !matches!(
                            self.objects[slot].as_ref().unwrap().kind,
                            ObjectKind32::Event
                        )
                    })
            });
        self.signal_plan_inner(now, queue, typed)
    }
    pub(super) fn signal_plan_typed(
        &self,
        now: u64,
        queue: Option<(u32, u32, u64)>,
    ) -> Result<PreparedSignals32, Error> {
        self.signal_plan_inner(now, queue, true)
    }
    fn signal_plan_for_wait(
        &self,
        wait: &Wait32,
        now: u64,
        queue: Option<(u32, u32, u64)>,
    ) -> Result<PreparedSignals32, Error> {
        if wait.events[..wait.count].iter().any(|&slot| {
            !matches!(
                self.objects[slot].as_ref().unwrap().kind,
                ObjectKind32::Event
            )
        }) {
            self.signal_plan_typed(now, queue)
        } else {
            self.signal_plan_inner(now, queue, false)
        }
    }
    fn signal_plan_inner(
        &self,
        now: u64,
        queue: Option<(u32, u32, u64)>,
        typed: bool,
    ) -> Result<PreparedSignals32, Error> {
        if now < self.now {
            return Err(Error::ClockBackwards);
        }
        if queue.is_some()
            && !self
                .queues
                .iter()
                .any(|queue| queue.thread == self.current_thread)
            && self.queues.len() == self.queues.capacity()
        {
            return Err(Error::Capacity);
        }
        let mut objects = Vec::new();
        if typed {
            objects
                .try_reserve_exact(self.typed_objects)
                .map_err(|_| Error::Allocation)?;
            for (slot, object) in self.objects.iter().enumerate() {
                if let Some(object) = object
                    && !matches!(object.kind, ObjectKind32::Event)
                {
                    objects.push((slot, object.kind));
                }
            }
        }
        Ok(PreparedSignals32 {
            epoch: self.epoch,
            revision: self.revision,
            next_revision: self.next_revision()?,
            now,
            signaled: self.signaled,
            completed: [None; MAX_WAITS],
            queue: queue.map(|(current, new, generation)| QueueObservation32 {
                thread: self.current_thread,
                current,
                new,
                generation,
            }),
            extra_queues: Vec::new(),
            cancel_thread: None,
            apc: None,
            extra_apcs: Vec::new(),
            clear_apc_thread: None,
            timer_apcs: Vec::new(),
            objects,
        })
    }
    pub(super) fn resolve_waiters(&self, prepared: &mut PreparedSignals32) -> Result<(), Error> {
        let mut order = [usize::MAX; MAX_WAITS];
        let mut count = 0;
        for (slot, wait) in self.waits.iter().enumerate() {
            if wait.as_ref().is_some_and(|wait| {
                (wait.result.is_none()
                    || Some(wait.thread) == prepared.clear_apc_thread
                        && wait.result.is_some_and(|result| result.value == 0xc0))
                    && Some(wait.thread) != prepared.cancel_thread
            }) && prepared.completed[slot].is_none()
            {
                order[count] = slot;
                count += 1;
            }
        }
        order[..count].sort_unstable_by_key(|slot| self.waits[*slot].as_ref().unwrap().id);
        for slot in &order[..count] {
            let wait = self.waits[*slot].as_ref().unwrap();
            let (current, new, _) = self.projected_queue(prepared, wait.thread);
            let timeout = wait
                .deadline
                .is_some_and(|deadline| deadline <= prepared.now);
            let result = if timeout {
                Some(WaitResult32 {
                    value: if wait.mode == WaitMode32::Sleep {
                        0
                    } else {
                        WAIT_TIMEOUT
                    },
                    last_error: None,
                })
            } else {
                self.resolve(wait, prepared, current, new)?
            };
            prepared.completed[*slot] = result;
        }
        Ok(())
    }
    pub fn commit_signals(&mut self, prepared: PreparedSignals32) {
        assert_eq!(
            (self.epoch, self.revision),
            (prepared.epoch, prepared.revision)
        );
        if let Some(thread) = prepared.cancel_thread {
            for wait in &mut self.waits {
                if wait.as_ref().is_some_and(|wait| wait.thread == thread) {
                    *wait = None;
                }
            }
            self.queues.retain(|queue| queue.thread != thread);
            self.apcs.retain(|id| *id != thread);
        }
        if let Some((thread, pending)) = prepared.apc {
            if pending && !self.apcs.contains(&thread) {
                self.apcs.push(thread);
            }
            if !pending {
                self.apcs.retain(|id| *id != thread);
            }
        }
        for thread in prepared.extra_apcs {
            if !self.apcs.contains(&thread) {
                self.apcs.push(thread);
            }
        }
        if let Some(thread) = prepared.clear_apc_thread {
            for wait in self.waits.iter_mut().flatten() {
                if wait.thread == thread && wait.result.is_some_and(|result| result.value == 0xc0) {
                    wait.result = None;
                }
            }
        }
        self.signaled = prepared.signaled;
        for (slot, kind) in prepared.objects {
            self.objects[slot].as_mut().unwrap().kind = kind;
            self.reclaim_event(slot);
        }
        self.now = prepared.now;
        for (slot, result) in prepared
            .completed
            .into_iter()
            .enumerate()
            .take(self.waits.len())
        {
            if let Some(result) = result {
                self.waits[slot].as_mut().unwrap().result = Some(result);
            }
        }
        if let Some(queue) = prepared.queue {
            self.commit_queue(queue);
        }
        for queue in prepared.extra_queues {
            self.commit_queue(queue);
        }
        self.revision = prepared.next_revision;
    }
    pub fn observe_time(&mut self, now_ns: u64) -> Result<(), Error> {
        if now_ns == self.now {
            return Ok(());
        }
        let prepared = self.prepare_time(now_ns)?;
        self.commit_signals(prepared);
        Ok(())
    }
    pub fn prepare_apc_state(
        &self,
        thread: u32,
        pending: bool,
    ) -> Result<PreparedSignals32, Error> {
        let mut prepared = self.signal_plan_inner(self.now, None, false)?;
        self.project_apc_state(&mut prepared, thread, pending)?;
        Ok(prepared)
    }
    pub(crate) fn project_apc_state(
        &self,
        prepared: &mut PreparedSignals32,
        thread: u32,
        pending: bool,
    ) -> Result<(), Error> {
        if thread == 0 {
            return Err(Error::InvalidWait);
        }
        if pending && !self.apcs.contains(&thread) && self.apcs.len() == self.apcs.capacity() {
            return Err(Error::Capacity);
        }
        prepared.apc = Some((thread, pending));
        if pending {
            for (slot, wait) in self.waits.iter().enumerate() {
                if wait.as_ref().is_some_and(|wait| {
                    wait.thread == thread && wait.alertable && wait.result.is_none()
                }) {
                    prepared.completed[slot] = Some(WaitResult32 {
                        value: 0xc0,
                        last_error: None,
                    });
                }
            }
        }
        Ok(())
    }
    pub(crate) fn project_pending_apcs(
        &self,
        prepared: &mut PreparedSignals32,
        threads: &[u32],
    ) -> Result<(), Error> {
        let additional = threads
            .iter()
            .filter(|thread| !self.apcs.contains(thread))
            .count();
        if self
            .apcs
            .len()
            .checked_add(additional)
            .is_none_or(|count| count > self.apcs.capacity())
        {
            return Err(Error::Capacity);
        }
        prepared
            .extra_apcs
            .try_reserve_exact(threads.len())
            .map_err(|_| Error::Allocation)?;
        for &thread in threads {
            if thread == 0 {
                return Err(Error::InvalidWait);
            }
            if !prepared.extra_apcs.contains(&thread) {
                prepared.extra_apcs.push(thread);
            }
            for (slot, wait) in self.waits.iter().enumerate() {
                if wait.as_ref().is_some_and(|wait| {
                    wait.thread == thread && wait.alertable && wait.result.is_none()
                }) {
                    prepared.completed[slot] = Some(WaitResult32 {
                        value: 0xc0,
                        last_error: None,
                    });
                }
            }
        }
        Ok(())
    }
    pub(crate) fn project_cancel_apc_state(
        &self,
        prepared: &mut PreparedSignals32,
        thread: u32,
        pending: bool,
    ) -> Result<(), Error> {
        self.project_apc_state(prepared, thread, pending)?;
        if !pending {
            prepared.clear_apc_thread = Some(thread);
            self.resolve_waiters(prepared)?;
        }
        Ok(())
    }
    pub fn prepare_time(&self, now_ns: u64) -> Result<PreparedSignals32, Error> {
        self.prepare_timer_clock(now_ns)
    }
    fn projected_queue(&self, prepared: &PreparedSignals32, thread: u32) -> (u32, u32, u64) {
        prepared
            .queue
            .iter()
            .chain(&prepared.extra_queues)
            .find(|queue| queue.thread == thread)
            .map_or_else(
                || self.queue_state(thread),
                |queue| (queue.current, queue.new, queue.generation),
            )
    }
    pub fn prepare_messages_for_thread(
        &self,
        thread: u32,
        current: u32,
        new: u32,
        generation: u64,
    ) -> Result<PreparedSignals32, Error> {
        self.prepare_messages_for_threads(&[(thread, current, new, generation)])
    }
    pub fn prepare_messages_for_threads(
        &self,
        rows: &[(u32, u32, u32, u64)],
    ) -> Result<PreparedSignals32, Error> {
        let mut additional = 0;
        for (index, &(thread, _, _, generation)) in rows.iter().enumerate() {
            if thread == 0 || rows[..index].iter().any(|row| row.0 == thread) {
                return Err(Error::InvalidWait);
            }
            if generation < self.queue_state(thread).2 {
                return Err(Error::Stale);
            }
            if !self.queues.iter().any(|queue| queue.thread == thread) {
                additional += 1;
            }
        }
        if self
            .queues
            .len()
            .checked_add(additional)
            .is_none_or(|n| n > self.queues.capacity())
        {
            return Err(Error::Capacity);
        }
        let mut prepared = self.signal_plan(self.now, None)?;
        prepared
            .extra_queues
            .try_reserve_exact(rows.len().saturating_sub(1))
            .map_err(|_| Error::Allocation)?;
        for (index, &(thread, current, new, generation)) in rows.iter().enumerate() {
            let queue = QueueObservation32 {
                thread,
                current,
                new,
                generation,
            };
            if index == 0 {
                prepared.queue = Some(queue);
            } else {
                prepared.extra_queues.push(queue);
            }
        }
        self.resolve_waiters(&mut prepared)?;
        Ok(prepared)
    }
    pub fn refresh_prepared_messages_for_thread(
        &self,
        mut prepared: PreparedSignals32,
        thread: u32,
        current: u32,
        new: u32,
        generation: u64,
    ) -> PreparedSignals32 {
        assert_eq!(
            (self.epoch, self.revision),
            (prepared.epoch, prepared.revision)
        );
        assert_eq!(
            prepared.queue.as_ref().map(|queue| queue.thread),
            Some(thread)
        );
        assert!(prepared.extra_queues.is_empty());
        assert!(generation >= self.queue_state(thread).2);
        prepared.signaled = self.signaled;
        prepared.completed.fill(None);
        for (slot, kind) in &mut prepared.objects {
            *kind = self.objects[*slot].as_ref().unwrap().kind;
        }
        prepared.queue = Some(QueueObservation32 {
            thread,
            current,
            new,
            generation,
        });
        self.resolve_waiters(&mut prepared)
            .expect("all possible message masks were preflighted before queue mutation");
        prepared
    }
    pub fn observe_messages_for_thread(
        &mut self,
        thread: u32,
        current: u32,
        new: u32,
        generation: u64,
    ) -> Result<(), Error> {
        if generation < self.queue_state(thread).2 {
            return Err(Error::Stale);
        }
        if (current, new, generation) == self.queue_state(thread) {
            return Ok(());
        }
        let receipt = self.prepare_messages_for_thread(thread, current, new, generation)?;
        self.commit_signals(receipt);
        Ok(())
    }
    pub fn observe_messages(
        &mut self,
        current: u32,
        new: u32,
        generation: u64,
    ) -> Result<(), Error> {
        if generation < self.queue_state(self.current_thread).2 {
            return Err(Error::Stale);
        }
        if (current, new, generation) == self.queue_state(self.current_thread) {
            return Ok(());
        }
        let prepared =
            self.prepare_signal_slots(&[], &[], self.now, Some((current, new, generation)))?;
        self.commit_signals(prepared);
        Ok(())
    }
    pub fn prepare_messages(
        &self,
        current: u32,
        new: u32,
        generation: u64,
    ) -> Result<PreparedSignals32, Error> {
        if generation < self.queue_state(self.current_thread).2 {
            return Err(Error::Stale);
        }
        self.prepare_messages_for_thread(self.current_thread, current, new, generation)
    }
    pub fn wait_with_messages(
        &mut self,
        handles: &[u32],
        mode: WaitMode32,
        timeout_ms: u32,
        alertable: bool,
        message: MessageWait32,
        queue: (u32, u32, u64),
    ) -> Result<WaitOutcome32, Error> {
        self.wait_with_messages_projected(
            WaitHandles32 {
                original: handles,
                projected: handles,
            },
            mode,
            timeout_ms,
            alertable,
            message,
            queue,
        )
    }
    pub fn wait_with_messages_projected(
        &mut self,
        handles: WaitHandles32<'_>,
        mode: WaitMode32,
        timeout_ms: u32,
        alertable: bool,
        message: MessageWait32,
        queue: (u32, u32, u64),
    ) -> Result<WaitOutcome32, Error> {
        if queue.2 < self.queue_state(self.current_thread).2 {
            return Err(Error::Stale);
        }
        let wait =
            self.validated_wait_projected(handles, mode, timeout_ms, alertable, Some(message))?;
        if alertable && self.apcs.contains(&wait.thread) {
            return Ok(WaitOutcome32::Ready(WaitResult32 {
                value: 0xc0,
                last_error: None,
            }));
        }
        let mut prepared = self.signal_plan_for_wait(&wait, self.now, Some(queue))?;
        self.resolve_waiters(&mut prepared)?;
        let result = self.resolve(&wait, &mut prepared, queue.0, queue.1)?;
        let slot = if result.is_none() && timeout_ms != 0 {
            Some(
                self.waits
                    .iter()
                    .position(Option::is_none)
                    .ok_or(Error::Capacity)?,
            )
        } else {
            None
        };
        prepared.queue = Some(QueueObservation32 {
            thread: self.current_thread,
            current: queue.0,
            new: queue.1 & !message.mask,
            generation: queue.2,
        });
        self.commit_signals(prepared);
        if let Some(result) = result {
            return Ok(WaitOutcome32::Ready(result));
        }
        if let Some(slot) = slot {
            self.next_wait = wait.id;
            let token = WaitToken32 {
                epoch: self.epoch,
                slot,
                id: wait.id,
            };
            self.waits[slot] = Some(wait);
            Ok(WaitOutcome32::Pending(token))
        } else {
            Ok(WaitOutcome32::Ready(WaitResult32 {
                value: WAIT_TIMEOUT,
                last_error: None,
            }))
        }
    }
    pub fn wait(
        &mut self,
        handles: &[u32],
        mode: WaitMode32,
        timeout_ms: u32,
        alertable: bool,
        message: Option<MessageWait32>,
    ) -> Result<WaitOutcome32, Error> {
        self.wait_projected(
            WaitHandles32 {
                original: handles,
                projected: handles,
            },
            mode,
            timeout_ms,
            alertable,
            message,
        )
    }
    pub fn wait_projected(
        &mut self,
        handles: WaitHandles32<'_>,
        mode: WaitMode32,
        timeout_ms: u32,
        alertable: bool,
        message: Option<MessageWait32>,
    ) -> Result<WaitOutcome32, Error> {
        let wait = self.validated_wait_projected(handles, mode, timeout_ms, alertable, message)?;
        if alertable && self.apcs.contains(&wait.thread) {
            return Ok(WaitOutcome32::Ready(WaitResult32 {
                value: 0xc0,
                last_error: None,
            }));
        }
        let mut prepared = self.signal_plan_for_wait(&wait, self.now, None)?;
        let (current, new, _) = self.queue_state(wait.thread);
        if let Some(result) = self.resolve(&wait, &mut prepared, current, new)? {
            self.commit_signals(prepared);
            return Ok(WaitOutcome32::Ready(result));
        }
        if timeout_ms == 0 {
            return Ok(WaitOutcome32::Ready(WaitResult32 {
                value: if mode == WaitMode32::Sleep {
                    0
                } else {
                    WAIT_TIMEOUT
                },
                last_error: None,
            }));
        }
        let slot = self
            .waits
            .iter()
            .position(Option::is_none)
            .ok_or(Error::Capacity)?;
        self.next_wait = wait.id;
        let token = WaitToken32 {
            epoch: self.epoch,
            id: wait.id,
            slot,
        };
        self.waits[slot] = Some(wait);
        self.commit_signals(prepared);
        Ok(WaitOutcome32::Pending(token))
    }
    fn validated_wait(
        &self,
        handles: &[u32],
        mode: WaitMode32,
        timeout_ms: u32,
        alertable: bool,
        message: Option<MessageWait32>,
    ) -> Result<Wait32, Error> {
        self.validated_wait_projected(
            WaitHandles32 {
                original: handles,
                projected: handles,
            },
            mode,
            timeout_ms,
            alertable,
            message,
        )
    }
    fn validated_wait_projected(
        &self,
        projection: WaitHandles32<'_>,
        mode: WaitMode32,
        timeout_ms: u32,
        alertable: bool,
        message: Option<MessageWait32>,
    ) -> Result<Wait32, Error> {
        let handles = projection.projected;
        let original = projection.original;
        if original.len() != handles.len() {
            return Err(Error::InvalidWait);
        }
        if handles.len() > MAX_WAIT_OBJECTS - usize::from(message.is_some())
            || mode != WaitMode32::Sleep && handles.is_empty() && message.is_none()
            || mode == WaitMode32::Sleep && (!handles.is_empty() || message.is_some())
        {
            return Err(Error::InvalidWait);
        }
        let deadline = if timeout_ms == INFINITE {
            None
        } else {
            Some(
                self.now
                    .checked_add(u64::from(timeout_ms) * 1_000_000)
                    .ok_or(Error::DeadlineOverflow)?,
            )
        };
        let mut wait = Wait32 {
            id: self.next_wait.checked_add(1).ok_or(Error::Exhausted)?,
            handles: [0; MAX_WAIT_OBJECTS],
            events: [0; MAX_WAIT_OBJECTS],
            count: handles.len(),
            mode,
            deadline,
            alertable,
            message,
            result: None,
            thread: self.current_thread,
        };
        for (index, handle) in handles.iter().enumerate() {
            let event = self.handles[self.handle_index(*handle, SYNCHRONIZE)?].event;
            if original[..index].contains(&original[index])
                || mode == WaitMode32::All && wait.events[..index].contains(&event)
            {
                return Err(Error::InvalidWait);
            }
            wait.handles[index] = *handle;
            wait.events[index] = event;
        }
        Ok(wait)
    }
    pub fn signal_and_wait(
        &mut self,
        signal: u32,
        wait_handle: u32,
        timeout_ms: u32,
        alertable: bool,
    ) -> Result<WaitOutcome32, Error> {
        let index = self.handle_index(signal, 0)?;
        let signal = self.handles[index].event;
        let kind = self.objects[signal].as_ref().unwrap().kind;
        let access = if matches!(kind, ObjectKind32::Mutex { .. }) {
            1
        } else {
            super::EVENT_MODIFY_STATE
        };
        if self.handles[index].access & access != access {
            return Err(Error::AccessDenied);
        }
        let wait =
            self.validated_wait(&[wait_handle], WaitMode32::Any, timeout_ms, alertable, None)?;
        let mut prepared = self.signal_plan_typed(self.now, None)?;
        match kind {
            ObjectKind32::Event => set(&mut prepared.signaled, signal, true),
            ObjectKind32::Mutex {
                owner,
                depth,
                abandoned,
            } => {
                if owner != self.current_thread {
                    return Err(Error::NotOwner);
                }
                *self.projected_kind_mut(&mut prepared, signal) = ObjectKind32::Mutex {
                    owner: if depth == 1 { 0 } else { owner },
                    depth: depth - 1,
                    abandoned,
                };
                set(&mut prepared.signaled, signal, depth == 1);
            }
            ObjectKind32::Semaphore { count, maximum } => {
                let count = count
                    .checked_add(1)
                    .filter(|count| *count <= maximum)
                    .ok_or(Error::TooManyPosts)?;
                *self.projected_kind_mut(&mut prepared, signal) =
                    ObjectKind32::Semaphore { count, maximum };
                set(&mut prepared.signaled, signal, true);
            }
            ObjectKind32::Thread { .. }
            | ObjectKind32::Process { .. }
            | ObjectKind32::Timer { .. } => {
                return Err(Error::InvalidHandle);
            }
        }
        self.resolve_waiters(&mut prepared)?;
        if alertable && self.apcs.contains(&wait.thread) {
            self.commit_signals(prepared);
            return Ok(WaitOutcome32::Ready(WaitResult32 {
                value: 0xc0,
                last_error: None,
            }));
        }
        let (current, new, _) = self.queue_state(wait.thread);
        let result = self.resolve(&wait, &mut prepared, current, new)?;
        if let Some(result) = result {
            self.commit_signals(prepared);
            return Ok(WaitOutcome32::Ready(result));
        }
        if timeout_ms == 0 {
            self.commit_signals(prepared);
            return Ok(WaitOutcome32::Ready(WaitResult32 {
                value: WAIT_TIMEOUT,
                last_error: None,
            }));
        }
        let slot = self
            .waits
            .iter()
            .position(Option::is_none)
            .ok_or(Error::Capacity)?;
        self.commit_signals(prepared);
        self.next_wait = wait.id;
        let token = WaitToken32 {
            epoch: self.epoch,
            id: wait.id,
            slot,
        };
        self.waits[slot] = Some(wait);
        Ok(WaitOutcome32::Pending(token))
    }
    pub(super) fn projected_kind_mut<'a>(
        &self,
        prepared: &'a mut PreparedSignals32,
        slot: usize,
    ) -> &'a mut ObjectKind32 {
        let index = prepared
            .objects
            .binary_search_by_key(&slot, |&(slot, _)| slot)
            .expect("non-event object is present in the signal projection");
        &mut prepared.objects[index].1
    }
    fn projected_kind(&self, prepared: &PreparedSignals32, slot: usize) -> ObjectKind32 {
        prepared
            .objects
            .binary_search_by_key(&slot, |&(slot, _)| slot)
            .map_or_else(
                |_| self.objects[slot].as_ref().unwrap().kind,
                |index| prepared.objects[index].1,
            )
    }
    fn resolve(
        &self,
        wait: &Wait32,
        prepared: &mut PreparedSignals32,
        current: u32,
        new: u32,
    ) -> Result<Option<WaitResult32>, Error> {
        if wait.mode == WaitMode32::Sleep {
            return Ok(None);
        }
        let messages = wait.message.is_some_and(|message| {
            (if message.input_available {
                current
            } else {
                new
            }) & message.mask
                != 0
        });
        let ready = |slot: usize| match self.projected_kind(prepared, slot) {
            ObjectKind32::Mutex { owner, .. } => owner == 0 || owner == wait.thread,
            ObjectKind32::Semaphore { count, .. } => count != 0,
            ObjectKind32::Thread { terminated, .. } | ObjectKind32::Process { terminated, .. } => {
                terminated
            }
            ObjectKind32::Event | ObjectKind32::Timer { .. } => bit(&prepared.signaled, slot),
        };
        let selected = if wait.mode == WaitMode32::All {
            if wait.events[..wait.count].iter().any(|slot| !ready(*slot))
                || wait.message.is_some() && !messages
            {
                return Ok(None);
            }
            0
        } else if let Some(index) = wait.events[..wait.count]
            .iter()
            .position(|slot| ready(*slot))
        {
            index
        } else if messages {
            wait.count
        } else {
            return Ok(None);
        };
        let mut abandoned = None;
        for (index, &slot) in wait.events[..wait.count].iter().enumerate() {
            if wait.mode != WaitMode32::All && index != selected {
                continue;
            }
            match self.projected_kind(prepared, slot) {
                ObjectKind32::Mutex {
                    depth,
                    abandoned: was_abandoned,
                    ..
                } => {
                    let depth = depth.checked_add(1).ok_or(Error::Exhausted)?;
                    *self.projected_kind_mut(prepared, slot) = ObjectKind32::Mutex {
                        owner: wait.thread,
                        depth,
                        abandoned: false,
                    };
                    set(&mut prepared.signaled, slot, false);
                    if was_abandoned && abandoned.is_none() {
                        abandoned = Some(index);
                    }
                }
                ObjectKind32::Semaphore { count, maximum } => {
                    *self.projected_kind_mut(prepared, slot) = ObjectKind32::Semaphore {
                        count: count - 1,
                        maximum,
                    };
                    set(&mut prepared.signaled, slot, count > 1);
                }
                ObjectKind32::Event | ObjectKind32::Timer { .. } => {
                    if !self.objects[slot].as_ref().unwrap().manual {
                        set(&mut prepared.signaled, slot, false);
                    }
                }
                ObjectKind32::Thread { .. } | ObjectKind32::Process { .. } => {}
            }
        }
        Ok(Some(WaitResult32 {
            value: abandoned.map_or(selected as u32, |index| 0x80 + index as u32),
            last_error: None,
        }))
    }
    pub fn wait_status(&self, token: WaitToken32) -> Result<Option<WaitResult32>, Error> {
        Ok(self.wait_record(token)?.result)
    }
    pub fn wait_details(
        &self,
        token: WaitToken32,
    ) -> Result<(Option<u64>, bool, Option<MessageWait32>), Error> {
        let wait = self.wait_record(token)?;
        Ok((wait.deadline, wait.alertable, wait.message))
    }
    pub fn take_wait_observing_messages(
        &mut self,
        token: WaitToken32,
    ) -> Result<Option<WaitResult32>, Error> {
        let wait = self.wait_record(token)?;
        let thread = wait.thread;
        let mask = wait.message.map_or(0, |message| message.mask);
        let result = self.take_wait(token)?;
        if result.is_some() {
            if let Some(queue) = self.queues.iter_mut().find(|queue| queue.thread == thread) {
                queue.new &= !mask;
            }
        }
        Ok(result)
    }
    pub fn take_wait(&mut self, token: WaitToken32) -> Result<Option<WaitResult32>, Error> {
        let result = self.wait_record(token)?.result;
        if result.is_some() {
            let revision = self.next_revision()?;
            self.waits[token.slot] = None;
            self.revision = revision;
        }
        Ok(result)
    }
    pub fn cancel_wait(&mut self, token: WaitToken32) -> Result<(), Error> {
        self.wait_record(token)?;
        let revision = self.next_revision()?;
        self.waits[token.slot] = None;
        self.revision = revision;
        Ok(())
    }
    fn wait_record(&self, token: WaitToken32) -> Result<&Wait32, Error> {
        if token.epoch != self.epoch {
            return Err(Error::Stale);
        }
        self.waits
            .get(token.slot)
            .and_then(Option::as_ref)
            .filter(|wait| wait.id == token.id)
            .ok_or(Error::Stale)
    }
}
