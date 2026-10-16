use super::super::{DirectInputError32, objects::ObjectKind32};
use crate::{
    input::{InputError, source::ControllerState32},
    process::{EngineInstance, HostError},
};
impl EngineInstance {
    pub(in crate::process) fn sync_dinput_input(&mut self) -> Result<(), HostError> {
        let next_policy = match (self.dinput.as_ref(), self.media.as_ref()) {
            (Some(runtime), Some(media)) => runtime.policy_after_sync(
                media.input.source(),
                self.user32
                    .as_ref()
                    .and_then(|user| user.windows().active())
                    .map_or(0, |window| window.get()),
            ),
            _ => return Ok(()),
        };
        let clock_origin = self.cpu_clock.monotonic_bounds()?.0;
        let previous_policy = self.dinput.as_ref().unwrap().input_policy();
        let policy_receipt = if next_policy != previous_policy {
            self.prepare_dinput_policy()?
        } else {
            None
        };
        let Some(runtime) = self.dinput.as_mut() else {
            return Ok(());
        };
        let Some(media) = self.media.as_ref() else {
            return Ok(());
        };
        let source = media.input.source();
        let active = self
            .user32
            .as_ref()
            .and_then(|user| user.windows().active())
            .map_or(0, |window| window.get());
        runtime.signals.clear();
        for record in runtime.records.iter().flatten() {
            if let ObjectKind32::Device(device) = &record.kind
                && let Some(binding) = device
                    .notification
                    .filter(|_| device.native.would_notify(source, active))
            {
                runtime.signals.push(binding);
            }
        }
        let signals = if runtime.signals.is_empty() {
            None
        } else {
            Some(
                self.synchronization
                    .as_ref()
                    .ok_or(HostError::DirectInput(DirectInputError32::NotConfigured))?
                    .context()
                    .prepare_signals(&runtime.signals)
                    .map_err(|error| {
                        HostError::DirectInput(DirectInputError32::Synchronization(error))
                    })?,
            )
        };
        for record in runtime.records.iter_mut().flatten() {
            if let ObjectKind32::Device(device) = &mut record.kind {
                device.native.synchronize(source, active, clock_origin);
                if device.native.acquisition()
                    != crate::input::dinput::device::Acquisition32::Acquired
                    && let Some(haptics) = &mut device.haptics
                {
                    haptics.lose_acquisition();
                }
            }
        }
        if let Some(signals) = signals {
            self.synchronization
                .as_mut()
                .unwrap()
                .context_mut()
                .commit_signals(signals);
        }
        if next_policy != previous_policy {
            self.commit_dinput_policy(policy_receipt, next_policy);
        }
        Ok(())
    }
    pub fn submit_controller_input(
        &mut self,
        epoch: u64,
        slot: u8,
        timestamp_ns: u64,
        state: ControllerState32,
    ) -> Result<(), HostError> {
        self.sync_windows_input()?;
        let (_, observed) = self.cpu_clock.monotonic_bounds()?;
        if timestamp_ns > observed {
            return Err(HostError::DirectInput(DirectInputError32::Input(
                InputError::TimeOrder,
            )));
        }
        let descriptor = self
            .dinput
            .as_ref()
            .and_then(|runtime| runtime.controllers.get(usize::from(slot)))
            .and_then(Option::as_ref)
            .ok_or(HostError::DirectInput(DirectInputError32::InvalidRequest))?;
        if state.connected
            && (state
                .axes
                .iter()
                .enumerate()
                .any(|(index, value)| index % 8 >= usize::from(descriptor.axes) && *value != 0)
                || state.pov[usize::from(descriptor.povs)..]
                    .iter()
                    .any(|value| *value != u32::MAX)
                || state.buttons[usize::from(descriptor.buttons)..]
                    .iter()
                    .any(|pressed| *pressed))
        {
            return Err(HostError::DirectInput(DirectInputError32::InvalidRequest));
        }
        self.media_contexts_mut()?
            .input
            .source_mut()
            .publish_controller(epoch, slot, timestamp_ns, state)
            .map_err(|error| HostError::DirectInput(DirectInputError32::Input(error)))?;
        let _ = self.sync_windows_input();
        Ok(())
    }
    pub fn submit_windows_pointer(
        &mut self,
        epoch: u64,
        timestamp_ns: u64,
        x: i32,
        y: i32,
    ) -> Result<(), HostError> {
        self.sync_windows_input()?;
        let (_, observed) = self.cpu_clock.monotonic_bounds()?;
        if timestamp_ns > observed {
            return Err(HostError::DirectInput(DirectInputError32::Input(
                InputError::TimeOrder,
            )));
        }
        if epoch != self.media_contexts()?.input.session_epoch() {
            return Err(HostError::DirectInput(DirectInputError32::Input(
                InputError::StaleSession,
            )));
        }
        self.media_contexts_mut()?
            .input
            .source_mut()
            .publish_cursor(timestamp_ns, x, y)
            .map_err(|error| HostError::DirectInput(DirectInputError32::Input(error)))?;
        let _ = self.sync_windows_input();
        Ok(())
    }
    pub(in crate::process) fn reset_dinput_input(&mut self) {
        if let Some(runtime) = self.dinput.as_mut() {
            runtime.epoch_changed();
        }
    }
}
