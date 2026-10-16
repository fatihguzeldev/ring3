use super::InterfaceKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FactoryGate {
    Create = 1,
    Create8 = 2,
    EnumerateA = 3,
    EnumerateW = 4,
    CaptureCreate = 5,
    CaptureCreate8 = 6,
    CaptureEnumerateA = 7,
    CaptureEnumerateW = 8,
    GetDeviceId = 9,
    FullDuplexCreate8 = 10,
}

impl FactoryGate {
    pub const ALL: [Self; 10] = [
        Self::Create,
        Self::Create8,
        Self::EnumerateA,
        Self::EnumerateW,
        Self::CaptureCreate,
        Self::CaptureCreate8,
        Self::CaptureEnumerateA,
        Self::CaptureEnumerateW,
        Self::GetDeviceId,
        Self::FullDuplexCreate8,
    ];
    pub const fn id(self) -> u32 {
        0x60000 | self as u32
    }
    pub fn from_id(id: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|gate| gate.id() == id)
    }
    pub const fn stack_words(self) -> u32 {
        match self {
            Self::Create | Self::Create8 | Self::CaptureCreate | Self::CaptureCreate8 => 3,
            Self::FullDuplexCreate8 => 10,
            _ => 2,
        }
    }
    pub const fn name(self) -> &'static str {
        match self {
            Self::Create => "DirectSoundCreate",
            Self::Create8 => "DirectSoundCreate8",
            Self::EnumerateA => "DirectSoundEnumerateA",
            Self::EnumerateW => "DirectSoundEnumerateW",
            Self::CaptureCreate => "DirectSoundCaptureCreate",
            Self::CaptureCreate8 => "DirectSoundCaptureCreate8",
            Self::CaptureEnumerateA => "DirectSoundCaptureEnumerateA",
            Self::CaptureEnumerateW => "DirectSoundCaptureEnumerateW",
            Self::GetDeviceId => "GetDeviceID",
            Self::FullDuplexCreate8 => "DirectSoundFullDuplexCreate",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Method {
    pub name: &'static str,
    pub stack_words: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MethodGate {
    pub interface: InterfaceKind,
    pub slot: u8,
}
impl MethodGate {
    pub const fn id(self) -> u32 {
        0x70000 | ((self.interface as u32) << 8) | self.slot as u32
    }
    pub fn from_id(id: u32) -> Option<Self> {
        if id & 0xffff0000 != 0x70000 {
            return None;
        }
        let interface = InterfaceKind::from_tag((id >> 8) as u8)?;
        let slot = id as u8;
        interface.methods().get(slot as usize)?;
        Some(Self { interface, slot })
    }
    pub fn method(self) -> &'static Method {
        &self.interface.methods()[self.slot as usize]
    }
}

macro_rules! methods {($($name:literal:$words:literal),* $(,)?)=>{&[$(Method{name:$name,stack_words:$words}),*]};}
impl InterfaceKind {
    pub fn methods(self) -> &'static [Method] {
        match self {
            Self::Unknown => methods!["QueryInterface":3,"AddRef":1,"Release":1],
            Self::Device => &DEVICE[..11],
            Self::Device8 => DEVICE,
            Self::Buffer => &BUFFER[..21],
            Self::Buffer8 => BUFFER,
            Self::Notify => {
                methods!["QueryInterface":3,"AddRef":1,"Release":1,"SetNotificationPositions":3]
            }
            Self::Listener => {
                methods!["QueryInterface":3,"AddRef":1,"Release":1,"GetAllParameters":2,"GetDistanceFactor":2,"GetDopplerFactor":2,"GetOrientation":3,"GetPosition":2,"GetRolloffFactor":2,"GetVelocity":2,"SetAllParameters":3,"SetDistanceFactor":3,"SetDopplerFactor":3,"SetOrientation":8,"SetPosition":5,"SetRolloffFactor":3,"SetVelocity":5,"CommitDeferredSettings":1]
            }
            Self::Buffer3D => {
                methods!["QueryInterface":3,"AddRef":1,"Release":1,"GetAllParameters":2,"GetConeAngles":3,"GetConeOrientation":2,"GetConeOutsideVolume":2,"GetMaxDistance":2,"GetMinDistance":2,"GetMode":2,"GetPosition":2,"GetVelocity":2,"SetAllParameters":3,"SetConeAngles":4,"SetConeOrientation":5,"SetConeOutsideVolume":3,"SetMaxDistance":3,"SetMinDistance":3,"SetMode":3,"SetPosition":5,"SetVelocity":5]
            }
            Self::Capture => {
                methods!["QueryInterface":3,"AddRef":1,"Release":1,"CreateCaptureBuffer":4,"GetCaps":2,"Initialize":2]
            }
            Self::CaptureBuffer => &CAPTURE_BUFFER[..12],
            Self::CaptureBuffer8 => CAPTURE_BUFFER,
            Self::FxI3dl2Reverb => {
                methods!["QueryInterface":3,"AddRef":1,"Release":1,"SetAllParameters":2,"GetAllParameters":2,"SetPreset":2,"GetPreset":2,"SetQuality":2,"GetQuality":2]
            }
            Self::FxChorus
            | Self::FxCompressor
            | Self::FxDistortion
            | Self::FxEcho
            | Self::FxFlanger
            | Self::FxGargle
            | Self::FxParametricEq
            | Self::FxWavesReverb => {
                methods!["QueryInterface":3,"AddRef":1,"Release":1,"SetAllParameters":2,"GetAllParameters":2]
            }
            Self::FullDuplex => methods!["QueryInterface":3,"AddRef":1,"Release":1,"Initialize":9],
            Self::CaptureFxAec => {
                methods!["QueryInterface":3,"AddRef":1,"Release":1,"SetAllParameters":2,"GetAllParameters":2,"GetStatus":2,"Reset":1]
            }
            Self::CaptureFxNoise => {
                methods!["QueryInterface":3,"AddRef":1,"Release":1,"SetAllParameters":2,"GetAllParameters":2,"Reset":1]
            }
        }
    }
}
const DEVICE: &[Method] = methods!["QueryInterface":3,"AddRef":1,"Release":1,"CreateSoundBuffer":4,"GetCaps":2,"DuplicateSoundBuffer":3,"SetCooperativeLevel":3,"Compact":1,"GetSpeakerConfig":2,"SetSpeakerConfig":2,"Initialize":2,"VerifyCertification":2];
const BUFFER: &[Method] = methods!["QueryInterface":3,"AddRef":1,"Release":1,"GetCaps":2,"GetCurrentPosition":3,"GetFormat":4,"GetVolume":2,"GetPan":2,"GetFrequency":2,"GetStatus":2,"Initialize":4,"Lock":8,"Play":4,"SetCurrentPosition":2,"SetFormat":2,"SetVolume":2,"SetPan":2,"SetFrequency":2,"Stop":1,"Unlock":5,"Restore":1,"SetFX":4,"AcquireResources":4,"GetObjectInPath":5];
const CAPTURE_BUFFER: &[Method] = methods!["QueryInterface":3,"AddRef":1,"Release":1,"GetCaps":2,"GetCurrentPosition":3,"GetFormat":4,"GetStatus":2,"Initialize":4,"Lock":8,"Start":2,"Stop":1,"Unlock":5,"GetObjectInPath":5,"GetFXStatus":3];
