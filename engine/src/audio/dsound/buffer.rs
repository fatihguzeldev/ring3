use super::{Buffer3D, BufferDescription, Device, HResult, WaveFormat, spatial::attenuation};
use crate::audio::{AudioBufferId32, AudioContext, AudioVoiceControls32, AudioVoiceId32};

#[derive(Clone, Copy)]
pub struct Buffer {
    pub description: BufferDescription,
    pub format: WaveFormat,
    pub storage: AudioBufferId32,
    pub voice: AudioVoiceId32,
    pub volume: i32,
    pub pan: i32,
    pub frequency: u32,
    pub spatial: Buffer3D,
    pub applied_spatial: Buffer3D,
    pub lost: bool,
    pub primary: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LockSpan {
    pub offset: u32,
    pub first: u32,
    pub second: u32,
}

impl Buffer {
    pub fn create(
        audio: &mut AudioContext,
        device: &Device,
        description: BufferDescription,
    ) -> Result<Self, HResult> {
        let description = description.validate(device.version8)?;
        let primary = description.flags & BufferDescription::PRIMARY != 0;
        let format = description.format.unwrap_or(device.format);
        let length = if primary {
            audio
                .config()
                .render_frames
                .saturating_mul(format.pcm.frame_bytes())
                .max(4)
        } else {
            description.bytes
        };
        let storage = audio.create_stream_buffer(format.pcm, format.encoding, length)?;
        let voice = match audio.create_stream_voice(storage) {
            Ok(voice) => voice,
            Err(error) => {
                audio.release_stream_buffer(storage).unwrap();
                return Err(error.into());
            }
        };
        Ok(Self {
            description,
            format,
            storage,
            voice,
            volume: 0,
            pan: 0,
            frequency: format.pcm.sample_rate,
            spatial: Buffer3D::default(),
            applied_spatial: Buffer3D::default(),
            lost: false,
            primary,
        })
    }
    pub fn duplicate(&self, audio: &mut AudioContext) -> Result<Self, HResult> {
        if self.primary || self.description.flags & BufferDescription::EFFECTS != 0 {
            return Err(HResult::INVALID_CALL);
        }
        let voice = audio.create_stream_voice(self.storage)?;
        Ok(Self {
            description: self.description,
            format: self.format,
            storage: self.storage,
            voice,
            volume: self.volume,
            pan: self.pan,
            frequency: self.frequency,
            spatial: self.spatial,
            applied_spatial: self.applied_spatial,
            lost: self.lost,
            primary: false,
        })
    }
    pub fn controls(&self, device: &Device) -> AudioVoiceControls32 {
        let mut controls = if self.description.flags & BufferDescription::CONTROL_3D != 0 {
            self.applied_spatial.controls(
                device.applied_listener,
                self.frequency,
                self.volume,
                self.description.flags & BufferDescription::MUTE_AT_MAX_DISTANCE != 0,
            )
        } else {
            let gain = attenuation(self.volume);
            AudioVoiceControls32 {
                frequency: self.frequency,
                gains: [
                    gain * attenuation(-self.pan.max(0)),
                    gain * attenuation(self.pan.min(0)),
                ],
            }
        };
        let master = attenuation(device.volume);
        for gain in &mut controls.gains {
            *gain *= master;
            if device.cooperative_level == 0
                || !device.window_alive
                || !device.focused && self.description.flags & 0xc000 == 0
            {
                *gain = 0.0;
            }
        }
        controls
    }
    pub fn apply_controls(&self, audio: &mut AudioContext, device: &Device) -> Result<(), HResult> {
        audio
            .set_voice_controls(self.voice, self.controls(device))
            .map_err(Into::into)
    }
    pub fn distance_stops_playback(&self, device: &Device) -> bool {
        self.description.flags
            & (BufferDescription::CONTROL_3D | BufferDescription::MUTE_AT_MAX_DISTANCE)
            == BufferDescription::CONTROL_3D | BufferDescription::MUTE_AT_MAX_DISTANCE
            && self
                .applied_spatial
                .beyond_max_distance(device.applied_listener)
    }
    pub fn length(&self, audio: &AudioContext) -> Result<u32, HResult> {
        Ok(audio.stream_bytes(self.storage)?.len() as u32)
    }
    pub fn positions(&self, audio: &AudioContext) -> Result<(u32, u32), HResult> {
        let state = audio.voice_state(self.voice)?;
        let bytes = self.format.pcm.frame_bytes();
        let length = self.length(audio)?;
        let play = state.position_frames * bytes;
        let write = if state.playing {
            ((u64::from(state.produced_position_frames) * u64::from(bytes)
                + u64::from(audio.config().render_frames) * u64::from(bytes))
                % u64::from(length)) as u32
        } else {
            play
        };
        Ok((play, write))
    }
    pub fn lock_span(
        &self,
        audio: &AudioContext,
        offset: u32,
        length: u32,
        flags: u32,
        wrap: bool,
    ) -> Result<LockSpan, HResult> {
        if self.lost {
            return Err(HResult::BUFFER_LOST);
        }
        if self.primary {
            return Err(HResult::INVALID_CALL);
        }
        if flags & !3 != 0 {
            return Err(HResult::INVALID_PARAM);
        }
        let size = self.length(audio)?;
        let offset = if flags & 1 != 0 {
            self.positions(audio)?.1
        } else {
            offset
        };
        let length = if flags & 2 != 0 { size } else { length };
        if offset >= size || length > size {
            return Err(HResult::INVALID_PARAM);
        }
        let first = length.min(size - offset);
        let second = if wrap { length - first } else { 0 };
        Ok(LockSpan {
            offset,
            first,
            second,
        })
    }
    pub fn play(
        &self,
        audio: &mut AudioContext,
        reserved: u32,
        priority: u32,
        flags: u32,
    ) -> Result<(), HResult> {
        self.validate_play(reserved, priority, flags)?;
        audio
            .play_stream(self.voice, flags & 1 != 0)
            .map_err(Into::into)
    }
    pub fn validate_play(&self, reserved: u32, priority: u32, flags: u32) -> Result<(), HResult> {
        if self.lost {
            return Err(HResult::BUFFER_LOST);
        }
        if reserved != 0
            || flags & !0x3f != 0
            || flags & 6 == 6
            || flags & 0x18 == 0x18
            || self.primary && flags & 1 == 0
            || self.description.flags & BufferDescription::DEFERRED_LOCATION == 0
                && (priority != 0 || flags & !1 != 0)
        {
            return Err(HResult::INVALID_PARAM);
        }
        if flags & 2 != 0 {
            return Err(HResult::CONTROL_UNAVAILABLE);
        }
        Ok(())
    }
    pub fn set_spatial(&mut self, value: Buffer3D, apply: u32) -> Result<(), HResult> {
        if apply > 1 {
            return Err(HResult::INVALID_PARAM);
        }
        let value = value.validate()?;
        self.spatial = value;
        if apply == 0 {
            self.applied_spatial = value;
        }
        Ok(())
    }
    pub fn status(&self, audio: &AudioContext) -> Result<u32, HResult> {
        if self.lost {
            return Ok(2);
        }
        let state = audio.voice_state(self.voice)?;
        Ok(u32::from(state.playing) | if state.playing && state.looping { 4 } else { 0 })
    }
}
