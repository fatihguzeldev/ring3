use super::{AudioContext, AudioError32, AudioSubmission32};

#[derive(Clone, Copy, Default)]
pub(super) struct VoiceSpan {
    first: u32,
    frames: u32,
    ended: bool,
}

pub(super) struct PendingSubmission {
    pub id: AudioSubmission32,
    consumed: u32,
}

impl AudioContext {
    /// produces immutable stereo PCM without advancing any consumed cursor.
    pub fn submit(&mut self, frames: u32) -> Result<AudioSubmission32, AudioError32> {
        self.ready()?;
        if frames == 0 || frames > self.config.render_frames {
            return Err(AudioError32::InvalidRequest);
        }
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or(AudioError32::Exhausted)?;
        let submitted = self
            .submitted_frames
            .checked_add(u64::from(frames))
            .ok_or(AudioError32::Exhausted)?;
        let id = AudioSubmission32 {
            epoch: self.epoch,
            sequence,
            first_frame: self.submitted_frames,
            frames,
        };
        self.output[..frames as usize * 2].fill(0.0);
        self.spans.fill(VoiceSpan::default());
        for (index, slot) in self.voices.iter_mut().enumerate() {
            let Some(voice) = slot.voice.as_mut().filter(|voice| voice.producing) else {
                continue;
            };
            let buffer = self.buffers[voice.buffer.slot as usize]
                .buffer
                .as_ref()
                .unwrap();
            let source_frames = buffer.frames();
            let count = if voice.looping {
                frames
            } else {
                frames.min(source_frames - voice.produced_position)
            };
            let first = voice.produced_position;
            let mut source = first;
            for frame in 0..count {
                for channel in 0..2 {
                    self.output[frame as usize * 2 + channel] += buffer.sample(source, channel);
                }
                source += 1;
                if source == source_frames {
                    source = 0;
                }
            }
            let ended = !voice.looping && count == source_frames - first;
            voice.produced_position = source;
            voice.producing = !ended;
            self.spans[index] = VoiceSpan {
                first,
                frames: count,
                ended,
            };
        }
        for sample in &mut self.output[..frames as usize * 2] {
            *sample = sample.clamp(-1.0, 1.0);
        }
        self.sequence = sequence;
        self.submitted_frames = submitted;
        self.pending = Some(PendingSubmission { id, consumed: 0 });
        Ok(id)
    }

    pub fn submission_pcm(&self, submission: AudioSubmission32) -> Result<&[f32], AudioError32> {
        self.pending_submission(submission)?;
        Ok(&self.output[..submission.frames as usize * 2])
    }

    /// cumulative progress within the exact pending submission; duplicate progress is rejected.
    pub fn acknowledge(
        &mut self,
        submission: AudioSubmission32,
        consumed_frames: u32,
    ) -> Result<(), AudioError32> {
        let pending = self.pending_submission(submission)?;
        if consumed_frames <= pending.consumed || consumed_frames > submission.frames {
            return Err(AudioError32::InvalidRequest);
        }
        for (index, span) in self
            .spans
            .iter()
            .enumerate()
            .filter(|(_, span)| span.frames != 0)
        {
            let voice = self.voices[index].voice.as_mut().unwrap();
            let frames = self.buffers[voice.buffer.slot as usize]
                .buffer
                .as_ref()
                .unwrap()
                .frames();
            let consumed = consumed_frames.min(span.frames);
            voice.position =
                ((u64::from(span.first) + u64::from(consumed)) % u64::from(frames)) as u32;
            if span.ended && consumed == span.frames {
                voice.playing = false;
            }
        }
        self.consumed_frames = submission.first_frame + u64::from(consumed_frames);
        if consumed_frames == submission.frames {
            self.pending = None;
        } else {
            self.pending.as_mut().unwrap().consumed = consumed_frames;
        }
        Ok(())
    }

    fn pending_submission(
        &self,
        id: AudioSubmission32,
    ) -> Result<&PendingSubmission, AudioError32> {
        self.pending
            .as_ref()
            .filter(|pending| pending.id == id)
            .ok_or(AudioError32::InvalidSubmission)
    }
}
