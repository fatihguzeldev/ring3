use super::super::{DirectInputError32, DirectInputRuntime32, objects::ObjectKind32};
use crate::{
    input::{
        dinput::{
            DeviceKind32,
            device::Acquisition32,
            haptics::{HapticCapabilities32, HapticFrame32},
        },
        source::ControllerState32,
    },
    process::{EngineInstance, HostError},
};

impl EngineInstance {
    pub fn configure_controller_haptics(
        &mut self,
        epoch: u64,
        slot: u8,
        capabilities: HapticCapabilities32,
    ) -> Result<(), HostError> {
        if self.media_contexts()?.input.session_epoch() != epoch {
            return Err(invalid());
        }
        capabilities.validate().map_err(|_| invalid())?;
        let runtime = self
            .dinput
            .as_mut()
            .ok_or(HostError::DirectInput(DirectInputError32::NotConfigured))?;
        let index = usize::from(slot);
        let descriptor = runtime
            .controllers
            .get(index)
            .and_then(Option::as_ref)
            .ok_or_else(invalid)?;
        if capabilities.axes>descriptor.axes||runtime.haptic_capabilities[index].is_some()
            ||runtime.records.iter().flatten().any(|record|matches!(&record.kind,ObjectKind32::Device(device) if device.kind==DeviceKind32::Controller(slot))) {return Err(invalid())}
        runtime.haptic_capabilities[index] = Some(capabilities);
        Ok(())
    }
    pub fn render_dinput_haptics(
        &mut self,
        epoch: u64,
        slot: u8,
        timestamp_ns: u64,
    ) -> Result<HapticFrame32, HostError> {
        self.sync_dinput_input()?;
        if self.media_contexts()?.input.session_epoch() != epoch
            || timestamp_ns > self.cpu_clock.monotonic_bounds()?.1
        {
            return Err(invalid());
        }
        let state = *self
            .media_contexts()?
            .input
            .source()
            .snapshot()
            .controllers
            .get(usize::from(slot))
            .ok_or_else(invalid)?;
        let runtime = self
            .dinput
            .as_mut()
            .ok_or(HostError::DirectInput(DirectInputError32::NotConfigured))?;
        let index = usize::from(slot);
        if runtime.haptic_capabilities[index].is_none() || timestamp_ns < runtime.haptic_time[index]
        {
            return Err(invalid());
        }
        if runtime.haptic_frames[index].is_some() {
            return Err(HostError::DirectInput(DirectInputError32::Busy));
        }
        let sequence = runtime.haptic_sequence.checked_add(1).ok_or_else(invalid)?;
        let mut forces = [0; 8];
        for record in runtime.records.iter_mut().flatten() {
            if let ObjectKind32::Device(device) = &mut record.kind
                && device.kind == DeviceKind32::Controller(slot)
            {
                let exclusive = state.connected
                    && device.native.acquisition() == Acquisition32::Acquired
                    && device
                        .native
                        .cooperative()
                        .is_some_and(|level| level.exclusive);
                if let Some(haptics) = &mut device.haptics {
                    let values = haptics.evaluate(
                        timestamp_ns,
                        &state.axes,
                        &button_bits(&state),
                        exclusive,
                    );
                    if exclusive {
                        forces = values;
                    }
                }
            }
        }
        let frame = HapticFrame32 {
            epoch,
            sequence,
            slot,
            timestamp_ns,
            forces,
        };
        runtime.haptic_frames[index] = Some(frame);
        runtime.haptic_sequence = sequence;
        runtime.haptic_time[index] = timestamp_ns;
        Ok(frame)
    }
    pub fn pending_dinput_haptics(&self, slot: u8) -> Result<Option<HapticFrame32>, HostError> {
        self.media_contexts()?;
        self.dinput
            .as_ref()
            .ok_or(HostError::DirectInput(DirectInputError32::NotConfigured))?
            .haptic_frames
            .get(usize::from(slot))
            .copied()
            .ok_or_else(invalid)
    }
    pub fn acknowledge_dinput_haptics(
        &mut self,
        epoch: u64,
        slot: u8,
        sequence: u64,
    ) -> Result<(), HostError> {
        if self.media_contexts()?.input.session_epoch() != epoch {
            return Err(invalid());
        }
        let runtime = self
            .dinput
            .as_mut()
            .ok_or(HostError::DirectInput(DirectInputError32::NotConfigured))?;
        let frame = runtime
            .haptic_frames
            .get_mut(usize::from(slot))
            .ok_or_else(invalid)?;
        if frame.is_none_or(|frame| frame.epoch != epoch || frame.sequence != sequence) {
            return Err(HostError::DirectInput(DirectInputError32::InvalidToken));
        }
        *frame = None;
        Ok(())
    }
}

impl DirectInputRuntime32 {
    pub(in crate::process::windows::dinput) fn observe_controller_haptics(
        &mut self,
        slot: u8,
        now: u64,
        state: &ControllerState32,
    ) {
        let buttons = button_bits(state);
        for record in self.records.iter_mut().flatten() {
            if let ObjectKind32::Device(device) = &mut record.kind
                && device.kind == DeviceKind32::Controller(slot)
            {
                let exclusive = state.connected
                    && device.native.acquisition() == Acquisition32::Acquired
                    && device
                        .native
                        .cooperative()
                        .is_some_and(|level| level.exclusive);
                if let Some(haptics) = &mut device.haptics {
                    haptics.evaluate(now, &state.axes, &buttons, exclusive);
                }
            }
        }
        self.haptic_time[slot as usize] = self.haptic_time[slot as usize].max(now);
    }
}
fn button_bits(state: &ControllerState32) -> [u8; 16] {
    let mut bits = [0; 16];
    for (index, pressed) in state.buttons.iter().enumerate() {
        bits[index / 8] |= u8::from(*pressed) << (index % 8);
    }
    bits
}
fn invalid() -> HostError {
    HostError::DirectInput(DirectInputError32::InvalidRequest)
}
