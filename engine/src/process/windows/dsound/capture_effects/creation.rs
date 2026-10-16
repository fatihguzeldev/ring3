use super::super::{
    DSoundRuntime32,
    memory::{read, words},
    objects::ObjectKind32,
};
use crate::{
    audio::{
        capture_effects::{CaptureEffectKind as K, CaptureProcessor, class_id, instance_id},
        dsound::{HResult, WaveFormat},
    },
    memory::AddressSpace,
};

impl DSoundRuntime32 {
    pub(in crate::process::windows::dsound) fn capture_allocation_bytes(&self) -> u32 {
        self.records
            .iter()
            .flatten()
            .filter_map(|object| match &object.kind {
                ObjectKind32::CaptureBuffer(buffer) => Some(
                    buffer.native.bytes().len() as u32
                        + buffer.effects.as_ref().map_or(0, CaptureProcessor::bytes),
                ),
                _ => None,
            })
            .sum()
    }
    pub(in crate::process::windows::dsound) fn prepare_capture_effects(
        &self,
        memory: &AddressSpace,
        description: u32,
        format: WaveFormat,
        version8: bool,
        duplex: bool,
        length: u32,
    ) -> Result<Option<CaptureProcessor>, HResult> {
        let header = words::<5>(memory, description)?;
        let effects = if header[0] == 28 {
            words::<2>(
                memory,
                description.checked_add(20).ok_or(HResult::INVALID_PARAM)?,
            )?
        } else {
            [0; 2]
        };
        if header[1] & 0x200 != 0 && !version8
            || effects[0] > 2
            || effects[0] != 0 && header[1] & 0x200 == 0
        {
            return Err(HResult::INVALID_PARAM);
        }
        if effects[0] == 0 {
            return Ok(None);
        }
        let mut kinds = [K::Aec; 2];
        for (index, selected) in kinds.iter_mut().enumerate().take(effects[0] as usize) {
            let address = effects[1]
                .checked_add(index as u32 * 48)
                .ok_or(HResult::INVALID_PARAM)?;
            let descriptor = read::<48>(memory, address)?;
            let size = u32::from_le_bytes(descriptor[..4].try_into().unwrap());
            let flags = u32::from_le_bytes(descriptor[4..8].try_into().unwrap());
            if size != 48 || !matches!(flags, 1 | 2) || descriptor[40..] != [0; 8] {
                return Err(HResult::INVALID_PARAM);
            }
            if flags == 1 {
                return Err(HResult::FX_UNAVAILABLE);
            }
            let class: [u8; 16] = descriptor[8..24].try_into().unwrap();
            let kind = [K::Aec, K::NoiseSuppress]
                .into_iter()
                .find(|kind| class == class_id(*kind))
                .ok_or(HResult::FX_UNAVAILABLE)?;
            let instance: [u8; 16] = descriptor[24..40].try_into().unwrap();
            if instance != instance_id(kind, false) && instance != instance_id(kind, true)
                || !duplex && instance == instance_id(kind, true)
            {
                return Err(HResult::FX_UNAVAILABLE);
            }
            *selected = kind;
        }
        let budget = self
            .config
            .max_capture_bytes
            .checked_sub(self.capture_allocation_bytes())
            .and_then(|bytes| bytes.checked_sub(length))
            .ok_or(HResult::OUT_OF_MEMORY)?;
        Ok(Some(CaptureProcessor::new(
            format,
            &kinds[..effects[0] as usize],
            budget,
        )?))
    }
}
