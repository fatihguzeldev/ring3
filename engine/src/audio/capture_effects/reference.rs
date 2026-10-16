use crate::audio::{AudioContext, AudioError32};

pub struct PreparedCaptureReference {
    pub(super) samples: Vec<[f32; 2]>,
    pub(super) bytes: u32,
    next: u64,
    first: u64,
}
impl PreparedCaptureReference {
    pub(in crate::audio) fn append(&mut self, first: u64, pcm: &[f32]) {
        if self.next != first {
            self.first = first;
            self.next = first
        }
        for frame in pcm.chunks_exact(2) {
            let index = (self.next % self.samples.len() as u64) as usize;
            self.samples[index] = [frame[0], frame[1]];
            self.next += 1;
        }
        self.first = self
            .first
            .max(self.next.saturating_sub(self.samples.len() as u64));
    }
    fn at(&self, frame: u64) -> Option<[f32; 2]> {
        (self.first <= frame && frame < self.next)
            .then(|| self.samples[(frame % self.samples.len() as u64) as usize])
    }
}
impl AudioContext {
    pub fn prepare_capture_reference(
        &self,
    ) -> Result<Option<PreparedCaptureReference>, AudioError32> {
        if self.capture_reference.is_some() {
            return Ok(None);
        }
        let frames = self
            .config
            .sample_rate
            .div_ceil(2)
            .max(self.config.render_frames.saturating_mul(2));
        let bytes = frames.checked_mul(8).ok_or(AudioError32::Capacity)?;
        if self
            .buffer_bytes
            .checked_add(self.effect_bytes)
            .and_then(|retained| retained.checked_add(bytes))
            .is_none_or(|total| total > self.config.max_buffer_bytes)
        {
            return Err(AudioError32::Capacity);
        }
        let mut samples = Vec::new();
        samples
            .try_reserve_exact(frames as usize)
            .map_err(|_| AudioError32::Allocation)?;
        samples.resize(frames as usize, [0.; 2]);
        Ok(Some(PreparedCaptureReference {
            samples,
            bytes,
            next: self.submitted_frames,
            first: self.submitted_frames,
        }))
    }
    pub fn commit_capture_reference(&mut self, prepared: Option<PreparedCaptureReference>) {
        if let Some(prepared) = prepared {
            assert!(self.capture_reference.is_none());
            self.effect_bytes += prepared.bytes;
            self.capture_reference = Some(prepared)
        }
    }
    pub fn release_capture_reference(&mut self) {
        if let Some(reference) = self.capture_reference.take() {
            self.effect_bytes -= reference.bytes
        }
    }
    pub fn capture_reference_sample(&self, frame: u64) -> Option<[f32; 2]> {
        self.capture_reference
            .as_ref()
            .and_then(|reference| reference.at(frame))
    }
}
