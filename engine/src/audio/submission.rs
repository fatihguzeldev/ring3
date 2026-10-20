use super::{
    AudioConfig32, AudioContext, AudioError32, AudioSubmission32, AudioVoiceId32,
    AudioVoiceProgress32,
};
use super::{SampleEncoding32, buffer::BufferSlot, mixing::MixSample, voice::VoiceSlot};

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
    pcm: Vec<f32>,
    spans: Vec<VoiceSpan>,
}

pub(super) struct SubmissionQueue {
    slots: Vec<PendingSubmission>,
    head: usize,
    length: usize,
}

impl SubmissionQueue {
    const CAPACITY: usize = 8;

    pub fn new(config: AudioConfig32) -> Result<Self, AudioError32> {
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(Self::CAPACITY)
            .map_err(|_| AudioError32::Allocation)?;
        for _ in 0..Self::CAPACITY {
            slots.push(PendingSubmission {
                id: AudioSubmission32 {
                    epoch: 0,
                    sequence: 0,
                    first_frame: 0,
                    frames: 0,
                },
                consumed: 0,
                pcm: super::allocated(config.render_frames as usize * 2, || 0.0)?,
                spans: super::allocated(config.max_voices as usize, VoiceSpan::default)?,
            });
        }
        Ok(Self {
            slots,
            head: 0,
            length: 0,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    pub fn front(&self) -> Option<&PendingSubmission> {
        (!self.is_empty()).then(|| &self.slots[self.head])
    }

    fn front_mut(&mut self) -> &mut PendingSubmission {
        &mut self.slots[self.head]
    }

    fn find(&self, id: AudioSubmission32) -> Option<&PendingSubmission> {
        (0..self.length)
            .map(|index| &self.slots[(self.head + index) % Self::CAPACITY])
            .find(|packet| packet.id == id)
    }

    fn full(&self) -> bool {
        self.length == Self::CAPACITY
    }

    fn push(&mut self, id: AudioSubmission32, pcm: &[f32], spans: &[VoiceSpan]) {
        let slot = &mut self.slots[(self.head + self.length) % Self::CAPACITY];
        slot.id = id;
        slot.consumed = 0;
        slot.pcm[..pcm.len()].copy_from_slice(pcm);
        slot.spans.copy_from_slice(spans);
        self.length += 1;
    }

    fn pop(&mut self) {
        self.length -= 1;
        self.head = if self.is_empty() {
            0
        } else {
            (self.head + 1) % Self::CAPACITY
        };
    }

    pub fn clear(&mut self) {
        self.head = 0;
        self.length = 0;
    }
}

impl AudioContext {
    /// submitted pcm is immutable; later stream mutations affect the next submission.
    pub fn submit(&mut self, frames: u32) -> Result<AudioSubmission32, AudioError32> {
        self.ready()?;
        self.enqueue(frames)
    }

    /// queued pcm and its voice witnesses remain immutable until consumed in order.
    pub fn enqueue(&mut self, frames: u32) -> Result<AudioSubmission32, AudioError32> {
        if self.pending.full() {
            return Err(AudioError32::Busy);
        }
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
        let length = frames as usize * 2;
        let wide = self.voices.iter().any(|slot| {
            slot.voice.as_ref().is_some_and(|voice| {
                voice.producing
                    && (voice.effects.is_some()
                        || self.buffers[voice.buffer.slot as usize]
                            .buffer
                            .as_ref()
                            .unwrap()
                            .encoding
                            == SampleEncoding32::Float)
            })
        });
        if wide {
            if self.wide_output.len() < length {
                self.wide_output
                    .try_reserve_exact(length - self.wide_output.len())
                    .map_err(|_| AudioError32::Allocation)?;
                self.wide_output.resize(length, 0.0);
            }
            render(
                &self.buffers,
                &mut self.voices,
                &mut self.spans,
                &mut self.wide_output[..length],
                frames,
            );
            for (output, sample) in self.output[..length]
                .iter_mut()
                .zip(&self.wide_output[..length])
            {
                *output = sample.clipped();
            }
        } else {
            render(
                &self.buffers,
                &mut self.voices,
                &mut self.spans,
                &mut self.output[..length],
                frames,
            );
            for sample in &mut self.output[..length] {
                *sample = sample.clipped();
            }
        }
        self.sequence = sequence;
        if let Some(reference) = &mut self.capture_reference {
            reference.append(id.first_frame, &self.output[..length]);
        }
        self.submitted_frames = submitted;
        self.pending.push(id, &self.output[..length], &self.spans);
        Ok(id)
    }

    pub fn submission_pcm(&self, submission: AudioSubmission32) -> Result<&[f32], AudioError32> {
        let pending = self
            .pending
            .find(submission)
            .ok_or(AudioError32::InvalidSubmission)?;
        Ok(&pending.pcm[..submission.frames as usize * 2])
    }

    pub fn visit_acknowledgement(
        &self,
        submission: AudioSubmission32,
        consumed_frames: u32,
        mut visit: impl FnMut(AudioVoiceProgress32),
    ) -> Result<(), AudioError32> {
        let previous = self.validate_acknowledgement(submission, consumed_frames)?;
        for (index, span) in self
            .pending
            .front()
            .unwrap()
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

    pub fn can_acknowledge(
        &self,
        submission: AudioSubmission32,
        consumed_frames: u32,
    ) -> Result<(), AudioError32> {
        self.validate_acknowledgement(submission, consumed_frames)?;
        Ok(())
    }

    /// queries one live voice without scanning unrelated spans or retaining a progress cache.
    pub fn acknowledgement_progress(
        &self,
        submission: AudioSubmission32,
        consumed_frames: u32,
        id: AudioVoiceId32,
    ) -> Result<Option<AudioVoiceProgress32>, AudioError32> {
        let previous = self.validate_acknowledgement(submission, consumed_frames)?;
        let voice = self.voice(id)?;
        let span = self.pending.front().unwrap().spans[id.slot as usize];
        if span.frames == 0 || span.generation != id.generation || span.revision != voice.revision {
            return Ok(None);
        }
        Ok(
            progress(span, previous, consumed_frames, id).map(|mut progress| {
                progress.stopped &= !voice.looping;
                progress
            }),
        )
    }

    /// cumulative progress applies only to the exact voice generation and playback revision.
    pub fn acknowledge(
        &mut self,
        submission: AudioSubmission32,
        consumed_frames: u32,
    ) -> Result<(), AudioError32> {
        let previous = self.validate_acknowledgement(submission, consumed_frames)?;
        for (index, span) in self
            .pending
            .front()
            .unwrap()
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
            self.pending.pop();
        } else {
            self.pending.front_mut().consumed = consumed_frames;
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
            .front()
            .filter(|pending| pending.id == id)
            .ok_or(AudioError32::InvalidSubmission)
    }
}

fn render<S: MixSample>(
    buffers: &[BufferSlot],
    voices: &mut [VoiceSlot],
    spans: &mut [VoiceSpan],
    output: &mut [S],
    frames: u32,
) {
    output.fill(S::SILENCE);
    spans.fill(VoiceSpan::default());
    for (index, slot) in voices.iter_mut().enumerate() {
        let Some(voice) = slot.voice.as_mut().filter(|voice| voice.producing) else {
            continue;
        };
        let buffer = buffers[voice.buffer.slot as usize].buffer.as_ref().unwrap();
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
            if let Some(effects) = &mut voice.effects {
                let fraction = f64::from(source as u32) / 4_294_967_296.0;
                let input = core::array::from_fn(|channel| {
                    let first = f64::from(buffer.sample(at, channel));
                    let next = f64::from(buffer.sample(next, channel));
                    (first + (next - first) * fraction) as f32
                });
                let processed = effects.process(input);
                for channel in 0..2 {
                    output[frame as usize * 2 + channel].accumulate(
                        processed[channel],
                        processed[channel],
                        0,
                        voice.controls.gains[channel],
                    );
                }
            } else {
                for channel in 0..2 {
                    output[frame as usize * 2 + channel].accumulate(
                        buffer.sample(at, channel),
                        buffer.sample(next, channel),
                        source as u32,
                        voice.controls.gains[channel],
                    );
                }
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
        spans[index] = VoiceSpan {
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
