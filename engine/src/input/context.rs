use super::{
    BUTTON_COUNT, InputError, InputKind, InputLease, InputLimits, InputOrigin, InputRecord,
    InputState, KEY_COUNT, MAX_BATCH_EVENTS, MAX_QUEUE_EVENTS, MIN_QUEUE_EVENTS, RelativeTotals,
    TimedInputEvent, wire::EncodedBatch,
};

pub struct InputContext {
    pub(super) epoch: u64,
    clock_epoch_ns: u64,
    limits: InputLimits,
    sequence: u64,
    timestamp_ns: u64,
    state: InputState,
    focus_generation: u64,
    capture_generation: u64,
    pub(super) queue: Box<[InputRecord]>,
    pub(super) head: usize,
    pub(super) length: usize,
}

impl InputContext {
    /// the root owns unique session epochs and the shared monotonic clock origin.
    pub fn new(
        session_epoch: u64,
        clock_epoch_ns: u64,
        limits: InputLimits,
    ) -> Result<Self, InputError> {
        if session_epoch == 0
            || !(MIN_QUEUE_EVENTS..=MAX_QUEUE_EVENTS).contains(&limits.queue_events)
            || !(1..=MAX_BATCH_EVENTS).contains(&limits.batch_events)
        {
            return Err(InputError::InvalidLimits);
        }
        let mut queue = Vec::new();
        queue
            .try_reserve_exact(limits.queue_events)
            .map_err(|_| InputError::Allocation)?;
        queue.resize(limits.queue_events, InputRecord::default());
        Ok(Self {
            epoch: session_epoch,
            clock_epoch_ns,
            limits,
            sequence: 0,
            timestamp_ns: clock_epoch_ns,
            state: InputState::default(),
            focus_generation: 0,
            capture_generation: 0,
            queue: queue.into_boxed_slice(),
            head: 0,
            length: 0,
        })
    }

    pub fn session_epoch(&self) -> u64 {
        self.epoch
    }

    pub fn state(&self) -> &InputState {
        &self.state
    }

    pub fn queued_events(&self) -> usize {
        self.length
    }

    pub fn last_sequence(&self) -> u64 {
        self.sequence
    }

    pub fn last_timestamp_ns(&self) -> u64 {
        self.timestamp_ns
    }

    pub fn submit_batch(
        &mut self,
        epoch: u64,
        sequence: u64,
        events: &[TimedInputEvent],
    ) -> Result<(), InputError> {
        self.submit_events(epoch, sequence, events.iter().copied())
    }

    pub fn submit_encoded(&mut self, batch: &EncodedBatch<'_>) -> Result<(), InputError> {
        self.submit_events(batch.session_epoch(), batch.sequence(), batch.events())
    }

    fn submit_events(
        &mut self,
        epoch: u64,
        sequence: u64,
        events: impl ExactSizeIterator<Item = TimedInputEvent> + Clone,
    ) -> Result<(), InputError> {
        if epoch != self.epoch {
            return Err(InputError::StaleSession);
        }
        if sequence != self.sequence.checked_add(1).ok_or(InputError::Exhausted)? {
            return Err(InputError::SequenceOrder);
        }
        if events.len() == 0 || events.len() > self.limits.batch_events {
            return Err(InputError::InvalidCount);
        }
        let mut state = self.state;
        let mut focus_generation = self.focus_generation;
        let mut capture_generation = self.capture_generation;
        let mut timestamp_ns = self.timestamp_ns;
        let mut required = 0;
        let available = self.queue.len() - self.length;
        for event in events.clone() {
            if event.timestamp_ns < self.clock_epoch_ns || event.timestamp_ns < timestamp_ns {
                return Err(InputError::TimeOrder);
            }
            transition(
                &mut state,
                &mut focus_generation,
                &mut capture_generation,
                event,
                |_, _| {
                    required += 1;
                    if required > available {
                        return Err(InputError::QueueFull);
                    }
                    Ok(())
                },
            )?;
            timestamp_ns = event.timestamp_ns;
        }
        let mut published_state = self.state;
        let mut published_focus = self.focus_generation;
        let mut published_capture = self.capture_generation;
        for event in events {
            transition(
                &mut published_state,
                &mut published_focus,
                &mut published_capture,
                event,
                |event, origin| {
                    self.push(InputRecord {
                        batch_sequence: sequence,
                        event,
                        origin,
                    });
                    Ok(())
                },
            )
            .expect("input transitions were preflighted against unchanged state");
        }
        self.state = state;
        self.focus_generation = focus_generation;
        self.capture_generation = capture_generation;
        self.timestamp_ns = timestamp_ns;
        self.sequence = sequence;
        Ok(())
    }

    pub fn acquire(&self, require_capture: bool) -> Result<InputLease, InputError> {
        if !self.state.focused {
            return Err(InputError::NotFocused);
        }
        if require_capture && !self.state.captured {
            return Err(InputError::NotCaptured);
        }
        Ok(InputLease {
            session_epoch: self.epoch,
            focus_generation: self.focus_generation,
            capture_generation: require_capture.then_some(self.capture_generation),
        })
    }

    pub fn lease_valid(&self, lease: InputLease) -> bool {
        lease.session_epoch == self.epoch
            && lease.focus_generation == self.focus_generation
            && self.state.focused
            && lease.capture_generation.is_none_or(|generation| {
                self.state.captured && generation == self.capture_generation
            })
    }

    pub fn peek(&self, maximum: usize, output: &mut [InputRecord]) -> Result<usize, InputError> {
        let count = maximum.min(self.length);
        if output.len() < count {
            return Err(InputError::OutputCapacity);
        }
        for (index, record) in output[..count].iter_mut().enumerate() {
            *record = self.record(index);
        }
        Ok(count)
    }

    pub fn read(
        &mut self,
        maximum: usize,
        output: &mut [InputRecord],
    ) -> Result<usize, InputError> {
        let count = self.peek(maximum, output)?;
        self.consume(count)?;
        Ok(count)
    }

    pub fn consume(&mut self, count: usize) -> Result<(), InputError> {
        if count > self.length {
            return Err(InputError::InvalidCount);
        }
        self.head = (self.head + count) % self.queue.len();
        self.length -= count;
        Ok(())
    }

    pub fn drain_relative(&mut self) -> RelativeTotals {
        std::mem::take(&mut self.state.relative)
    }

    pub fn reset(&mut self, new_epoch: u64, now_ns: u64) -> Result<(), InputError> {
        if new_epoch <= self.epoch {
            return Err(InputError::StaleSession);
        }
        if now_ns < self.timestamp_ns {
            return Err(InputError::TimeOrder);
        }
        self.epoch = new_epoch;
        self.sequence = 0;
        self.timestamp_ns = now_ns;
        self.state = InputState::default();
        self.focus_generation = 0;
        self.capture_generation = 0;
        self.head = 0;
        self.length = 0;
        Ok(())
    }

    pub(super) fn record(&self, offset: usize) -> InputRecord {
        self.queue[(self.head + offset) % self.queue.len()]
    }

    fn push(&mut self, record: InputRecord) {
        debug_assert!(self.length < self.queue.len());
        let index = (self.head + self.length) % self.queue.len();
        self.queue[index] = record;
        self.length += 1;
    }
}

fn transition(
    state: &mut InputState,
    focus_generation: &mut u64,
    capture_generation: &mut u64,
    event: TimedInputEvent,
    mut emit: impl FnMut(TimedInputEvent, InputOrigin) -> Result<(), InputError>,
) -> Result<(), InputError> {
    match event.kind {
        InputKind::Key { code, pressed } => {
            let key = state
                .keys
                .get_mut(usize::from(code))
                .ok_or(InputError::InvalidEvent)?;
            if pressed && !state.focused {
                return Err(InputError::NotFocused);
            }
            *key = pressed;
        }
        InputKind::Button { button, pressed } => {
            let button = state
                .buttons
                .get_mut(usize::from(button))
                .ok_or(InputError::InvalidEvent)?;
            if pressed && !state.focused {
                return Err(InputError::NotFocused);
            }
            *button = pressed;
        }
        InputKind::Relative { dx, dy, wheel } => {
            if !state.focused {
                return Err(InputError::NotFocused);
            }
            state.relative.dx = state
                .relative
                .dx
                .checked_add(i64::from(dx))
                .ok_or(InputError::Exhausted)?;
            state.relative.dy = state
                .relative
                .dy
                .checked_add(i64::from(dy))
                .ok_or(InputError::Exhausted)?;
            state.relative.wheel = state
                .relative
                .wheel
                .checked_add(i64::from(wheel))
                .ok_or(InputError::Exhausted)?;
        }
        InputKind::Focus { focused } => {
            if !focused {
                if state.focused {
                    *focus_generation = focus_generation
                        .checked_add(1)
                        .ok_or(InputError::Exhausted)?;
                }
                if state.captured {
                    *capture_generation = capture_generation
                        .checked_add(1)
                        .ok_or(InputError::Exhausted)?;
                }
                for index in 0..KEY_COUNT {
                    if state.keys[index] {
                        state.keys[index] = false;
                        emit(
                            TimedInputEvent {
                                timestamp_ns: event.timestamp_ns,
                                kind: InputKind::Key {
                                    code: index as u16,
                                    pressed: false,
                                },
                            },
                            InputOrigin::FocusRelease,
                        )?;
                    }
                }
                for index in 0..BUTTON_COUNT {
                    if state.buttons[index] {
                        state.buttons[index] = false;
                        emit(
                            TimedInputEvent {
                                timestamp_ns: event.timestamp_ns,
                                kind: InputKind::Button {
                                    button: index as u8,
                                    pressed: false,
                                },
                            },
                            InputOrigin::FocusRelease,
                        )?;
                    }
                }
                state.captured = false;
                state.relative = RelativeTotals::default();
            }
            state.focused = focused;
        }
        InputKind::Capture { captured } => {
            if captured && !state.focused {
                return Err(InputError::NotFocused);
            }
            if !captured && state.captured {
                *capture_generation = capture_generation
                    .checked_add(1)
                    .ok_or(InputError::Exhausted)?;
            }
            state.captured = captured;
        }
    }
    emit(event, InputOrigin::Host)
}
