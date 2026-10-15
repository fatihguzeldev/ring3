use super::{
    DSoundRuntime32,
    memory::words,
    objects::{Notification32, ObjectKind32},
};
use crate::{
    audio::{AudioContext, AudioError32, AudioSubmission32, dsound::HResult},
    memory::{AddressSpace, GuestAddress},
    windows::synchronization::{EventBinding32, SynchronizationContext32},
};

impl DSoundRuntime32 {
    pub(in crate::process::windows::dsound) fn notifications(
        &self,
        pointer: GuestAddress,
    ) -> Result<&[Notification32], HResult> {
        match &self.object(pointer)?.kind {
            ObjectKind32::Buffer(buffer) => Ok(&buffer.notifications),
            ObjectKind32::CaptureBuffer(buffer) => Ok(&buffer.notifications),
            _ => Err(HResult::INVALID_PARAM),
        }
    }
    pub(in crate::process::windows::dsound) fn set_notifications(
        &mut self,
        memory: &AddressSpace,
        audio: &AudioContext,
        sync: &mut SynchronizationContext32,
        pointer: GuestAddress,
        count: u32,
        input: u32,
    ) -> Result<u32, HResult> {
        let slot = self.slot(pointer)?;
        let old = self.notifications(pointer)?;
        let total = self
            .records
            .iter()
            .flatten()
            .map(|object| match &object.kind {
                ObjectKind32::Buffer(buffer) => buffer.notifications.len(),
                ObjectKind32::CaptureBuffer(buffer) => buffer.notifications.len(),
                _ => 0,
            })
            .sum::<usize>();
        if count as usize > self.config.max_notifications as usize - (total - old.len()) {
            return Err(HResult::OUT_OF_MEMORY);
        }
        let (length, playing) = match &self.records[slot].as_ref().unwrap().kind {
            ObjectKind32::Buffer(buffer) => (
                buffer.native.length(audio)?,
                audio.voice_state(buffer.native.voice)?.playing,
            ),
            ObjectKind32::CaptureBuffer(buffer) => (
                buffer.native.bytes().len() as u32,
                buffer.native.status() & 1 != 0,
            ),
            _ => return Err(HResult::INVALID_PARAM),
        };
        if playing {
            return Err(HResult::INVALID_CALL);
        }
        let mut offsets = Vec::new();
        let mut handles = Vec::new();
        let mut release = Vec::new();
        let mut notifications = Vec::new();
        for values in [&mut offsets, &mut handles] {
            values
                .try_reserve_exact(count as usize)
                .map_err(|_| HResult::OUT_OF_MEMORY)?;
        }
        release
            .try_reserve_exact(old.len())
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        release.extend(old.iter().map(|notification| notification.binding));
        notifications
            .try_reserve_exact(count as usize)
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        if u64::from(input) + u64::from(count) * 8 > 1 << 32 {
            return Err(HResult::INVALID_PARAM);
        }
        for index in 0..count {
            let pair = words::<2>(
                memory,
                input.checked_add(index * 8).ok_or(HResult::INVALID_PARAM)?,
            )?;
            if pair[0] != u32::MAX && pair[0] >= length {
                return Err(HResult::INVALID_PARAM);
            }
            offsets.push(pair[0]);
            handles.push(pair[1]);
        }
        let prepared = sync
            .prepare_event_bindings(&handles, &release)
            .map_err(|_| HResult::INVALID_PARAM)?;
        for (offset, binding) in offsets.into_iter().zip(prepared.bindings().iter().copied()) {
            notifications.push(Notification32 { offset, binding });
        }
        sync.commit_event_bindings(prepared);
        match &mut self.records[slot].as_mut().unwrap().kind {
            ObjectKind32::Buffer(buffer) => buffer.notifications = notifications,
            ObjectKind32::CaptureBuffer(buffer) => buffer.notifications = notifications,
            _ => unreachable!(),
        }
        Ok(0)
    }
    pub(in crate::process::windows::dsound) fn stop_signals(
        &mut self,
        pointer: GuestAddress,
    ) -> Result<&[(EventBinding32, u64)], HResult> {
        self.signals.clear();
        let slot = self.slot(pointer)?;
        let notifications = match &self.records[slot].as_ref().unwrap().kind {
            ObjectKind32::Buffer(buffer) => &buffer.notifications,
            ObjectKind32::CaptureBuffer(buffer) => &buffer.notifications,
            _ => return Err(HResult::INVALID_PARAM),
        };
        self.signals.extend(
            notifications
                .iter()
                .filter(|notification| notification.offset == u32::MAX)
                .map(|notification| (notification.binding, 1)),
        );
        Ok(&self.signals)
    }
    pub(in crate::process) fn prepare_audio_signals(
        &mut self,
        audio: &AudioContext,
        submission: AudioSubmission32,
        consumed: u32,
    ) -> Result<&[(EventBinding32, u64)], AudioError32> {
        self.signals.clear();
        audio.can_acknowledge(submission, consumed)?;
        for object in self.records.iter().flatten() {
            let ObjectKind32::Buffer(buffer) = &object.kind else {
                continue;
            };
            let Some(progress) =
                audio.acknowledgement_progress(submission, consumed, buffer.native.voice)?
            else {
                continue;
            };
            let frame_bytes = u64::from(buffer.native.format.pcm.frame_bytes());
            for notification in &buffer.notifications {
                let occurrences = if notification.offset == u32::MAX {
                    u64::from(progress.stopped)
                } else {
                    let phase = (u64::from(notification.offset) << 32).div_ceil(frame_bytes);
                    progress.wraps + u64::from(phase <= progress.last_phase)
                        - u64::from(phase <= progress.first_phase)
                };
                if occurrences != 0 {
                    self.signals.push((notification.binding, occurrences));
                }
            }
        }
        Ok(&self.signals)
    }
}
