use super::super::{DSoundError32, objects::ObjectKind32};
use crate::{
    audio::dsound::{CaptureRequest, HResult},
    process::{EngineInstance, HostError},
    windows::{synchronization::PreparedSignals32, window::WindowHandle32},
};

fn error(_: HResult) -> HostError {
    HostError::DSound(DSoundError32::InvalidRequest)
}
impl EngineInstance {
    pub fn dsound_capture_request(&self, index: u32) -> Result<Option<CaptureRequest>, HostError> {
        self.media_contexts()?;
        let runtime = self
            .dsound
            .as_ref()
            .ok_or(HostError::DSound(DSoundError32::NotConfigured))?;
        Ok(runtime
            .records
            .iter()
            .flatten()
            .filter_map(|object| {
                let ObjectKind32::CaptureBuffer(buffer) = &object.kind else {
                    return None;
                };
                let id = runtime.com.lookup(object.pointer).ok()?.id.get();
                buffer.native.request(self.media_epoch, id)
            })
            .nth(index as usize))
    }
    pub fn dsound_receive_capture(
        &mut self,
        request: CaptureRequest,
        bytes: &[u8],
    ) -> Result<(), HostError> {
        self.media_contexts()?;
        if request.epoch != self.media_epoch {
            return Err(HostError::DSound(DSoundError32::InvalidRequest));
        }
        let runtime = self
            .dsound
            .as_mut()
            .ok_or(HostError::DSound(DSoundError32::NotConfigured))?;
        if !runtime.config.capture_available {
            return Err(HostError::DSound(DSoundError32::InvalidRequest));
        }
        let index = runtime
            .records
            .iter()
            .position(|object| {
                object.as_ref().is_some_and(|object| {
                    runtime
                        .com
                        .lookup(object.pointer)
                        .is_ok_and(|identity| identity.id.get() == request.object)
                })
            })
            .ok_or(HostError::DSound(DSoundError32::InvalidRequest))?;
        let ObjectKind32::CaptureBuffer(buffer) = &runtime.records[index].as_ref().unwrap().kind
        else {
            return Err(HostError::DSound(DSoundError32::InvalidRequest));
        };
        let prepared = buffer
            .native
            .prepare_write(request, self.media_epoch, request.object, bytes)
            .map_err(error)?;
        let progress = buffer.native.progress(prepared);
        runtime.signals.clear();
        for notification in &buffer.notifications {
            let reached = if notification.offset == u32::MAX {
                progress.stopped
            } else if progress.wrapped {
                notification.offset > progress.first_byte
                    || notification.offset <= progress.last_byte
            } else {
                notification.offset > progress.first_byte
                    && notification.offset <= progress.last_byte
            };
            if reached {
                runtime.signals.push((notification.binding, 1));
            }
        }
        let signals = prepare_signals(
            self.synchronization
                .as_ref()
                .map(|runtime| runtime.context()),
            &runtime.signals,
        )?;
        let ObjectKind32::CaptureBuffer(buffer) =
            &mut runtime.records[index].as_mut().unwrap().kind
        else {
            unreachable!()
        };
        buffer.native.commit_write(prepared, bytes);
        if let Some(signals) = signals {
            self.synchronization
                .as_mut()
                .unwrap()
                .context_mut()
                .commit_signals(signals);
        }
        Ok(())
    }
    pub fn dsound_capture_availability(
        &mut self,
        epoch: u64,
        available: bool,
    ) -> Result<(), HostError> {
        self.media_contexts()?;
        if epoch != self.media_epoch {
            return Err(HostError::DSound(DSoundError32::InvalidRequest));
        }
        let runtime = self
            .dsound
            .as_mut()
            .ok_or(HostError::DSound(DSoundError32::NotConfigured))?;
        if runtime.config.capture_available == available {
            return Ok(());
        }
        runtime.signals.clear();
        if !available {
            for object in runtime.records.iter().flatten() {
                if let ObjectKind32::CaptureBuffer(buffer) = &object.kind {
                    buffer.native.can_invalidate().map_err(error)?;
                    if buffer.native.status() & 1 != 0 {
                        runtime.signals.extend(
                            buffer
                                .notifications
                                .iter()
                                .filter(|notification| notification.offset == u32::MAX)
                                .map(|notification| (notification.binding, 1)),
                        );
                    }
                }
            }
        }
        let signals = prepare_signals(
            self.synchronization
                .as_ref()
                .map(|runtime| runtime.context()),
            &runtime.signals,
        )?;
        if !available {
            for object in runtime.records.iter_mut().flatten() {
                if let ObjectKind32::CaptureBuffer(buffer) = &mut object.kind {
                    buffer
                        .native
                        .invalidate()
                        .expect("capture loss was preflighted");
                }
            }
        }
        runtime.config.capture_available = available;
        if let Some(signals) = signals {
            self.synchronization
                .as_mut()
                .unwrap()
                .context_mut()
                .commit_signals(signals);
        }
        Ok(())
    }
    pub fn dsound_observe_playback_loss(
        &mut self,
        epoch: u64,
        lost: bool,
    ) -> Result<(), HostError> {
        self.media_contexts()?;
        if epoch != self.media_epoch {
            return Err(HostError::DSound(DSoundError32::InvalidRequest));
        }
        let runtime = self
            .dsound
            .as_mut()
            .ok_or(HostError::DSound(DSoundError32::NotConfigured))?;
        if runtime.playback_lost == lost {
            return Ok(());
        }
        let audio = &mut self.media.as_mut().unwrap().audio;
        runtime.signals.clear();
        if lost {
            for object in runtime.records.iter().flatten() {
                if let ObjectKind32::Buffer(buffer) = &object.kind {
                    audio
                        .can_stop_stream(buffer.native.voice)
                        .map_err(|value| error(value.into()))?;
                    if audio
                        .voice_state(buffer.native.voice)
                        .map_err(|value| error(value.into()))?
                        .playing
                    {
                        runtime.signals.extend(
                            buffer
                                .notifications
                                .iter()
                                .filter(|notification| notification.offset == u32::MAX)
                                .map(|notification| (notification.binding, 1)),
                        );
                    }
                }
            }
        }
        let signals = prepare_signals(
            self.synchronization
                .as_ref()
                .map(|runtime| runtime.context()),
            &runtime.signals,
        )?;
        if lost {
            for object in runtime.records.iter_mut().flatten() {
                if let ObjectKind32::Buffer(buffer) = &mut object.kind {
                    audio
                        .stop_stream(buffer.native.voice)
                        .expect("sound loss was preflighted");
                    buffer.native.lost = true;
                }
            }
        }
        runtime.playback_lost = lost;
        if let Some(signals) = signals {
            self.synchronization
                .as_mut()
                .unwrap()
                .context_mut()
                .commit_signals(signals);
        }
        Ok(())
    }
    pub(in crate::process) fn refresh_dsound_focus(&mut self) -> Result<(), HostError> {
        let Some(runtime) = self.dsound.as_mut() else {
            return Ok(());
        };
        let Some(media) = self.media.as_mut() else {
            return Ok(());
        };
        let windows = self
            .user32
            .as_ref()
            .map(crate::process::windows::user32::User32Context32::windows);
        let physical = media.input.source().snapshot().focused;
        for object in runtime.records.iter().flatten() {
            if let ObjectKind32::Device(device) = &object.kind {
                let mut candidate = device.native;
                candidate.window_alive = windows.is_some_and(|windows| {
                    WindowHandle32::try_from(candidate.window)
                        .is_ok_and(|window| windows.get(window).is_ok())
                });
                candidate.focused = physical
                    && windows.is_some_and(|windows| {
                        windows
                            .active()
                            .is_some_and(|window| window.get() == candidate.window)
                    });
                if (candidate.focused, candidate.window_alive)
                    != (device.native.focused, device.native.window_alive)
                {
                    for buffer in runtime.records.iter().flatten() {
                        if let ObjectKind32::Buffer(buffer) = &buffer.kind
                            && buffer.device == object.pointer
                        {
                            media
                                .audio
                                .can_set_voice_controls(
                                    buffer.native.voice,
                                    buffer.native.controls(&candidate),
                                )
                                .map_err(|value| error(value.into()))?;
                        }
                    }
                }
            }
        }
        for index in 0..runtime.records.len() {
            let Some(object) = runtime.records[index].as_mut() else {
                continue;
            };
            let ObjectKind32::Device(device) = &mut object.kind else {
                continue;
            };
            let old = (device.native.focused, device.native.window_alive);
            device.native.window_alive = windows.is_some_and(|windows| {
                WindowHandle32::try_from(device.native.window)
                    .is_ok_and(|window| windows.get(window).is_ok())
            });
            device.native.focused = physical
                && windows.is_some_and(|windows| {
                    windows
                        .active()
                        .is_some_and(|window| window.get() == device.native.window)
                });
            let pointer = object.pointer;
            if old != (device.native.focused, device.native.window_alive) {
                runtime
                    .apply_device_controls(&mut media.audio, pointer)
                    .map_err(error)?;
            }
        }
        Ok(())
    }
}
fn prepare_signals(
    context: Option<&crate::windows::synchronization::SynchronizationContext32>,
    signals: &[(crate::windows::synchronization::EventBinding32, u64)],
) -> Result<Option<PreparedSignals32>, HostError> {
    if signals.is_empty() {
        return Ok(None);
    }
    context
        .ok_or(HostError::DSound(DSoundError32::InvalidRequest))?
        .prepare_signal_occurrences(signals)
        .map(Some)
        .map_err(HostError::Synchronization)
}
