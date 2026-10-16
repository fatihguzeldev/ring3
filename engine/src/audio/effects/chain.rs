use super::{EffectKind, Parameters, delay::Delay, filter::Filter};
use crate::audio::{AudioContext, AudioError32, AudioVoiceId32};

pub struct EffectChain {
    pub(super) effects: Vec<Effect>,
    pub(in crate::audio) bytes: u32,
}
pub(super) struct Effect {
    pub parameters: Parameters,
    pub rate: u32,
    pub delay: Delay,
    pub filter: Filter,
    pub phase: f64,
    pub envelope: f64,
    pub lowpass: [f64; 2],
    pub coefficients: [f64; 16],
    pub feedback_hf: f64,
    pub quality: u32,
    pub preset: u32,
}
impl EffectChain {
    pub const MAX_EFFECTS: usize = 32;

    pub fn prepare(kinds: &[EffectKind], rate: u32, byte_limit: u32) -> Result<Self, AudioError32> {
        if kinds.len() > Self::MAX_EFFECTS || !(1..=200_000).contains(&rate) {
            return Err(AudioError32::InvalidRequest);
        }
        let mut effects = Vec::new();
        effects
            .try_reserve_exact(kinds.len())
            .map_err(|_| AudioError32::Allocation)?;
        let mut bytes = 0u32;
        for kind in kinds {
            let milliseconds = match kind {
                EffectKind::Echo => 2000,
                EffectKind::Chorus => 40,
                EffectKind::Flanger => 8,
                EffectKind::Compressor => 4,
                EffectKind::I3dl2Reverb | EffectKind::WavesReverb => 1000,
                _ => 0,
            };
            let frames = (u64::from(rate) * milliseconds / 1000 + 2) as usize;
            bytes = bytes
                .checked_add(u32::try_from(frames * 8).map_err(|_| AudioError32::Capacity)?)
                .filter(|bytes| *bytes <= byte_limit)
                .ok_or(AudioError32::Capacity)?;
            let mut effect = Effect {
                parameters: kind.defaults(),
                rate,
                delay: Delay::new(frames)?,
                filter: Filter::default(),
                phase: 0.,
                envelope: 0.,
                lowpass: [0.; 2],
                coefficients: [0.; 16],
                feedback_hf: 0.,
                quality: 2,
                preset: 0,
            };
            effect.update_coefficients();
            effects.push(effect);
        }
        Ok(Self { effects, bytes })
    }
    pub fn parameters(&self, index: usize) -> Result<Parameters, AudioError32> {
        self.effects
            .get(index)
            .map(|effect| effect.parameters)
            .ok_or(AudioError32::InvalidRequest)
    }
    pub fn len(&self) -> usize {
        self.effects.len()
    }
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }
    pub fn set_parameters(
        &mut self,
        index: usize,
        parameters: Parameters,
    ) -> Result<(), AudioError32> {
        let effect = self
            .effects
            .get_mut(index)
            .filter(|effect| effect.parameters.kind() == parameters.kind())
            .ok_or(AudioError32::InvalidRequest)?;
        effect.parameters = parameters;
        effect.update_coefficients();
        Ok(())
    }
    pub(in crate::audio) fn process(&mut self, value: [f32; 2]) -> [f32; 2] {
        let mut value = value.map(f64::from);
        for effect in &mut self.effects {
            value = effect.process(value);
        }
        value.map(super::sample::stored_sample)
    }
}

impl AudioContext {
    pub fn prepare_voice_effects(
        &self,
        voice: AudioVoiceId32,
        kinds: &[EffectKind],
    ) -> Result<EffectChain, AudioError32> {
        let voice = self.voice(voice)?;
        if voice.playing {
            return Err(AudioError32::Busy);
        }
        let previous = voice.effects.as_ref().map_or(0, |chain| chain.bytes);
        let available = self
            .config
            .max_buffer_bytes
            .saturating_sub(self.buffer_bytes)
            .saturating_sub(self.effect_bytes - previous);
        EffectChain::prepare(kinds, self.config.sample_rate, available)
    }
    pub fn commit_voice_effects(&mut self, id: AudioVoiceId32, chain: EffectChain) {
        let previous = self
            .voice(id)
            .expect("effect voice was preflighted")
            .effects
            .as_ref()
            .map_or(0, |chain| chain.bytes);
        self.effect_bytes = self.effect_bytes - previous + chain.bytes;
        self.voice_mut(id)
            .expect("effect voice was preflighted")
            .effects = (!chain.is_empty()).then_some(chain);
    }
    pub fn voice_effects(&self, id: AudioVoiceId32) -> Result<Option<&EffectChain>, AudioError32> {
        Ok(self.voice(id)?.effects.as_ref())
    }
    pub fn set_effect_parameters(
        &mut self,
        id: AudioVoiceId32,
        index: usize,
        parameters: Parameters,
    ) -> Result<(), AudioError32> {
        self.voice_mut(id)?
            .effects
            .as_mut()
            .ok_or(AudioError32::InvalidRequest)?
            .set_parameters(index, parameters)
    }
    pub fn effect_quality(&self, id: AudioVoiceId32, index: usize) -> Result<u32, AudioError32> {
        let effect = self
            .voice(id)?
            .effects
            .as_ref()
            .and_then(|chain| chain.effects.get(index))
            .filter(|effect| effect.parameters.kind() == EffectKind::I3dl2Reverb)
            .ok_or(AudioError32::InvalidRequest)?;
        Ok(effect.quality)
    }
    pub fn set_effect_quality(
        &mut self,
        id: AudioVoiceId32,
        index: usize,
        quality: u32,
    ) -> Result<(), AudioError32> {
        if quality > 3 {
            return Err(AudioError32::InvalidRequest);
        }
        let effect = self
            .voice_mut(id)?
            .effects
            .as_mut()
            .and_then(|chain| chain.effects.get_mut(index))
            .filter(|effect| effect.parameters.kind() == EffectKind::I3dl2Reverb)
            .ok_or(AudioError32::InvalidRequest)?;
        effect.quality = quality;
        Ok(())
    }
    pub fn effect_preset(&self, id: AudioVoiceId32, index: usize) -> Result<u32, AudioError32> {
        let effect = self
            .voice(id)?
            .effects
            .as_ref()
            .and_then(|chain| chain.effects.get(index))
            .filter(|effect| effect.parameters.kind() == EffectKind::I3dl2Reverb)
            .ok_or(AudioError32::InvalidRequest)?;
        Ok(effect.preset)
    }
    pub fn set_effect_preset(
        &mut self,
        id: AudioVoiceId32,
        index: usize,
        preset: u32,
    ) -> Result<(), AudioError32> {
        if preset >= 30 {
            return Err(AudioError32::InvalidRequest);
        }
        let effect = self
            .voice_mut(id)?
            .effects
            .as_mut()
            .and_then(|chain| chain.effects.get_mut(index))
            .filter(|effect| effect.parameters.kind() == EffectKind::I3dl2Reverb)
            .ok_or(AudioError32::InvalidRequest)?;
        effect.preset = preset;
        Ok(())
    }

    pub(crate) fn copy_voice_effects(
        &self,
        source: AudioVoiceId32,
        replacement: &mut AudioContext,
        destination: AudioVoiceId32,
    ) -> Result<(), AudioError32> {
        let Some(chain) = self.voice_effects(source)? else {
            return Ok(());
        };
        let mut kinds = Vec::new();
        kinds
            .try_reserve_exact(chain.len())
            .map_err(|_| AudioError32::Allocation)?;
        kinds.extend(chain.effects.iter().map(|effect| effect.parameters.kind()));
        let mut next = replacement.prepare_voice_effects(destination, &kinds)?;
        for (old, new) in chain.effects.iter().zip(&mut next.effects) {
            new.parameters = old.parameters;
            new.quality = old.quality;
            new.preset = old.preset;
            new.update_coefficients();
        }
        replacement.commit_voice_effects(destination, next);
        Ok(())
    }
}
