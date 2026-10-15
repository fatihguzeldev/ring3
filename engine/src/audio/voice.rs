use super::{AudioBufferId32, AudioContext, AudioError32, AudioVoiceId32, AudioVoiceState32};

#[derive(Default)]
pub(super) struct VoiceSlot {
    pub generation: u32,
    pub voice: Option<Voice>,
}

pub(super) struct Voice {
    pub buffer: AudioBufferId32,
    pub position: u32,
    pub produced_position: u32,
    pub playing: bool,
    pub producing: bool,
    pub looping: bool,
}

impl AudioContext {
    pub fn create_voice(
        &mut self,
        buffer: AudioBufferId32,
    ) -> Result<AudioVoiceId32, AudioError32> {
        self.ready()?;
        self.buffer(buffer)?;
        let (index, generation) = self
            .voices
            .iter()
            .enumerate()
            .find_map(|(index, slot)| {
                slot.voice
                    .is_none()
                    .then(|| slot.generation.checked_add(1).map(|next| (index, next)))
                    .flatten()
            })
            .ok_or(AudioError32::Capacity)?;
        self.voices[index].voice = Some(Voice {
            buffer,
            position: 0,
            produced_position: 0,
            playing: false,
            producing: false,
            looping: false,
        });
        self.voices[index].generation = generation;
        Ok(AudioVoiceId32 {
            epoch: self.epoch,
            slot: index as u32,
            generation,
        })
    }

    pub fn release_voice(&mut self, id: AudioVoiceId32) -> Result<(), AudioError32> {
        self.ready()?;
        self.voice(id)?;
        self.voices[id.slot as usize].voice = None;
        Ok(())
    }

    pub fn play(&mut self, id: AudioVoiceId32, looping: bool) -> Result<(), AudioError32> {
        self.ready()?;
        let voice = self.voice_mut(id)?;
        voice.playing = true;
        voice.producing = true;
        voice.looping = looping;
        Ok(())
    }

    /// pending PCM must be acknowledged first; stop latency is at most one submission.
    pub fn stop(&mut self, id: AudioVoiceId32) -> Result<(), AudioError32> {
        self.ready()?;
        let voice = self.voice_mut(id)?;
        voice.playing = false;
        voice.producing = false;
        Ok(())
    }

    pub fn seek(&mut self, id: AudioVoiceId32, frame: u32) -> Result<(), AudioError32> {
        self.ready()?;
        let voice = self.voice(id)?;
        if frame >= self.buffer(voice.buffer)?.frames() {
            return Err(AudioError32::InvalidRequest);
        }
        let voice = self.voice_mut(id)?;
        voice.position = frame;
        voice.produced_position = frame;
        Ok(())
    }

    pub fn voice_state(&self, id: AudioVoiceId32) -> Result<AudioVoiceState32, AudioError32> {
        let voice = self.voice(id)?;
        Ok(AudioVoiceState32 {
            buffer: voice.buffer,
            position_frames: voice.position,
            produced_position_frames: voice.produced_position,
            playing: voice.playing,
            producing: voice.producing,
            looping: voice.looping,
        })
    }

    fn voice(&self, id: AudioVoiceId32) -> Result<&Voice, AudioError32> {
        if id.epoch != self.epoch || id.generation == 0 {
            return Err(AudioError32::InvalidVoice);
        }
        self.voices
            .get(id.slot as usize)
            .filter(|slot| slot.generation == id.generation)
            .and_then(|slot| slot.voice.as_ref())
            .ok_or(AudioError32::InvalidVoice)
    }

    fn voice_mut(&mut self, id: AudioVoiceId32) -> Result<&mut Voice, AudioError32> {
        self.voice(id)?;
        Ok(self.voices[id.slot as usize].voice.as_mut().unwrap())
    }
}
