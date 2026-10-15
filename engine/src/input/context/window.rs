use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct PhysicalInput32 {
    pub focused: bool,
    pub captured: bool,
}

pub(crate) struct PreparedWindowInput32 {
    epoch: u64,
    sequence: u64,
    previous_timestamp: u64,
    head: usize,
    length: usize,
    previous_state: InputState,
    previous_focus: u64,
    previous_capture: u64,
    physical: PhysicalInput32,
    previous_flags: Option<(bool, bool)>,
    mirror_window_events: bool,
    flags: (bool, bool),
    state: InputState,
    focus: u64,
    capture: u64,
    timestamp: u64,
    records: [InputRecord; KEY_COUNT + BUTTON_COUNT + 2],
    count: usize,
}

impl InputContext {
    pub(crate) fn prepare_window_flags(
        &self,
        focused: bool,
        captured: bool,
        timestamp_ns: u64,
    ) -> Result<PreparedWindowInput32, InputError> {
        if timestamp_ns < self.clock_epoch_ns || timestamp_ns < self.timestamp_ns {
            return Err(InputError::TimeOrder);
        }
        let mut prepared = PreparedWindowInput32 {
            epoch: self.epoch,
            sequence: self.sequence,
            previous_timestamp: self.timestamp_ns,
            head: self.head,
            length: self.length,
            previous_state: self.state,
            previous_focus: self.focus_generation,
            previous_capture: self.capture_generation,
            physical: self.physical,
            previous_flags: self.window_flags,
            mirror_window_events: self.mirror_window_events,
            flags: (focused, captured),
            state: self.state,
            focus: self.focus_generation,
            capture: self.capture_generation,
            timestamp: timestamp_ns,
            records: [InputRecord::default(); KEY_COUNT + BUTTON_COUNT + 2],
            count: 0,
        };
        let effective_focus = self.physical.focused && focused;
        let effective_capture = self.physical.captured && captured && effective_focus;
        let available = if self.mirror_window_events {
            self.queue.len() - self.length
        } else {
            usize::MAX
        };
        let sequence = self.sequence;
        let mut emit = |event, origin| {
            if prepared.count == available || prepared.count == prepared.records.len() {
                return Err(InputError::QueueFull);
            }
            prepared.records[prepared.count] = InputRecord {
                batch_sequence: sequence,
                event,
                origin: if origin == InputOrigin::Host {
                    InputOrigin::Window
                } else {
                    origin
                },
            };
            prepared.count += 1;
            Ok(())
        };
        if prepared.state.focused != effective_focus {
            transition(
                &mut prepared.state,
                &mut prepared.focus,
                &mut prepared.capture,
                TimedInputEvent {
                    timestamp_ns,
                    kind: InputKind::Focus {
                        focused: effective_focus,
                    },
                },
                &mut emit,
            )?;
        }
        if prepared.state.captured != effective_capture {
            transition(
                &mut prepared.state,
                &mut prepared.focus,
                &mut prepared.capture,
                TimedInputEvent {
                    timestamp_ns,
                    kind: InputKind::Capture {
                        captured: effective_capture,
                    },
                },
                emit,
            )?;
        }
        Ok(prepared)
    }

    pub(crate) fn commit_window_flags(&mut self, prepared: PreparedWindowInput32) {
        assert!(
            self.epoch == prepared.epoch
                && self.sequence == prepared.sequence
                && self.timestamp_ns == prepared.previous_timestamp
                && self.head == prepared.head
                && self.length == prepared.length
                && self.state == prepared.previous_state
                && self.focus_generation == prepared.previous_focus
                && self.capture_generation == prepared.previous_capture
                && self.physical == prepared.physical
                && self.window_flags == prepared.previous_flags,
            "prepared window input owner changed"
        );
        assert_eq!(
            self.mirror_window_events, prepared.mirror_window_events,
            "prepared input delivery changed"
        );
        if self.mirror_window_events {
            for record in prepared.records[..prepared.count].iter().copied() {
                self.push(record);
            }
        }
        self.window_flags = Some(prepared.flags);
        self.state = prepared.state;
        self.focus_generation = prepared.focus;
        self.capture_generation = prepared.capture;
        self.timestamp_ns = prepared.timestamp;
    }
}

pub(super) fn host_transition(
    state: &mut InputState,
    focus: &mut u64,
    capture: &mut u64,
    physical: &mut PhysicalInput32,
    flags: Option<(bool, bool)>,
    event: TimedInputEvent,
    mut emit: impl FnMut(TimedInputEvent, InputOrigin) -> Result<(), InputError>,
) -> Result<(), InputError> {
    let Some((window_focus, window_capture)) = flags else {
        transition(state, focus, capture, event, &mut emit)?;
        match event.kind {
            InputKind::Focus { focused } => {
                physical.focused = focused;
                if !focused {
                    physical.captured = false;
                }
            }
            InputKind::Capture { captured } => physical.captured = captured,
            _ => {}
        }
        return Ok(());
    };
    match event.kind {
        InputKind::Focus { focused } => {
            physical.focused = focused;
            if !focused {
                physical.captured = false;
            }
            let effective = physical.focused && window_focus;
            transition(
                state,
                focus,
                capture,
                TimedInputEvent {
                    kind: InputKind::Focus { focused: effective },
                    ..event
                },
                &mut emit,
            )?;
            let captured = physical.captured && window_capture && effective;
            if state.captured != captured {
                transition(
                    state,
                    focus,
                    capture,
                    TimedInputEvent {
                        kind: InputKind::Capture { captured },
                        ..event
                    },
                    emit,
                )?;
            }
        }
        InputKind::Capture { captured } => {
            if captured && !physical.focused {
                return Err(InputError::NotFocused);
            }
            physical.captured = captured;
            transition(
                state,
                focus,
                capture,
                TimedInputEvent {
                    kind: InputKind::Capture {
                        captured: captured && window_capture && physical.focused && window_focus,
                    },
                    ..event
                },
                emit,
            )?;
        }
        InputKind::Key { code, .. } if !state.focused => {
            if usize::from(code) >= KEY_COUNT {
                return Err(InputError::InvalidEvent);
            }
        }
        InputKind::Button { button, .. } if !state.focused => {
            if usize::from(button) >= BUTTON_COUNT {
                return Err(InputError::InvalidEvent);
            }
        }
        InputKind::Relative { .. } if !state.focused => {}
        _ => transition(state, focus, capture, event, emit)?,
    }
    Ok(())
}

#[cfg(test)]
mod tests;
