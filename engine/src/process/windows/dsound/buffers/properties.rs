use super::super::{
    DSoundRuntime32,
    memory::{output, wave, write_words},
    objects::ObjectKind32,
};
use crate::{
    audio::{
        AudioContext,
        dsound::{Buffer, BufferDescription, HResult, WaveFormat},
    },
    memory::{AddressSpace, GuestAddress},
};
impl DSoundRuntime32 {
    pub(in crate::process::windows::dsound) fn buffer_control(
        &mut self,
        audio: &mut AudioContext,
        pointer: GuestAddress,
        slot: u8,
        value: u32,
    ) -> Result<u32, HResult> {
        let index = self.slot(pointer)?;
        let ObjectKind32::Buffer(buffer) = &self.records[index].as_ref().unwrap().kind else {
            return Err(HResult::INVALID_PARAM);
        };
        let device = buffer.device;
        let mut candidate = buffer.native;
        match slot {
            15 => {
                if candidate.description.flags & BufferDescription::VOLUME == 0 {
                    return Err(HResult::CONTROL_UNAVAILABLE);
                }
                let value = value as i32;
                if !(-10_000..=0).contains(&value) {
                    return Err(HResult::INVALID_PARAM);
                }
                candidate.volume = value;
            }
            16 => {
                if candidate.description.flags & BufferDescription::PAN == 0
                    || candidate.description.flags & BufferDescription::CONTROL_3D != 0
                {
                    return Err(HResult::CONTROL_UNAVAILABLE);
                }
                let value = value as i32;
                if !(-10_000..=10_000).contains(&value) {
                    return Err(HResult::INVALID_PARAM);
                }
                candidate.pan = value;
            }
            17 => {
                if candidate.primary
                    || candidate.description.flags & BufferDescription::FREQUENCY == 0
                {
                    return Err(HResult::CONTROL_UNAVAILABLE);
                }
                let value = if value == 0 {
                    candidate.format.pcm.sample_rate
                } else {
                    value
                };
                if !(100..=200_000).contains(&value) {
                    return Err(HResult::INVALID_PARAM);
                }
                candidate.frequency = value;
            }
            _ => return Err(HResult::INVALID_PARAM),
        }
        candidate.apply_controls(audio, &self.device(device)?.native)?;
        if candidate.primary && slot == 15 {
            let device_index = self.slot(device)?;
            let ObjectKind32::Device(device) =
                &mut self.records[device_index].as_mut().unwrap().kind
            else {
                unreachable!()
            };
            device.native.volume = candidate.volume;
        }
        let ObjectKind32::Buffer(buffer) = &mut self.records[index].as_mut().unwrap().kind else {
            unreachable!()
        };
        buffer.native = candidate;
        self.apply_device_controls(audio, device)?;
        Ok(0)
    }
    pub(in crate::process::windows::dsound) fn apply_device_controls(
        &self,
        audio: &mut AudioContext,
        device: GuestAddress,
    ) -> Result<(), HResult> {
        let native = &self.device(device)?.native;
        for object in self.records.iter().flatten() {
            if let ObjectKind32::Buffer(buffer) = &object.kind
                && buffer.device == device
            {
                buffer.native.apply_controls(audio, native)?;
            }
        }
        Ok(())
    }
    pub(in crate::process::windows::dsound) fn buffer_format(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        pointer: GuestAddress,
        input: u32,
    ) -> Result<u32, HResult> {
        let format = wave(memory, input)?;
        let index = self.slot(pointer)?;
        let ObjectKind32::Buffer(buffer) = &self.records[index].as_ref().unwrap().kind else {
            return Err(HResult::INVALID_PARAM);
        };
        if !buffer.native.primary {
            return Err(HResult::INVALID_CALL);
        }
        let device_pointer = buffer.device;
        let device = self.device(device_pointer)?;
        if device.native.cooperative_level < 2 {
            return Err(HResult::PRIORITY_LEVEL);
        }
        let identity = self.com.lookup(pointer)?;
        if self
            .locks
            .entries
            .iter()
            .any(|lock| lock.object == identity.id)
        {
            return Err(HResult::INVALID_CALL);
        }
        let old = buffer.native;
        let state = audio.voice_state(old.voice)?;
        let mut description = device.native;
        description.format = format;
        let mut replacement = Buffer::create(audio, &description, old.description)?;
        replacement.volume = old.volume;
        if let Err(error) = replacement.apply_controls(audio, &description) {
            super::super::lifetime::discard(
                audio,
                &ObjectKind32::Buffer(super::super::objects::Buffer32 {
                    native: replacement,
                    device: device_pointer,
                    version8: false,
                    notifications: Vec::new(),
                }),
            );
            return Err(error);
        }
        if state.playing {
            audio.play_stream(replacement.voice, true)?;
        }
        let device_index = self.slot(device_pointer)?;
        let ObjectKind32::Device(device) = &mut self.records[device_index].as_mut().unwrap().kind
        else {
            unreachable!()
        };
        device.native.format = format;
        let ObjectKind32::Buffer(buffer) = &mut self.records[index].as_mut().unwrap().kind else {
            unreachable!()
        };
        buffer.native = replacement;
        audio.release_stream_voice(old.voice)?;
        audio.release_stream_buffer(old.storage)?;
        Ok(0)
    }
}

pub(in crate::process::windows::dsound) fn get_format(
    memory: &mut AddressSpace,
    format: WaveFormat,
    out: u32,
    capacity: u32,
    required: u32,
) -> Result<u32, HResult> {
    let length = format.encoded_length() as u32;
    if out == 0 {
        if required == 0 {
            return Err(HResult::INVALID_PARAM);
        }
        write_words(memory, required, &[length])?;
        return Ok(0);
    }
    if capacity < length {
        return Err(HResult::INVALID_PARAM);
    }
    output(memory, out, length as usize)?;
    if required != 0 {
        output(memory, required, 4)?;
    }
    let encoded = format.encode();
    let size = length.to_le_bytes();
    let spans = [
        (GuestAddress(out), &encoded[..length as usize]),
        (GuestAddress(required), &size[..]),
    ];
    let count = if required == 0 { 1 } else { 2 };
    memory.write_spans(&spans[..count])?;
    Ok(0)
}
