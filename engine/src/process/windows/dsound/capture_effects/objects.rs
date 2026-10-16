use super::super::{
    DSoundRuntime32,
    lifetime::Publication32,
    memory::{output, read, word, words, write_words},
    objects::ObjectKind32,
};
use crate::{
    audio::dsound::{HResult, InterfaceKind},
    audio::{
        AudioContext,
        capture_effects::{CaptureEffectKind as K, class_id},
    },
    memory::{AddressSpace, GuestAddress},
};

impl DSoundRuntime32 {
    pub(in crate::process::windows::dsound) fn capture_effect_path(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        pointer: GuestAddress,
        args: &[u32],
    ) -> Result<u32, HResult> {
        output(memory, args[4], 4)?;
        let class = read::<16>(memory, args[1])?;
        let buffer = self.object(pointer)?.pointer;
        let ObjectKind32::CaptureBuffer(record) = &self.object(buffer)?.kind else {
            return Err(HResult::INVALID_PARAM);
        };
        let kind = record
            .effects
            .as_ref()
            .ok_or(HResult::OBJECT_NOT_FOUND)?
            .kinds()
            .filter(|kind| {
                class == class_id(*kind) || class == crate::audio::dsound::effects::ALL_OBJECTS.0
            })
            .nth(args[2] as usize)
            .ok_or(HResult::OBJECT_NOT_FOUND)?;
        let interface = if kind == K::Aec {
            InterfaceKind::CaptureFxAec
        } else {
            InterfaceKind::CaptureFxNoise
        };
        let iid = crate::windows::com::InterfaceId(read(memory, args[3])?);
        let requested = if iid == crate::windows::com::IID_IUNKNOWN {
            InterfaceKind::Unknown
        } else if iid == interface.iid() {
            interface
        } else {
            return Err(HResult::NO_INTERFACE);
        };
        if let Some(pointer)=self.records.iter().flatten().find_map(|object|matches!(object.kind,ObjectKind32::CaptureEffect{buffer:owner,kind:found} if owner==buffer&&found==kind).then_some(object.pointer)) {
            return self.acquire(memory,pointer,args[4],requested)
        }
        self.publish_object(
            memory,
            audio,
            ObjectKind32::CaptureEffect { buffer, kind },
            Publication32 {
                interfaces: &[interface],
                output: Some((args[4], requested)),
                retain: &[buffer],
            },
        )?;
        Ok(0)
    }
    pub(in crate::process::windows::dsound) fn capture_effect_status(
        &self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        args: &[u32],
    ) -> Result<u32, HResult> {
        let ObjectKind32::CaptureBuffer(buffer) = &self.object(pointer)?.kind else {
            return Err(HResult::INVALID_PARAM);
        };
        let count = buffer
            .effects
            .as_ref()
            .map_or(0, |effects| effects.kinds().count());
        if args[1] as usize != count {
            return Err(HResult::INVALID_PARAM);
        }
        if count != 0 {
            write_words(memory, args[2], &[0x20; 2][..count])?;
        }
        Ok(0)
    }
    pub(in crate::process::windows::dsound) fn capture_effect_call(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        interface: InterfaceKind,
        slot: u8,
        args: &[u32],
    ) -> Result<u32, HResult> {
        let ObjectKind32::CaptureEffect { buffer, kind } = self.object(pointer)?.kind else {
            return Err(HResult::INVALID_PARAM);
        };
        if interface.capture_effect() != Some(kind) {
            return Err(HResult::INVALID_CALL);
        }
        match (kind, slot) {
            (K::Aec, 3) => {
                let values = words::<3>(memory, args[1])?;
                self.capture_aec = crate::audio::capture_effects::AecParameters {
                    enabled: values[0] != 0,
                    noise_fill: values[1] != 0,
                };
            }
            (K::NoiseSuppress, 3) => self.capture_noise = words::<1>(memory, args[1])?[0] != 0,
            (K::Aec, 4) => {
                let ObjectKind32::CaptureBuffer(buffer) = &self.object(buffer)?.kind else {
                    unreachable!()
                };
                write_words(
                    memory,
                    args[1],
                    &[
                        u32::from(self.capture_aec.enabled),
                        u32::from(self.capture_aec.noise_fill),
                        buffer.effects.as_ref().unwrap().mode(),
                    ],
                )?;
            }
            (K::NoiseSuppress, 4) => {
                write_words(memory, args[1], &[u32::from(self.capture_noise)])?
            }
            (K::Aec, 5) => {
                let ObjectKind32::CaptureBuffer(buffer) = &self.object(buffer)?.kind else {
                    unreachable!()
                };
                memory
                    .write_words32(&[word(args[1], buffer.effects.as_ref().unwrap().status())])?;
            }
            (K::Aec, 6) | (K::NoiseSuppress, 5) => {
                for record in self.records.iter_mut().flatten() {
                    if let ObjectKind32::CaptureBuffer(buffer) = &mut record.kind
                        && let Some(effects) = &mut buffer.effects
                    {
                        if kind == K::Aec {
                            effects.reset()
                        } else {
                            effects.reset_noise()
                        }
                    }
                }
            }
            _ => return Err(HResult::INVALID_PARAM),
        }
        Ok(0)
    }
}
