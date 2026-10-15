use crate::audio::{AudioError32, PcmFormat32, SampleEncoding32};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HResult(pub u32);

impl HResult {
    pub const OK: Self = Self(0);
    pub const NO_VIRTUALIZATION: Self = Self(0x0878_000a);
    pub const INVALID_PARAM: Self = Self(0x8007_0057);
    pub const NO_INTERFACE: Self = Self(0x8000_4002);
    pub const NO_AGGREGATION: Self = Self(0x8004_0110);
    pub const OUT_OF_MEMORY: Self = Self(0x8007_000e);
    pub const UNSUPPORTED: Self = Self(0x8000_4001);
    pub const ALLOCATED: Self = Self(0x8878_000a);
    pub const CONTROL_UNAVAILABLE: Self = Self(0x8878_001e);
    pub const INVALID_CALL: Self = Self(0x8878_0032);
    pub const PRIORITY_LEVEL: Self = Self(0x8878_0046);
    pub const BAD_FORMAT: Self = Self(0x8878_0064);
    pub const NO_DRIVER: Self = Self(0x8878_0078);
    pub const ALREADY_INITIALIZED: Self = Self(0x8878_0082);
    pub const BUFFER_LOST: Self = Self(0x8878_0096);
    pub const OTHER_APP_PRIORITY: Self = Self(0x8878_00a0);
    pub const UNINITIALIZED: Self = Self(0x8878_00aa);
    pub const BUFFER_TOO_SMALL: Self = Self(0x8878_00b4);
    pub const FX_UNAVAILABLE: Self = Self(0x8878_00dc);
    pub const OBJECT_NOT_FOUND: Self = Self(0x8878_1161);
}

impl From<AudioError32> for HResult {
    fn from(value: AudioError32) -> Self {
        match value {
            AudioError32::Allocation | AudioError32::Capacity => Self::OUT_OF_MEMORY,
            AudioError32::UnsupportedFormat => Self::BAD_FORMAT,
            AudioError32::Busy | AudioError32::BufferInUse => Self::INVALID_CALL,
            _ => Self::INVALID_PARAM,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaveFormat {
    pub pcm: PcmFormat32,
    pub encoding: SampleEncoding32,
    pub extensible: bool,
    pub channel_mask: u32,
}

impl WaveFormat {
    pub fn validate(self) -> Result<Self, HResult> {
        if !(1..=2).contains(&self.pcm.channels)
            || !(100..=200_000).contains(&self.pcm.sample_rate)
            || !matches!(
                (self.encoding, self.pcm.bits_per_sample),
                (SampleEncoding32::Integer, 8 | 16 | 24 | 32) | (SampleEncoding32::Float, 32)
            )
            || self.channel_mask != 0
                && self.channel_mask != if self.pcm.channels == 1 { 4 } else { 3 }
        {
            return Err(HResult::BAD_FORMAT);
        }
        Ok(self)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, HResult> {
        if bytes.len() < 16 {
            return Err(HResult::INVALID_PARAM);
        }
        let word = |at| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());
        let dword = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let tag = word(0);
        let pcm = PcmFormat32 {
            channels: word(2),
            sample_rate: dword(4),
            bits_per_sample: word(14),
        };
        if !(1..=2).contains(&pcm.channels)
            || !(100..=200_000).contains(&pcm.sample_rate)
            || !matches!(pcm.bits_per_sample, 8 | 16 | 24 | 32)
            || u32::from(word(12)) != pcm.frame_bytes()
            || pcm.sample_rate.checked_mul(pcm.frame_bytes()) != Some(dword(8))
        {
            return Err(HResult::BAD_FORMAT);
        }
        let (encoding, mask) = match tag {
            1 => (SampleEncoding32::Integer, 0),
            3 if pcm.bits_per_sample == 32 => (SampleEncoding32::Float, 0),
            0xfffe if bytes.len() >= 40 && word(16) >= 22 => {
                if word(18) != 0 && word(18) != pcm.bits_per_sample {
                    return Err(HResult::BAD_FORMAT);
                }
                let mask = dword(20);
                if mask != 0 && mask != if pcm.channels == 1 { 4 } else { 3 } {
                    return Err(HResult::BAD_FORMAT);
                }
                if bytes[28..40] != [0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113] {
                    return Err(HResult::BAD_FORMAT);
                }
                let encoding = match dword(24) {
                    1 => SampleEncoding32::Integer,
                    3 if pcm.bits_per_sample == 32 => SampleEncoding32::Float,
                    _ => return Err(HResult::BAD_FORMAT),
                };
                (encoding, mask)
            }
            _ => return Err(HResult::BAD_FORMAT),
        };
        Ok(Self {
            pcm,
            encoding,
            extensible: tag == 0xfffe,
            channel_mask: mask,
        })
    }

    pub fn encoded_length(self) -> usize {
        if self.extensible { 40 } else { 18 }
    }

    pub fn encode(self) -> [u8; 40] {
        let mut bytes = [0; 40];
        let tag = if self.extensible {
            0xfffeu16
        } else if self.encoding == SampleEncoding32::Float {
            3
        } else {
            1
        };
        bytes[0..2].copy_from_slice(&tag.to_le_bytes());
        bytes[2..4].copy_from_slice(&self.pcm.channels.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.pcm.sample_rate.to_le_bytes());
        bytes[8..12]
            .copy_from_slice(&(self.pcm.sample_rate * self.pcm.frame_bytes()).to_le_bytes());
        bytes[12..14].copy_from_slice(&(self.pcm.frame_bytes() as u16).to_le_bytes());
        bytes[14..16].copy_from_slice(&self.pcm.bits_per_sample.to_le_bytes());
        bytes[16..18].copy_from_slice(&(if self.extensible { 22u16 } else { 0 }).to_le_bytes());
        if self.extensible {
            bytes[18..20].copy_from_slice(&self.pcm.bits_per_sample.to_le_bytes());
            bytes[20..24].copy_from_slice(&self.channel_mask.to_le_bytes());
            bytes[24..28].copy_from_slice(
                &(if self.encoding == SampleEncoding32::Float {
                    3u32
                } else {
                    1
                })
                .to_le_bytes(),
            );
            bytes[28..40].copy_from_slice(&[0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113]);
        }
        bytes
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferDescription {
    pub flags: u32,
    pub bytes: u32,
    pub format: Option<WaveFormat>,
    pub algorithm: [u8; 16],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpatialAlgorithm {
    Default,
    NoVirtualization,
    HrtfFull,
    HrtfLight,
}
impl SpatialAlgorithm {
    pub fn from_guid(guid: [u8; 16]) -> Result<Self, HResult> {
        if guid == [0; 16] {
            return Ok(Self::Default);
        }
        if guid[4..]
            != [
                0x1b, 0x1c, 0xd2, 0x11, 0x94, 0xf5, 0, 0xc0, 0x4f, 0xc2, 0x8a, 0xca,
            ]
        {
            return Err(HResult::INVALID_PARAM);
        }
        match u32::from_le_bytes(guid[..4].try_into().unwrap()) {
            0xc241_333f => Ok(Self::NoVirtualization),
            0xc241_3340 => Ok(Self::HrtfFull),
            0xc241_3342 => Ok(Self::HrtfLight),
            _ => Err(HResult::INVALID_PARAM),
        }
    }
    pub fn creation_result(self) -> HResult {
        match self {
            Self::Default | Self::NoVirtualization => HResult::OK,
            Self::HrtfFull | Self::HrtfLight => HResult::NO_VIRTUALIZATION,
        }
    }
}

impl BufferDescription {
    pub const PRIMARY: u32 = 1;
    pub const HARDWARE: u32 = 4;
    pub const SOFTWARE: u32 = 8;
    pub const CONTROL_3D: u32 = 0x10;
    pub const FREQUENCY: u32 = 0x20;
    pub const PAN: u32 = 0x40;
    pub const VOLUME: u32 = 0x80;
    pub const NOTIFY: u32 = 0x100;
    pub const EFFECTS: u32 = 0x200;
    pub const DEFERRED_LOCATION: u32 = 0x40000;
    pub const MUTE_AT_MAX_DISTANCE: u32 = 0x20000;

    pub fn spatial_algorithm(self) -> Result<SpatialAlgorithm, HResult> {
        SpatialAlgorithm::from_guid(self.algorithm)
    }

    pub fn validate(self, version8: bool) -> Result<Self, HResult> {
        const VALID: u32 = 0xfc3ff;
        if self.flags & !VALID != 0 || self.flags & 12 == 12 {
            return Err(HResult::INVALID_PARAM);
        }
        let algorithm = self.spatial_algorithm()?;
        if self.flags & Self::CONTROL_3D == 0 && algorithm != SpatialAlgorithm::Default {
            return Err(HResult::INVALID_PARAM);
        }
        if self.flags & Self::HARDWARE != 0 {
            return Err(HResult::CONTROL_UNAVAILABLE);
        }
        if self.flags & Self::EFFECTS != 0 {
            return Err(HResult::FX_UNAVAILABLE);
        }
        if self.flags & Self::PRIMARY != 0 {
            if self.format.is_some()
                || self.bytes != 0
                || self.flags
                    & (Self::FREQUENCY | Self::PAN | Self::NOTIFY | Self::DEFERRED_LOCATION)
                    != 0
            {
                return Err(HResult::INVALID_PARAM);
            }
        } else {
            let format = self.format.ok_or(HResult::INVALID_PARAM)?.validate()?;
            if self.bytes < 4
                || self.bytes > 0x0fff_ffff
                || !self.bytes.is_multiple_of(format.pcm.frame_bytes())
            {
                return Err(HResult::INVALID_PARAM);
            }
            if version8
                && self.flags & Self::CONTROL_3D != 0
                && (format.pcm.channels != 1 || self.flags & Self::PAN != 0)
            {
                return Err(HResult::INVALID_PARAM);
            }
        }
        Ok(self)
    }
}
