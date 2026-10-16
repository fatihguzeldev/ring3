mod buffer;
pub mod dsound;
mod mixing;
mod submission;
mod types;
mod voice;

pub use types::{
    AudioBufferId32, AudioConfig32, AudioError32, AudioState32, AudioSubmission32, AudioVoiceId32,
    AudioVoiceState32, PcmFormat32,
};
pub use types::{AudioVoiceControls32, AudioVoiceProgress32, SampleEncoding32};

use buffer::BufferSlot;
use submission::{PendingSubmission, VoiceSpan};
use voice::VoiceSlot;

pub struct AudioContext {
    epoch: u64,
    config: AudioConfig32,
    buffers: Vec<BufferSlot>,
    voices: Vec<VoiceSlot>,
    buffer_bytes: u32,
    output: Vec<f32>,
    wide_output: Vec<f64>,
    spans: Vec<VoiceSpan>,
    pending: Option<PendingSubmission>,
    sequence: u64,
    submitted_frames: u64,
    consumed_frames: u64,
}

impl AudioContext {
    pub fn new(epoch: u64, config: AudioConfig32) -> Result<Self, AudioError32> {
        if epoch == 0
            || config.sample_rate == 0
            || !(1..=AudioConfig32::MAX_RESOURCES).contains(&config.max_buffers)
            || !(1..=AudioConfig32::MAX_RESOURCES).contains(&config.max_voices)
            || !(1..=AudioConfig32::MAX_BUFFER_BYTES).contains(&config.max_buffer_bytes)
            || !(1..=AudioConfig32::MAX_RENDER_FRAMES).contains(&config.render_frames)
        {
            return Err(AudioError32::InvalidConfig);
        }
        Ok(Self {
            epoch,
            config,
            buffers: allocated(config.max_buffers as usize, BufferSlot::default)?,
            voices: allocated(config.max_voices as usize, VoiceSlot::default)?,
            buffer_bytes: 0,
            output: allocated(config.render_frames as usize * 2, || 0.0)?,
            wide_output: Vec::new(),
            spans: allocated(config.max_voices as usize, VoiceSpan::default)?,
            pending: None,
            sequence: 0,
            submitted_frames: 0,
            consumed_frames: 0,
        })
    }

    pub fn config(&self) -> AudioConfig32 {
        self.config
    }

    pub fn state(&self) -> AudioState32 {
        AudioState32 {
            epoch: self.epoch,
            submitted_frames: self.submitted_frames,
            consumed_frames: self.consumed_frames,
            pending: self.pending.as_ref().map(|pending| pending.id),
        }
    }

    /// cancels pending output and invalidates every prior resource and acknowledgement.
    pub fn reset(&mut self, new_epoch: u64) -> Result<(), AudioError32> {
        if new_epoch <= self.epoch {
            return Err(AudioError32::InvalidRequest);
        }
        for slot in &mut self.buffers {
            slot.buffer = None;
        }
        for slot in &mut self.voices {
            slot.voice = None;
        }
        self.output.fill(0.0);
        self.spans.fill(VoiceSpan::default());
        self.pending = None;
        self.epoch = new_epoch;
        self.buffer_bytes = 0;
        self.sequence = 0;
        self.submitted_frames = 0;
        self.consumed_frames = 0;
        Ok(())
    }

    /// process ownership ends while the exact already submitted pcm remains drainable.
    pub(crate) fn terminate_process(&mut self) {
        for slot in &mut self.voices {
            slot.voice = None;
        }
        for slot in &mut self.buffers {
            slot.buffer = None;
        }
        self.spans.fill(VoiceSpan::default());
        self.buffer_bytes = 0;
    }

    fn ready(&self) -> Result<(), AudioError32> {
        if self.pending.is_some() {
            return Err(AudioError32::Busy);
        }
        Ok(())
    }
}

fn allocated<T>(count: usize, value: impl Fn() -> T) -> Result<Vec<T>, AudioError32> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| AudioError32::Allocation)?;
    values.resize_with(count, value);
    Ok(values)
}

#[cfg(test)]
mod tests;
