use super::state::{SourceEvent32, SourceKind32, SourceSnapshot32, physical};
use crate::input::{InputError, MAX_QUEUE_EVENTS, MIN_QUEUE_EVENTS, TimedInputEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputCursor32 {
    pub epoch: u64,
    pub sequence: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceRead32 {
    pub next: InputCursor32,
    pub count: usize,
    pub overflow: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct PreparedSourceWindow32 {
    epoch: u64,
    sequence: u64,
    previous_timestamp: u64,
    timestamp: u64,
}

pub struct InputSource32 {
    pub(super) epoch: u64,
    sequence: u64,
    pub(super) timestamp_ns: u64,
    pub(super) state: SourceSnapshot32,
    records: Box<[SourceEvent32]>,
    length: usize,
}
impl InputSource32 {
    pub fn new(epoch: u64, now_ns: u64, capacity: usize) -> Result<Self, InputError> {
        if epoch == 0 || !(MIN_QUEUE_EVENTS..=MAX_QUEUE_EVENTS).contains(&capacity) {
            return Err(InputError::InvalidLimits);
        }
        let mut records = Vec::new();
        records
            .try_reserve_exact(capacity)
            .map_err(|_| InputError::Allocation)?;
        records.resize(capacity, SourceEvent32::default());
        Ok(Self {
            epoch,
            sequence: 0,
            timestamp_ns: now_ns,
            state: SourceSnapshot32::default(),
            records: records.into_boxed_slice(),
            length: 0,
        })
    }
    pub fn snapshot(&self) -> &SourceSnapshot32 {
        &self.state
    }
    pub fn tail(&self) -> InputCursor32 {
        InputCursor32 {
            epoch: self.epoch,
            sequence: self.sequence,
        }
    }
    pub(crate) fn last_timestamp_ns(&self) -> u64 {
        self.timestamp_ns
    }
    pub fn check_physical(
        &self,
        events: impl ExactSizeIterator<Item = TimedInputEvent> + Clone,
    ) -> Result<(), InputError> {
        let mut state = self.state;
        let mut count = 0u64;
        let mut timestamp = self.timestamp_ns;
        for event in events {
            if event.timestamp_ns < timestamp {
                return Err(InputError::TimeOrder);
            }
            physical(&mut state, event, |_| count += 1)?;
            timestamp = event.timestamp_ns;
        }
        self.sequence
            .checked_add(count)
            .ok_or(InputError::Exhausted)?;
        Ok(())
    }
    pub fn publish_physical(
        &mut self,
        events: impl ExactSizeIterator<Item = TimedInputEvent> + Clone,
    ) -> Result<(), InputError> {
        self.check_physical(events.clone())?;
        let mut state = self.state;
        for event in events {
            physical(&mut state, event, |kind| {
                self.push(event.timestamp_ns, kind)
            })
            .expect("physical source was preflighted");
            self.timestamp_ns = event.timestamp_ns;
        }
        self.state = state;
        Ok(())
    }
    pub fn read(
        &self,
        cursor: InputCursor32,
        output: &mut [SourceEvent32],
    ) -> Result<SourceRead32, InputError> {
        if cursor.epoch != self.epoch {
            return Err(InputError::StaleSession);
        }
        if cursor.sequence > self.sequence {
            return Err(InputError::SequenceOrder);
        }
        let earliest = self.sequence - self.length as u64;
        let start = cursor.sequence.max(earliest);
        let count = ((self.sequence - start) as usize).min(output.len());
        for (index, record) in output[..count].iter_mut().enumerate() {
            let sequence = start + index as u64 + 1;
            *record = self.records[((sequence - 1) % self.records.len() as u64) as usize];
        }
        Ok(SourceRead32 {
            next: InputCursor32 {
                epoch: self.epoch,
                sequence: start + count as u64,
            },
            count,
            overflow: cursor.sequence < earliest,
        })
    }
    pub fn publish_cursor(&mut self, timestamp_ns: u64, x: i32, y: i32) -> Result<(), InputError> {
        self.check_one(timestamp_ns)?;
        let (x, y) = super::state::clamp_cursor((x, y), self.state.cursor_bounds);
        self.state.cursor = (x, y);
        self.push(timestamp_ns, SourceKind32::Cursor { x, y });
        Ok(())
    }
    pub fn can_retain_from(&self, cursor: InputCursor32, additional: usize) -> bool {
        cursor.epoch == self.epoch
            && cursor.sequence <= self.sequence
            && (self.sequence - cursor.sequence)
                .checked_add(additional as u64)
                .is_some_and(|count| count <= self.records.len() as u64)
    }
    pub fn commit_translation_policy(
        &mut self,
        prepared: PreparedSourceWindow32,
        layout: crate::input::keyboard::KeyboardLayout32,
        swapped: bool,
    ) {
        assert!(
            self.epoch == prepared.epoch
                && self.sequence == prepared.sequence
                && self.timestamp_ns == prepared.previous_timestamp,
            "prepared source owner changed"
        );
        self.push(
            prepared.timestamp,
            SourceKind32::TranslationPolicy { layout, swapped },
        );
    }
    pub fn commit_process_translation_policy(
        &mut self,
        prepared: PreparedSourceWindow32,
        layout: crate::input::keyboard::KeyboardLayout32,
        swapped: bool,
    ) {
        assert!(
            self.epoch == prepared.epoch
                && self.sequence == prepared.sequence
                && self.timestamp_ns == prepared.previous_timestamp,
            "prepared source owner changed"
        );
        self.push(
            prepared.timestamp,
            SourceKind32::ProcessTranslationPolicy { layout, swapped },
        );
    }
    pub fn commit_thread_translation_policy(
        &mut self,
        prepared: PreparedSourceWindow32,
        thread: u32,
        layout: crate::input::keyboard::KeyboardLayout32,
        swapped: bool,
    ) {
        assert!(
            thread != 0
                && self.epoch == prepared.epoch
                && self.sequence == prepared.sequence
                && self.timestamp_ns == prepared.previous_timestamp,
            "prepared source owner changed"
        );
        self.push(
            prepared.timestamp,
            SourceKind32::ThreadTranslationPolicy {
                thread,
                layout,
                swapped,
            },
        );
    }
    pub fn commit_cursor_policy(
        &mut self,
        prepared: PreparedSourceWindow32,
        x: i32,
        y: i32,
        bounds: Option<(i32, i32, i32, i32)>,
    ) {
        assert!(
            self.epoch == prepared.epoch
                && self.sequence == prepared.sequence
                && self.timestamp_ns == prepared.previous_timestamp,
            "prepared source owner changed"
        );
        let (x, y) = super::state::clamp_cursor((x, y), bounds);
        self.state.cursor_bounds = bounds;
        self.state.cursor = (x, y);
        self.push(
            prepared.timestamp,
            SourceKind32::CursorPolicy { x, y, bounds },
        );
    }
    pub fn commit_cursor(&mut self, prepared: PreparedSourceWindow32, x: i32, y: i32) {
        assert!(
            self.epoch == prepared.epoch
                && self.sequence == prepared.sequence
                && self.timestamp_ns == prepared.previous_timestamp,
            "prepared source owner changed"
        );
        let (x, y) = super::state::clamp_cursor((x, y), self.state.cursor_bounds);
        self.state.cursor = (x, y);
        self.push(prepared.timestamp, SourceKind32::Cursor { x, y });
    }
    pub fn commit_policy(
        &mut self,
        prepared: PreparedSourceWindow32,
        mouse_exclusive: bool,
        no_windows_key: bool,
    ) {
        assert!(
            self.epoch == prepared.epoch
                && self.sequence == prepared.sequence
                && self.timestamp_ns == prepared.previous_timestamp,
            "prepared source owner changed"
        );
        self.state.mouse_exclusive = mouse_exclusive;
        self.state.no_windows_key = no_windows_key;
        self.push(
            prepared.timestamp,
            SourceKind32::Policy {
                mouse_exclusive,
                no_windows_key,
            },
        );
    }
    pub fn prepare_window(&self, timestamp_ns: u64) -> Result<PreparedSourceWindow32, InputError> {
        self.check_one(timestamp_ns)?;
        Ok(PreparedSourceWindow32 {
            epoch: self.epoch,
            sequence: self.sequence,
            previous_timestamp: self.timestamp_ns,
            timestamp: timestamp_ns,
        })
    }
    pub fn commit_window(&mut self, prepared: PreparedSourceWindow32, focus: u32, capture: u32) {
        assert!(
            self.epoch == prepared.epoch
                && self.sequence == prepared.sequence
                && self.timestamp_ns == prepared.previous_timestamp,
            "prepared source owner changed"
        );
        self.push(prepared.timestamp, SourceKind32::Window { focus, capture });
    }
    pub fn check_delivery_capacity(
        &self,
        events: impl ExactSizeIterator<Item = TimedInputEvent> + Clone,
    ) -> Result<(), InputError> {
        let mut state = self.state;
        let mut count = 0usize;
        for event in events {
            physical(&mut state, event, |_| count += 1)?;
        }
        if count > self.records.len() {
            Err(InputError::QueueFull)
        } else {
            Ok(())
        }
    }
    pub fn publish_window(
        &mut self,
        timestamp_ns: u64,
        focus: u32,
        capture: u32,
    ) -> Result<(), InputError> {
        self.check_one(timestamp_ns)?;
        self.push(timestamp_ns, SourceKind32::Window { focus, capture });
        Ok(())
    }
    pub fn reset(&mut self, epoch: u64, now_ns: u64) -> Result<(), InputError> {
        if epoch <= self.epoch {
            return Err(InputError::StaleSession);
        }
        if now_ns < self.timestamp_ns {
            return Err(InputError::TimeOrder);
        }
        self.epoch = epoch;
        self.sequence = 0;
        self.timestamp_ns = now_ns;
        self.state = SourceSnapshot32::default();
        self.length = 0;
        Ok(())
    }
    pub(super) fn check_one(&self, time: u64) -> Result<(), InputError> {
        self.check_count(time, 1)
    }
    pub(super) fn check_count(&self, time: u64, count: u64) -> Result<(), InputError> {
        if time < self.timestamp_ns {
            return Err(InputError::TimeOrder);
        }
        self.sequence
            .checked_add(count)
            .ok_or(InputError::Exhausted)?;
        Ok(())
    }
    pub(super) fn push_group(&mut self, timestamp_ns: u64, kind: SourceKind32, sequence: u32) {
        self.push(timestamp_ns, kind);
        let index = ((self.sequence - 1) % self.records.len() as u64) as usize;
        self.records[index].input_sequence = sequence;
    }
    pub(super) fn push(&mut self, timestamp_ns: u64, kind: SourceKind32) {
        self.sequence += 1;
        self.timestamp_ns = timestamp_ns;
        let index = ((self.sequence - 1) % self.records.len() as u64) as usize;
        self.records[index] = SourceEvent32 {
            sequence: self.sequence,
            timestamp_ns,
            input_sequence: self.sequence as u32,
            kind,
        };
        self.length = (self.length + 1).min(self.records.len());
    }
}
