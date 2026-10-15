use super::{AudioContext, AudioError32, AudioSubmission32, AudioVoiceId32, AudioVoiceProgress32};

#[derive(Clone, Copy, Default)]
pub(super) struct VoiceSpan {
    first: u64,
    step: u64,
    source_frames: u32,
    frames: u32,
    ended: bool,
    looping: bool,
    generation: u32,
    revision: u64,
}

pub(super) struct PendingSubmission {
    pub id: AudioSubmission32,
    consumed: u32,
}

impl AudioContext {
    /// submitted pcm is immutable; later stream mutations affect the next submission.
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
            let limit = u64::from(source_frames) << 32;
            let first = voice.produced_position;
            let mut source = first;
            let mut count = 0;
            let mut ended = false;
            for frame in 0..frames {
                let at = (source >> 32) as u32;
                let next = if at + 1 < source_frames {
                    at + 1
                } else if voice.looping {
                    0
                } else {
                    at
                };
                let fraction = (source as u32) as f32 / 4_294_967_296.0;
                for channel in 0..2 {
                    let a = buffer.sample(at, channel);
                    let value = a + (buffer.sample(next, channel) - a) * fraction;
                    self.output[frame as usize * 2 + channel] +=
                        value * voice.controls.gains[channel];
                }
                count += 1;
                source += voice.step;
                if source >= limit {
                    if voice.looping {
                        source %= limit;
                    } else {
                        source = 0;
                        ended = true;
                        break;
                    }
                }
            }
            voice.produced_position = source;
            voice.producing = !ended;
            self.spans[index] = VoiceSpan {
                first,
                step: voice.step,
                source_frames,
                frames: count,
                ended,
                looping: voice.looping,
                generation: slot.generation,
                revision: voice.revision,
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

    pub fn visit_acknowledgement(
        &self,
        submission: AudioSubmission32,
        consumed_frames: u32,
        mut visit: impl FnMut(AudioVoiceProgress32),
    ) -> Result<(), AudioError32> {
        let previous = self.validate_acknowledgement(submission, consumed_frames)?;
        for (index, span) in self
            .spans
            .iter()
            .enumerate()
            .filter(|(_, span)| span.frames != 0)
        {
            let slot = &self.voices[index];
            if slot.generation != span.generation
                || !slot
                    .voice
                    .as_ref()
                    .is_some_and(|voice| voice.revision == span.revision)
            {
                continue;
            }
            if let Some(mut progress) = progress(
                *span,
                previous,
                consumed_frames,
                AudioVoiceId32 {
                    epoch: self.epoch,
                    slot: index as u32,
                    generation: span.generation,
                },
            ) {
                progress.stopped &= !slot.voice.as_ref().unwrap().looping;
                visit(progress);
            }
        }
        Ok(())
    }

    /// cumulative progress applies only to the exact voice generation and playback revision.
    pub fn acknowledge(
        &mut self,
        submission: AudioSubmission32,
        consumed_frames: u32,
    ) -> Result<(), AudioError32> {
        let previous = self.validate_acknowledgement(submission, consumed_frames)?;
        for (index, span) in self
            .spans
            .iter()
            .enumerate()
            .filter(|(_, span)| span.frames != 0)
        {
            let slot = &mut self.voices[index];
            if slot.generation != span.generation {
                continue;
            }
            let Some(voice) = slot
                .voice
                .as_mut()
                .filter(|voice| voice.revision == span.revision)
            else {
                continue;
            };
            if let Some(progress) = progress(
                *span,
                previous,
                consumed_frames,
                AudioVoiceId32 {
                    epoch: self.epoch,
                    slot: index as u32,
                    generation: span.generation,
                },
            ) {
                voice.position = progress.last_phase;
                if progress.stopped && !voice.looping {
                    voice.playing = false;
                }
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

    fn validate_acknowledgement(
        &self,
        id: AudioSubmission32,
        consumed: u32,
    ) -> Result<u32, AudioError32> {
        let pending = self.pending_submission(id)?;
        if consumed <= pending.consumed || consumed > id.frames {
            return Err(AudioError32::InvalidRequest);
        }
        Ok(pending.consumed)
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

fn progress(
    span: VoiceSpan,
    previous: u32,
    consumed: u32,
    voice: AudioVoiceId32,
) -> Option<AudioVoiceProgress32> {
    let first = previous.min(span.frames);
    let last = consumed.min(span.frames);
    if first == last {
        return None;
    }
    let limit = u128::from(span.source_frames) << 32;
    let start = u128::from(span.first) + u128::from(first) * u128::from(span.step);
    let end = u128::from(span.first) + u128::from(last) * u128::from(span.step);
    let stopped = span.ended && last == span.frames;
    Some(AudioVoiceProgress32 {
        voice,
        first_phase: (start % limit) as u64,
        last_phase: if stopped { 0 } else { (end % limit) as u64 },
        wraps: if span.looping {
            (end / limit - start / limit) as u64
        } else {
            u64::from(stopped)
        },
        stopped,
    })
}
