use super::super::{
    DSoundRuntime32,
    lifetime::Publication32,
    memory::{bytes, output, read, write_words},
    objects::ObjectKind32,
};
use super::{DetachedEffect32, EffectObject32};
use crate::{
    audio::{
        AudioContext,
        dsound::{
            BufferDescription, HResult as H, InterfaceKind,
            effects::{ALL_OBJECTS, class_id},
        },
        effects::{EffectChain, EffectKind},
    },
    memory::{AddressSpace, GuestAddress},
    windows::com::{IID_IUNKNOWN, InterfaceId},
};

impl DSoundRuntime32 {
    pub(in crate::process::windows::dsound) fn buffer_effects(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        pointer: GuestAddress,
        slot: u8,
        args: &[u32],
    ) -> Result<u32, H> {
        let ObjectKind32::Buffer(buffer) = &self.object(pointer)?.kind else {
            return Err(H::INVALID_PARAM);
        };
        if buffer.native.description.flags & BufferDescription::EFFECTS == 0 {
            return Err(H::CONTROL_UNAVAILABLE);
        }
        if buffer.native.lost {
            return Err(H::BUFFER_LOST);
        }
        match slot {
            21 => self.set_effects(memory, audio, pointer, args),
            22 => {
                let count = audio
                    .voice_effects(buffer.native.voice)?
                    .map_or(0, EffectChain::len);
                if buffer.native.description.flags & BufferDescription::DEFERRED_LOCATION == 0 {
                    return Err(H::INVALID_CALL);
                }
                if args[1] & !6 != 0 || args[1] & 2 != 0 {
                    return Err(H::CONTROL_UNAVAILABLE);
                }
                if (args[3] == 0 && args[2] != 0) || (args[3] != 0 && args[2] as usize != count) {
                    return Err(H::INVALID_PARAM);
                }
                if args[3] != 0 {
                    write_words(memory, args[3], &[2; EffectChain::MAX_EFFECTS][..count])?;
                }
                Ok(0)
            }
            23 => self.effect_object(memory, audio, pointer, args),
            _ => Err(H::INVALID_PARAM),
        }
    }

    fn set_effects(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        pointer: GuestAddress,
        args: &[u32],
    ) -> Result<u32, H> {
        let ObjectKind32::Buffer(buffer) = &self.object(pointer)?.kind else {
            return Err(H::INVALID_PARAM);
        };
        let voice = buffer.native.voice;
        let generation = buffer
            .effects_generation
            .checked_add(1)
            .ok_or(H::INVALID_CALL)?;
        let canonical = self.object(pointer)?.pointer;
        let identity = self.com.lookup(pointer)?;
        if audio.voice_state(voice)?.playing
            || self
                .locks
                .entries
                .iter()
                .any(|lock| lock.object == identity.id)
        {
            return Err(H::INVALID_CALL);
        }
        let count = args[1] as usize;
        if count > EffectChain::MAX_EFFECTS || (count == 0 && (args[2] != 0 || args[3] != 0)) {
            return Err(H::INVALID_PARAM);
        }
        if args[3] != 0 {
            output(memory, args[3], count * 4)?;
        }
        let descriptors = bytes(memory, args[2], count * 32)?;
        let mut kinds = Vec::new();
        kinds
            .try_reserve_exact(count)
            .map_err(|_| H::OUT_OF_MEMORY)?;
        let mut codes = [0u32; EffectChain::MAX_EFFECTS];
        let mut failure = None;
        for (index, desc) in descriptors.chunks_exact(32).enumerate() {
            let word = |offset| u32::from_le_bytes(desc[offset..offset + 4].try_into().unwrap());
            if word(0) != 32 || word(4) & !3 != 0 || word(4) == 3 || word(24) != 0 || word(28) != 0
            {
                return Err(H::INVALID_PARAM);
            }
            let class = InterfaceId(desc[8..24].try_into().unwrap());
            let kind = EffectKind::ALL
                .into_iter()
                .find(|kind| class_id(*kind) == class);
            if let Some(kind) = kind {
                kinds.push(kind);
                if word(4) & 1 != 0 {
                    codes[index] = 4;
                    failure = Some(H::CONTROL_UNAVAILABLE);
                } else {
                    codes[index] = 0;
                }
            } else {
                codes[index] = 5;
                failure = Some(H::NO_INTERFACE);
            }
        }
        if let Some(error) = failure {
            if args[3] != 0 {
                write_words(memory, args[3], &codes[..count])?;
            }
            return Err(error);
        }
        let chain = audio.prepare_voice_effects(voice, &kinds)?;
        let mut detached = Vec::new();
        detached
            .try_reserve_exact(self.records.len())
            .map_err(|_| H::OUT_OF_MEMORY)?;
        for (index, record) in self
            .records
            .iter()
            .enumerate()
            .filter_map(|(index, record)| record.as_ref().map(|record| (index, record)))
        {
            if let ObjectKind32::Effect(effect) = &record.kind
                && effect.buffer == canonical
                && effect.detached.is_none()
            {
                detached.push((
                    index,
                    DetachedEffect32 {
                        parameters: audio
                            .voice_effects(voice)?
                            .ok_or(H::INVALID_CALL)?
                            .parameters(effect.index)?,
                        quality: if effect.kind == EffectKind::I3dl2Reverb {
                            audio.effect_quality(voice, effect.index)?
                        } else {
                            0
                        },
                        preset: if effect.kind == EffectKind::I3dl2Reverb {
                            audio.effect_preset(voice, effect.index)?
                        } else {
                            0
                        },
                    },
                ));
            }
        }
        let deferred = buffer.native.description.flags & BufferDescription::DEFERRED_LOCATION != 0;
        if args[3] != 0 {
            write_words(
                memory,
                args[3],
                &[if deferred { 3 } else { 2 }; EffectChain::MAX_EFFECTS][..count],
            )?;
        }
        audio.commit_voice_effects(voice, chain);
        for (index, snapshot) in detached {
            let ObjectKind32::Effect(effect) = &mut self.records[index].as_mut().unwrap().kind
            else {
                unreachable!()
            };
            effect.detached = Some(snapshot);
        }
        let index = self.slot(pointer)?;
        let ObjectKind32::Buffer(buffer) = &mut self.records[index].as_mut().unwrap().kind else {
            unreachable!()
        };
        buffer.effects_generation = generation;
        Ok(0)
    }

    fn effect_object(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        pointer: GuestAddress,
        args: &[u32],
    ) -> Result<u32, H> {
        output(memory, args[4], 4)?;
        let class = InterfaceId(read(memory, args[1])?);
        let iid = InterfaceId(read(memory, args[3])?);
        let ObjectKind32::Buffer(buffer) = &self.object(pointer)?.kind else {
            return Err(H::INVALID_PARAM);
        };
        let chain = audio
            .voice_effects(buffer.native.voice)?
            .ok_or(H::OBJECT_NOT_FOUND)?;
        let mut matching = 0;
        let mut found = None;
        for index in 0..chain.len() {
            let kind = chain.parameters(index)?.kind();
            if class != ALL_OBJECTS && class != class_id(kind) {
                continue;
            }
            if matching == args[2] {
                found = Some((index, kind));
                break;
            }
            matching += 1;
        }
        let (index, kind) = found.ok_or(H::OBJECT_NOT_FOUND)?;
        let interface = InterfaceKind::ALL
            .into_iter()
            .find(|interface| interface.effect() == Some(kind))
            .unwrap();
        let requested = if iid == IID_IUNKNOWN {
            InterfaceKind::Unknown
        } else if iid == interface.iid() {
            interface
        } else {
            write_words(memory, args[4], &[0])?;
            return Err(H::NO_INTERFACE);
        };
        let canonical = self.object(pointer)?.pointer;
        let generation = buffer.effects_generation;
        if let Some(existing)=self.records.iter().flatten().find(|record|matches!(&record.kind,ObjectKind32::Effect(effect)
            if effect.buffer==canonical&&effect.generation==generation&&effect.index==index&&effect.detached.is_none())).map(|record|record.pointer) {
            return self.acquire(memory,existing,args[4],requested);
        }
        self.publish_object(
            memory,
            audio,
            ObjectKind32::Effect(EffectObject32 {
                buffer: canonical,
                generation,
                index,
                kind,
                detached: None,
            }),
            Publication32 {
                interfaces: &[interface],
                output: Some((args[4], requested)),
                retain: &[canonical],
            },
        )?;
        Ok(0)
    }
}
