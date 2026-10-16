use super::{EngineInstance, HostError};
use crate::windows::synchronization::EventBinding32;

pub(in crate::process) struct PreparedMediaExit32 {
    epoch: u64,
    bindings: Vec<EventBinding32>,
}
impl PreparedMediaExit32 {
    pub(in crate::process) fn bindings(&self) -> &[EventBinding32] {
        &self.bindings
    }
}
impl EngineInstance {
    pub(in crate::process) fn prepare_media_process_exit(
        &mut self,
    ) -> Result<PreparedMediaExit32, HostError> {
        let mut bindings = Vec::new();
        if let Some(sound) = &self.dsound {
            sound.append_process_exit_bindings(&mut bindings)?;
        }
        if let Some(input) = &self.dinput {
            input.append_process_exit_bindings(&mut bindings)?;
        }
        if let Some(graphics) = self.d3d8.as_mut() {
            graphics.prepare_process_exit()?;
        }
        Ok(PreparedMediaExit32 {
            epoch: self.media_epoch,
            bindings,
        })
    }
    pub(in crate::process) fn commit_media_process_exit(&mut self, prepared: PreparedMediaExit32) {
        assert_eq!(prepared.epoch, self.media_epoch);
        if let Some(graphics) = self.d3d8.as_mut() {
            graphics.commit_process_exit();
        }
        if let Some(media) = self.media.as_mut() {
            media.audio.terminate_process();
        }
        self.dsound = None;
        self.dinput = None;
        self.d3d8_reply = None;
        self.windows_input_delivery_error = None;
        self.winmm.terminate_process();
        if let Some(user32) = self.user32.as_mut() {
            user32.terminate_raw_input();
        }
        self.ole32 = None;
        self.ole32_task_memory = Default::default();
    }
    pub(in crate::process) fn check_media_output_session(&self) -> Result<(), HostError> {
        if self.is_open() {
            Ok(())
        } else {
            Err(HostError::Closed)
        }
    }
}

#[cfg(test)]
mod tests;
