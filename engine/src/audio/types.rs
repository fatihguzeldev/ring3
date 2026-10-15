#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioConfig32 {
    pub sample_rate: u32,
    pub max_buffers: u32,
    pub max_voices: u32,
    pub max_buffer_bytes: u32,
    pub render_frames: u32,
}

impl AudioConfig32 {
    /// provisioning ceilings bound bookkeeping and retained PCM, not a device's quantum.
    pub const MAX_RESOURCES: u32 = 4096;
    pub const MAX_BUFFER_BYTES: u32 = 256 * 1024 * 1024;
    pub const MAX_RENDER_FRAMES: u32 = 65536;
}

impl Default for AudioConfig32 {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            max_buffers: 64,
            max_voices: 64,
            max_buffer_bytes: 8 * 1024 * 1024,
            render_frames: 256,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PcmFormat32 {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
}

impl PcmFormat32 {
    pub(super) fn frame_bytes(self) -> u32 {
        u32::from(self.channels) * u32::from(self.bits_per_sample / 8)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioBufferId32 {
    pub epoch: u64,
    pub slot: u32,
    pub generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioVoiceId32 {
    pub epoch: u64,
    pub slot: u32,
    pub generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioVoiceState32 {
    pub buffer: AudioBufferId32,
    pub position_frames: u32,
    pub produced_position_frames: u32,
    pub playing: bool,
    pub producing: bool,
    pub looping: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioSubmission32 {
    pub epoch: u64,
    pub sequence: u64,
    pub first_frame: u64,
    pub frames: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioState32 {
    pub epoch: u64,
    pub submitted_frames: u64,
    pub consumed_frames: u64,
    pub pending: Option<AudioSubmission32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioError32 {
    InvalidConfig,
    UnsupportedFormat,
    InvalidBuffer,
    InvalidVoice,
    InvalidRequest,
    InvalidSubmission,
    Busy,
    BufferInUse,
    Capacity,
    Allocation,
    Exhausted,
}
