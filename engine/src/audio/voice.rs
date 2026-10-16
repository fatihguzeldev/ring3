use super::{
    AudioBufferId32, AudioContext, AudioError32, AudioVoiceControls32, AudioVoiceId32,
    AudioVoiceState32,
};

#[derive(Default)]
pub(super) struct VoiceSlot {
    pub generation: u32,
    pub voice: Option<Voice>,
}

pub(super) struct Voice {
    pub buffer: AudioBufferId32,
    pub position: u64,
    pub produced_position: u64,
    pub playing: bool,
    pub producing: bool,
    pub looping: bool,
    pub revision: u64,
    pub controls: AudioVoiceControls32,
    pub step: u64,
    pub effects: Option<super::effects::EffectChain>,
}

impl AudioContext {
    pub fn create_voice(
        &mut self,
        buffer: AudioBufferId32,
    ) -> Result<AudioVoiceId32, AudioError32> {
        self.ready()?;
        self.create_stream_voice(buffer)
    }

    pub fn create_stream_voice(
        &mut self,
        buffer: AudioBufferId32,
    ) -> Result<AudioVoiceId32, AudioError32> {
        let frequency = self.buffer(buffer)?.format.sample_rate;
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
            effects: None,
            position: 0,
            produced_position: 0,
            playing: false,
            producing: false,
            looping: false,
            revision: 1,
            controls: AudioVoiceControls32 {
                frequency,
                gains: [1.0; 2],
            },
            step: (u64::from(frequency) << 32) / u64::from(self.config.sample_rate),
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
        self.release_stream_voice(id)
    }

    pub fn release_stream_voice(&mut self, id: AudioVoiceId32) -> Result<(), AudioError32> {
        let bytes = self
            .voice(id)?
            .effects
            .as_ref()
            .map_or(0, |chain| chain.bytes);
        self.effect_bytes -= bytes;
        self.voices[id.slot as usize].voice = None;
        Ok(())
    }

    pub fn play(&mut self, id: AudioVoiceId32, looping: bool) -> Result<(), AudioError32> {
        self.ready()?;
        self.play_stream(id, looping)
    }

    pub fn play_stream(&mut self, id: AudioVoiceId32, looping: bool) -> Result<(), AudioError32> {
        let voice = self.voice_mut(id)?;
        if !voice.playing {
            let revision = voice
                .revision
                .checked_add(1)
                .ok_or(AudioError32::Exhausted)?;
            voice.revision = revision;
            voice.produced_position = voice.position;
            voice.producing = true;
        } else if looping && !voice.producing {
            voice.producing = true;
        }
        voice.playing = true;
        voice.looping = looping;
        Ok(())
    }

    pub fn stop(&mut self, id: AudioVoiceId32) -> Result<(), AudioError32> {
        self.ready()?;
        self.stop_stream(id)
    }

    pub fn stop_stream(&mut self, id: AudioVoiceId32) -> Result<(), AudioError32> {
        self.can_stop_stream(id)?;
        let voice = self.voice_mut(id)?;
        let revision = voice
            .revision
            .checked_add(1)
            .ok_or(AudioError32::Exhausted)?;
        voice.revision = revision;
        voice.playing = false;
        voice.producing = false;
        voice.produced_position = voice.position;
        Ok(())
    }

    pub fn can_stop_stream(&self, id: AudioVoiceId32) -> Result<(), AudioError32> {
        self.voice(id)?
            .revision
            .checked_add(1)
            .ok_or(AudioError32::Exhausted)?;
        Ok(())
    }
    pub fn stop_primary_stream(&mut self, id: AudioVoiceId32) -> Result<(), AudioError32> {
        self.can_stop_stream(id)?;
        let voice = self.voice_mut(id)?;
        voice.revision += 1;
        voice.playing = false;
        voice.producing = false;
        voice.position = 0;
        voice.produced_position = 0;
        Ok(())
    }

    pub fn seek(&mut self, id: AudioVoiceId32, frame: u32) -> Result<(), AudioError32> {
        self.ready()?;
        self.seek_stream(id, frame)
    }

    pub fn seek_stream(&mut self, id: AudioVoiceId32, frame: u32) -> Result<(), AudioError32> {
        let voice = self.voice(id)?;
        if frame >= self.buffer(voice.buffer)?.frames() {
            return Err(AudioError32::InvalidRequest);
        }
        let voice = self.voice_mut(id)?;
        let revision = voice
            .revision
            .checked_add(1)
            .ok_or(AudioError32::Exhausted)?;
        voice.revision = revision;
        voice.position = u64::from(frame) << 32;
        voice.produced_position = voice.position;
        voice.producing = voice.playing;
        Ok(())
    }

    pub fn set_voice_controls(
        &mut self,
        id: AudioVoiceId32,
        controls: AudioVoiceControls32,
    ) -> Result<(), AudioError32> {
        self.can_set_voice_controls(id, controls)?;
        let step = (u64::from(controls.frequency) << 32) / u64::from(self.config.sample_rate);
        let voice = self.voice_mut(id)?;
        voice.controls = controls;
        voice.step = step;
        Ok(())
    }

    pub fn can_set_voice_controls(
        &self,
        id: AudioVoiceId32,
        controls: AudioVoiceControls32,
    ) -> Result<(), AudioError32> {
        if !(100..=200_000).contains(&controls.frequency)
            || controls
                .gains
                .iter()
                .any(|gain| !gain.is_finite() || *gain < 0.0 || *gain > 16.0)
        {
            return Err(AudioError32::InvalidRequest);
        }
        self.voice(id)?;
        Ok(())
    }

    pub fn voice_controls(&self, id: AudioVoiceId32) -> Result<AudioVoiceControls32, AudioError32> {
        Ok(self.voice(id)?.controls)
    }

    pub fn voice_state(&self, id: AudioVoiceId32) -> Result<AudioVoiceState32, AudioError32> {
        let voice = self.voice(id)?;
        Ok(AudioVoiceState32 {
            buffer: voice.buffer,
            position_frames: (voice.position >> 32) as u32,
            produced_position_frames: (voice.produced_position >> 32) as u32,
            playing: voice.playing,
            producing: voice.producing,
            looping: voice.looping,
        })
    }

    pub(super) fn voice(&self, id: AudioVoiceId32) -> Result<&Voice, AudioError32> {
        if id.epoch != self.epoch || id.generation == 0 {
            return Err(AudioError32::InvalidVoice);
        }
        self.voices
            .get(id.slot as usize)
            .filter(|slot| slot.generation == id.generation)
            .and_then(|slot| slot.voice.as_ref())
            .ok_or(AudioError32::InvalidVoice)
    }

    pub(super) fn voice_mut(&mut self, id: AudioVoiceId32) -> Result<&mut Voice, AudioError32> {
        self.voice(id)?;
        Ok(self.voices[id.slot as usize].voice.as_mut().unwrap())
    }
}
