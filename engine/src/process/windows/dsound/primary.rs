use super::{DSoundRuntime32, objects::ObjectKind32};
use crate::{
    audio::{
        AudioContext,
        dsound::{Device, HResult},
    },
    memory::GuestAddress,
    windows::synchronization::SynchronizationContext32,
};

impl DSoundRuntime32 {
    pub(in crate::process::windows::dsound) fn set_cooperative_level(
        &mut self,
        audio: &mut AudioContext,
        sync: Option<&mut SynchronizationContext32>,
        pointer: GuestAddress,
        window: u32,
        level: u32,
    ) -> Result<(), HResult> {
        let slot = self.slot(pointer)?;
        let mut candidate = self.device(pointer)?.native;
        candidate.set_cooperative_level(window, level)?;
        self.publish_focus_changes(audio, sync, &[(slot, candidate)])
    }

    pub(in crate::process::windows::dsound) fn publish_focus_changes(
        &mut self,
        audio: &mut AudioContext,
        sync: Option<&mut SynchronizationContext32>,
        changes: &[(usize, Device)],
    ) -> Result<(), HResult> {
        let candidate = |slot: usize| -> Device {
            changes
                .iter()
                .find(|(index, _)| *index == slot)
                .map(|(_, native)| *native)
                .unwrap_or_else(|| match &self.records[slot].as_ref().unwrap().kind {
                    ObjectKind32::Device(device) => device.native,
                    _ => unreachable!(),
                })
        };
        let mut primary = None;
        for (slot, object) in self
            .records
            .iter()
            .enumerate()
            .filter_map(|(slot, object)| object.as_ref().map(|object| (slot, object)))
        {
            if matches!(object.kind, ObjectKind32::Device(_)) {
                let device = candidate(slot);
                if device.cooperative_level == 4 && device.focused && device.window_alive {
                    if primary.is_some() {
                        return Err(HResult::ALLOCATED);
                    }
                    primary = Some(object.pointer);
                }
            }
        }
        let mut losses = Vec::new();
        losses
            .try_reserve_exact(self.records.len())
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        self.signals.clear();
        for (slot, object) in self
            .records
            .iter()
            .enumerate()
            .filter_map(|(slot, object)| object.as_ref().map(|object| (slot, object)))
        {
            let ObjectKind32::Buffer(buffer) = &object.kind else {
                continue;
            };
            let owner = self.slot(buffer.device)?;
            let device = candidate(owner);
            audio.can_set_voice_controls(buffer.native.voice, buffer.native.controls(&device))?;
            let lost = if buffer.native.primary {
                let old = self.device(buffer.device)?.native;
                old.cooperative_level == 4
                    && old.focused
                    && (!device.focused || !device.window_alive)
            } else {
                primary.is_some_and(|primary| primary != buffer.device)
            };
            let stop = lost || !buffer.native.primary && device.cooperative_level == 4;
            if stop
                && (lost && !buffer.native.lost || audio.voice_state(buffer.native.voice)?.playing)
            {
                audio.can_stop_stream(buffer.native.voice)?;
                if audio.voice_state(buffer.native.voice)?.playing {
                    self.signals.extend(
                        buffer
                            .notifications
                            .iter()
                            .filter(|item| item.offset == u32::MAX)
                            .map(|item| (item.binding, 1)),
                    );
                }
                losses.push((slot, lost));
            }
        }
        let signals = if self.signals.is_empty() {
            None
        } else {
            Some(
                sync.as_ref()
                    .ok_or(HResult::INVALID_CALL)?
                    .prepare_signal_occurrences(&self.signals)
                    .map_err(|_| HResult::INVALID_CALL)?,
            )
        };
        for &(slot, candidate) in changes {
            let ObjectKind32::Device(device) = &mut self.records[slot].as_mut().unwrap().kind
            else {
                unreachable!()
            };
            device.native = candidate;
        }
        for (slot, lost) in losses {
            let ObjectKind32::Buffer(buffer) = &mut self.records[slot].as_mut().unwrap().kind
            else {
                unreachable!()
            };
            audio
                .stop_stream(buffer.native.voice)
                .expect("primary ownership loss was preflighted");
            buffer.native.lost |= lost;
        }
        for object in self.records.iter().flatten() {
            if let ObjectKind32::Buffer(buffer) = &object.kind {
                buffer
                    .native
                    .apply_controls(audio, &self.device(buffer.device)?.native)
                    .expect("all device controls were preflighted");
            }
        }
        if let Some(signals) = signals {
            sync.unwrap().commit_signals(signals)
        }
        Ok(())
    }
}
