use super::super::{DSoundRuntime32, objects::ObjectKind32};
use crate::{
    audio::{
        AudioContext,
        dsound::{Buffer, Device, HResult},
    },
    memory::GuestAddress,
    windows::synchronization::{PreparedSignals32, SynchronizationContext32},
};

pub(super) struct SpatialPublication32 {
    pub device_pointer: GuestAddress,
    pub device: Device,
    pub deferred: bool,
    pub buffer: Option<(usize, Buffer)>,
}

impl DSoundRuntime32 {
    pub(super) fn prepare_spatial_publication(
        &mut self,
        audio: &AudioContext,
        sync: Option<&SynchronizationContext32>,
        changes: SpatialPublication32,
    ) -> Result<Option<PreparedSignals32>, HResult> {
        self.signals.clear();
        for (index, object) in self.records.iter().enumerate() {
            let Some(object) = object else { continue };
            let ObjectKind32::Buffer(buffer) = &object.kind else {
                continue;
            };
            if buffer.device != changes.device_pointer {
                continue;
            }
            let mut candidate = changes
                .buffer
                .filter(|(slot, _)| *slot == index)
                .map_or(buffer.native, |(_, candidate)| candidate);
            if changes.deferred {
                candidate.applied_spatial = candidate.spatial;
            }
            audio.can_set_voice_controls(candidate.voice, candidate.controls(&changes.device))?;
            if candidate.distance_stops_playback(&changes.device)
                && audio.voice_state(candidate.voice)?.playing
            {
                audio.can_stop_stream(candidate.voice)?;
                self.signals.extend(
                    buffer
                        .notifications
                        .iter()
                        .filter(|notification| notification.offset == u32::MAX)
                        .map(|notification| (notification.binding, 1)),
                );
            }
        }
        if self.signals.is_empty() {
            return Ok(None);
        }
        sync.ok_or(HResult::INVALID_CALL)?
            .prepare_signal_occurrences(&self.signals)
            .map(Some)
            .map_err(|_| HResult::INVALID_CALL)
    }

    pub(super) fn commit_spatial_stops(
        &self,
        audio: &mut AudioContext,
        sync: Option<&mut SynchronizationContext32>,
        pointer: GuestAddress,
        device: Device,
        signals: Option<PreparedSignals32>,
    ) {
        for object in self.records.iter().flatten() {
            if let ObjectKind32::Buffer(buffer) = &object.kind
                && buffer.device == pointer
                && buffer.native.distance_stops_playback(&device)
                && audio
                    .voice_state(buffer.native.voice)
                    .expect("sound voice was preflighted")
                    .playing
            {
                audio
                    .stop_stream(buffer.native.voice)
                    .expect("distance stop was preflighted");
            }
        }
        if let Some(signals) = signals {
            sync.unwrap().commit_signals(signals);
        }
    }
}
