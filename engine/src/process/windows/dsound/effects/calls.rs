use super::super::{
    DSoundRuntime32,
    memory::{read, write_words},
    objects::ObjectKind32,
};
use crate::{
    audio::{
        AudioContext,
        dsound::{HResult as H, InterfaceKind},
        effects::{EffectKind, Parameters},
    },
    memory::{AddressSpace, GuestAddress},
};

impl DSoundRuntime32 {
    pub(in crate::process::windows::dsound) fn effect_call(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        pointer: GuestAddress,
        interface: InterfaceKind,
        slot: u8,
        args: &[u32],
    ) -> Result<u32, H> {
        let ObjectKind32::Effect(effect) = &self.object(pointer)?.kind else {
            return Err(H::INVALID_CALL);
        };
        if interface.effect() != Some(effect.kind) {
            return Err(H::NO_INTERFACE);
        }
        let object_index = self.slot(pointer)?;
        let buffer = effect.buffer;
        let index = effect.index;
        let kind = effect.kind;
        let detached = effect.detached;
        let ObjectKind32::Buffer(buffer) = &self.object(buffer)?.kind else {
            return Err(H::INVALID_CALL);
        };
        let voice = buffer.native.voice;
        if detached.is_none() && buffer.effects_generation != effect.generation {
            return Err(H::INVALID_CALL);
        }
        let parameters = match detached {
            Some(value) => value.parameters,
            None => audio
                .voice_effects(voice)?
                .ok_or(H::INVALID_CALL)?
                .parameters(index)?,
        };
        let update = match slot {
            3 => {
                let mut words = [0; 12];
                memory.check_span(
                    GuestAddress(args[1]),
                    kind.word_count() * 4,
                    crate::memory::Access::Read,
                )?;
                for (offset, word) in words[..kind.word_count()].iter_mut().enumerate() {
                    let address = args[1]
                        .checked_add(offset as u32 * 4)
                        .ok_or(H::INVALID_PARAM)?;
                    *word = u32::from_le_bytes(read(memory, address)?);
                }
                let (parameters, clamped) = Parameters::parse(
                    kind,
                    &words[..kind.word_count()],
                    buffer.native.format.pcm.sample_rate,
                )?;
                Some((parameters, None, u32::from(clamped)))
            }
            4 => {
                write_words(memory, args[1], parameters.words())?;
                None
            }
            5 if kind == EffectKind::I3dl2Reverb => {
                Some((Parameters::i3dl2_preset(args[1])?, Some(args[1]), 0))
            }
            6 if kind == EffectKind::I3dl2Reverb => {
                let value = match detached {
                    Some(value) => value.preset,
                    None => audio.effect_preset(voice, index)?,
                };
                write_words(memory, args[1], &[value])?;
                None
            }
            7 if kind == EffectKind::I3dl2Reverb => {
                if args[1] > 3 {
                    return Err(H::INVALID_PARAM);
                }
                if detached.is_some() {
                    let ObjectKind32::Effect(effect) =
                        &mut self.records[object_index].as_mut().unwrap().kind
                    else {
                        unreachable!()
                    };
                    effect.detached.as_mut().unwrap().quality = args[1];
                } else {
                    audio.set_effect_quality(voice, index, args[1])?;
                }
                None
            }
            8 if kind == EffectKind::I3dl2Reverb => {
                let value = match detached {
                    Some(value) => value.quality,
                    None => audio.effect_quality(voice, index)?,
                };
                write_words(memory, args[1], &[value])?;
                None
            }
            _ => return Err(H::INVALID_PARAM),
        };
        if let Some((parameters, preset, result)) = update {
            if detached.is_some() {
                let ObjectKind32::Effect(effect) =
                    &mut self.records[object_index].as_mut().unwrap().kind
                else {
                    unreachable!()
                };
                let value = effect.detached.as_mut().unwrap();
                value.parameters = parameters;
                if let Some(preset) = preset {
                    value.preset = preset;
                }
            } else {
                audio.set_effect_parameters(voice, index, parameters)?;
                if let Some(preset) = preset {
                    audio.set_effect_preset(voice, index, preset)?;
                }
            }
            return Ok(result);
        }
        Ok(0)
    }
}
