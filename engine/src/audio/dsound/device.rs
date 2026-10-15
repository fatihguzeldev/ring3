use super::{HResult, Listener3D, WaveFormat};
use crate::audio::{PcmFormat32, SampleEncoding32};

#[derive(Clone, Copy)]
pub struct Device {
    pub focused: bool,
    pub window_alive: bool,
    pub version8: bool,
    pub cooperative_level: u32,
    pub window: u32,
    pub speaker_config: u32,
    pub format: WaveFormat,
    pub volume: i32,
    pub listener: Listener3D,
    pub applied_listener: Listener3D,
}
impl Device {
    pub fn new(version8: bool, sample_rate: u32) -> Self {
        Self {
            focused: true,
            window_alive: true,
            version8,
            cooperative_level: 0,
            window: 0,
            speaker_config: 4,
            format: WaveFormat {
                pcm: PcmFormat32 {
                    sample_rate,
                    channels: 2,
                    bits_per_sample: 16,
                },
                encoding: SampleEncoding32::Integer,
                extensible: false,
                channel_mask: 0,
            },
            volume: 0,
            listener: Listener3D::default(),
            applied_listener: Listener3D::default(),
        }
    }
    pub fn set_cooperative_level(&mut self, window: u32, level: u32) -> Result<(), HResult> {
        if window == 0 || !(1..=4).contains(&level) {
            return Err(HResult::INVALID_PARAM);
        }
        if level == 4 {
            return Err(HResult::UNSUPPORTED);
        }
        self.window = window;
        self.cooperative_level = level;
        Ok(())
    }
    pub fn set_listener(&mut self, value: Listener3D, apply: u32) -> Result<(), HResult> {
        if apply > 1 {
            return Err(HResult::INVALID_PARAM);
        }
        let value = value.validate()?;
        self.listener = value;
        if apply == 0 {
            self.applied_listener = value;
        }
        Ok(())
    }
    pub fn caps(&self) -> [u32; 24] {
        let mut caps = [0; 24];
        caps[0] = 96;
        caps[1] = 0xf3f;
        caps[2] = 100;
        caps[3] = 200_000;
        caps[4] = 1;
        caps
    }
}
