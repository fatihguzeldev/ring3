use super::{ControllerState32, InputSource32, SourceKind32};
use crate::input::InputError;

impl InputSource32 {
    pub fn publish_controller(
        &mut self,
        epoch: u64,
        slot: u8,
        timestamp_ns: u64,
        state: ControllerState32,
    ) -> Result<(), InputError> {
        if epoch != self.epoch {
            return Err(InputError::StaleSession);
        }
        let old = *self
            .state
            .controllers
            .get(slot as usize)
            .ok_or(InputError::InvalidEvent)?;
        if state
            .axes
            .iter()
            .any(|axis| !(-32768..=32767).contains(axis))
            || state.pov.iter().any(|pov| *pov != u32::MAX && *pov > 35999)
        {
            return Err(InputError::InvalidEvent);
        }
        let state = if state.connected {
            state
        } else {
            ControllerState32::default()
        };
        let mut count = 0;
        changes(old, state, slot, |_| count += 1);
        self.check_count(timestamp_ns, count)?;
        let group = self.tail().sequence.wrapping_add(1) as u32;
        changes(old, state, slot, |event| {
            self.push_group(timestamp_ns, event, group)
        });
        self.state.controllers[slot as usize] = state;
        self.timestamp_ns = timestamp_ns;
        Ok(())
    }
}

fn changes(
    old: ControllerState32,
    new: ControllerState32,
    slot: u8,
    mut emit: impl FnMut(SourceKind32),
) {
    if old.connected != new.connected {
        emit(SourceKind32::Connection {
            slot,
            connected: new.connected,
        });
    }
    for (index, axis) in new.axes.iter().enumerate() {
        if old.axes[index] != *axis {
            emit(SourceKind32::Controller {
                slot,
                object: index as u16,
                value: *axis as u32,
            });
        }
    }
    for (index, pov) in new.pov.iter().enumerate() {
        if old.pov[index] != *pov {
            emit(SourceKind32::Controller {
                slot,
                object: 0x100 + index as u16,
                value: *pov,
            });
        }
    }
    for (index, pressed) in new.buttons.iter().enumerate() {
        if old.buttons[index] != *pressed {
            emit(SourceKind32::Controller {
                slot,
                object: 0x200 + index as u16,
                value: if *pressed { 0x80 } else { 0 },
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{MIN_QUEUE_EVENTS, source::SourceEvent32};
    #[test]
    fn controller_batches_validate_before_publication_and_disconnect_releases_all_state() {
        let mut source = InputSource32::new(1, 0, MIN_QUEUE_EVENTS).unwrap();
        let mut state = ControllerState32 {
            connected: true,
            ..Default::default()
        };
        state.axes[0] = 20000;
        state.buttons[127] = true;
        state.pov[3] = 9000;
        source.publish_controller(1, 0, 1, state).unwrap();
        let before = source.tail();
        let snapshot = *source.snapshot();
        state.axes[0] = 32768;
        assert_eq!(
            source.publish_controller(1, 0, 2, state),
            Err(InputError::InvalidEvent)
        );
        assert_eq!(*source.snapshot(), snapshot);
        assert_eq!(source.tail(), before);
        source
            .publish_controller(1, 0, 3, ControllerState32::default())
            .unwrap();
        let mut events = [SourceEvent32::default(); 8];
        assert_eq!(source.read(before, &mut events).unwrap().count, 4);
        assert_eq!(
            source.snapshot().controllers[0],
            ControllerState32::default()
        );
    }
}
