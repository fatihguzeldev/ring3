use super::context::{Wait32, bit, set};
use super::types::{MAX_WAITS, STATE_WORDS};
use super::{
    EventBinding32, INFINITE, MAX_WAIT_OBJECTS, MessageWait32, SYNCHRONIZE,
    SynchronizationContext32, SynchronizationError32 as Error, WAIT_TIMEOUT, WaitMode32,
    WaitOutcome32, WaitResult32, WaitToken32,
};

pub struct PreparedSignals32 {
    epoch: u64,
    revision: u64,
    next_revision: u64,
    now: u64,
    signaled: [u64; STATE_WORDS],
    completed: [Option<WaitResult32>; MAX_WAITS],
    queue: Option<(u32, u32, u64)>,
}
impl SynchronizationContext32 {
    pub fn prepare_signals(&self, bindings: &[EventBinding32]) -> Result<PreparedSignals32, Error> {
        let mut prepared = self.signal_plan(self.now, None)?;
        for binding in bindings {
            self.prepare_occurrence(&mut prepared, *binding, 1)?;
        }
        if bindings.is_empty() {
            self.resolve_waiters(&mut prepared);
        }
        Ok(prepared)
    }
    pub fn prepare_signal_occurrences(
        &self,
        occurrences: &[(EventBinding32, u64)],
    ) -> Result<PreparedSignals32, Error> {
        let mut prepared = self.signal_plan(self.now, None)?;
        for (binding, count) in occurrences {
            self.prepare_occurrence(&mut prepared, *binding, *count)?;
        }
        if occurrences.is_empty() {
            self.resolve_waiters(&mut prepared);
        }
        Ok(prepared)
    }
    fn prepare_occurrence(
        &self,
        prepared: &mut PreparedSignals32,
        binding: EventBinding32,
        count: u64,
    ) -> Result<(), Error> {
        let slot = self.validate_binding(binding)?;
        let count = count.min(if self.events[slot].as_ref().unwrap().manual {
            1
        } else {
            self.waits.len() as u64 + 1
        });
        for _ in 0..count {
            set(&mut prepared.signaled, slot, true);
            self.resolve_waiters(prepared);
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
        self.resolve_waiters(&mut prepared);
        for slot in pulse {
            set(&mut prepared.signaled, *slot, false);
        }
        Ok(prepared)
    }
    fn signal_plan(
        &self,
        now: u64,
        queue: Option<(u32, u32, u64)>,
    ) -> Result<PreparedSignals32, Error> {
        if now < self.now {
            return Err(Error::ClockBackwards);
        }
        Ok(PreparedSignals32 {
            epoch: self.epoch,
            revision: self.revision,
            next_revision: self.next_revision()?,
            now,
            signaled: self.signaled,
            completed: [None; MAX_WAITS],
            queue,
        })
    }
    fn resolve_waiters(&self, prepared: &mut PreparedSignals32) {
        let mut order = [usize::MAX; MAX_WAITS];
        let mut count = 0;
        for (slot, wait) in self.waits.iter().enumerate() {
            if wait.as_ref().is_some_and(|wait| wait.result.is_none())
                && prepared.completed[slot].is_none()
            {
                order[count] = slot;
                count += 1;
            }
        }
        order[..count].sort_unstable_by_key(|slot| self.waits[*slot].as_ref().unwrap().id);
        let (current, new, _) =
            prepared
                .queue
                .unwrap_or((self.queue_current, self.queue_new, self.queue_generation));
        for slot in &order[..count] {
            let wait = self.waits[*slot].as_ref().unwrap();
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
                self.resolve(wait, &mut prepared.signaled, current, new)
            };
            prepared.completed[*slot] = result;
        }
    }
    pub fn commit_signals(&mut self, prepared: PreparedSignals32) {
        assert_eq!(
            (self.epoch, self.revision),
            (prepared.epoch, prepared.revision)
        );
        self.signaled = prepared.signaled;
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
        if let Some((current, new, generation)) = prepared.queue {
            self.queue_current = current;
            self.queue_new = new;
            self.queue_generation = generation;
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
    pub fn prepare_time(&self, now_ns: u64) -> Result<PreparedSignals32, Error> {
        self.prepare_signal_slots(&[], &[], now_ns, None)
    }
    pub fn observe_messages(
        &mut self,
        current: u32,
        new: u32,
        generation: u64,
    ) -> Result<(), Error> {
        if generation < self.queue_generation {
            return Err(Error::Stale);
        }
        if (current, new, generation) == (self.queue_current, self.queue_new, self.queue_generation)
        {
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
        if generation < self.queue_generation {
            return Err(Error::Stale);
        }
        self.prepare_signal_slots(&[], &[], self.now, Some((current, new, generation)))
    }
    pub fn wait(
        &mut self,
        handles: &[u32],
        mode: WaitMode32,
        timeout_ms: u32,
        alertable: bool,
        message: Option<MessageWait32>,
    ) -> Result<WaitOutcome32, Error> {
        let wait = self.validated_wait(handles, mode, timeout_ms, alertable, message)?;
        let revision = self.next_revision()?;
        let mut signaled = self.signaled;
        if let Some(result) = self.resolve(&wait, &mut signaled, self.queue_current, self.queue_new)
        {
            self.signaled = signaled;
            self.revision = revision;
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
        self.revision = revision;
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
        };
        for (index, handle) in handles.iter().enumerate() {
            let event = self.handles[self.handle_index(*handle, SYNCHRONIZE)?].event;
            if wait.handles[..index].contains(handle)
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
        let signal = self.handles[self.handle_index(signal, super::EVENT_MODIFY_STATE)?].event;
        let wait =
            self.validated_wait(&[wait_handle], WaitMode32::Any, timeout_ms, alertable, None)?;
        let mut prepared = self.prepare_signal_slots(&[signal], &[], self.now, None)?;
        let result = self.resolve(
            &wait,
            &mut prepared.signaled,
            self.queue_current,
            self.queue_new,
        );
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
    fn resolve(
        &self,
        wait: &Wait32,
        signaled: &mut [u64; STATE_WORDS],
        current: u32,
        new: u32,
    ) -> Option<WaitResult32> {
        if wait.mode == WaitMode32::Sleep {
            return None;
        }
        let messages = wait.message.is_some_and(|message| {
            (if message.input_available {
                current
            } else {
                new
            }) & message.mask
                != 0
        });
        let selected = if wait.mode == WaitMode32::All {
            if wait.events[..wait.count]
                .iter()
                .any(|event| !bit(signaled, *event))
                || wait.message.is_some() && !messages
            {
                return None;
            }
            0
        } else if let Some(index) = wait.events[..wait.count]
            .iter()
            .position(|event| bit(signaled, *event))
        {
            index
        } else if messages {
            wait.count
        } else {
            return None;
        };
        for (index, slot) in wait.events[..wait.count].iter().enumerate() {
            if (wait.mode == WaitMode32::All || index == selected)
                && !self.events[*slot].as_ref().unwrap().manual
            {
                set(signaled, *slot, false);
            }
        }
        Some(WaitResult32 {
            value: selected as u32,
            last_error: None,
        })
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
