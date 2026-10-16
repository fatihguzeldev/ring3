use crate::memory::{GuestAddress, PageRange};

#[derive(Clone, Copy, Debug)]
pub struct ActivationConfig32 {
    pub contexts: u32,
    pub frames: u32,
    pub threads: u32,
    pub manifest_bytes: u32,
    pub nodes: u32,
    pub assemblies: u32,
    pub section_bytes: u32,
    pub section_bounds: PageRange,
}
impl Default for ActivationConfig32 {
    fn default() -> Self {
        Self {
            contexts: 256,
            frames: 4096,
            threads: 256,
            manifest_bytes: 1024 * 1024,
            nodes: 16384,
            assemblies: 256,
            section_bytes: 16 * 1024 * 1024,
            section_bounds: PageRange::new(GuestAddress(0x6a00_0000), 4096).unwrap(),
        }
    }
}
impl ActivationConfig32 {
    pub fn validate(self) -> Result<(), ActivationError32> {
        if self.contexts == 0
            || self.contexts > 65536
            || self.frames == 0
            || self.frames > 65536
            || self.threads == 0
            || self.threads > 4096
            || self.manifest_bytes == 0
            || self.manifest_bytes > 16 * 1024 * 1024
            || self.nodes == 0
            || self.nodes > 1024 * 1024
            || self.assemblies == 0
            || self.assemblies > 4096
            || self.section_bytes == 0
            || u64::from(self.section_bytes)
                > self.section_bounds.count as u64 * u64::from(crate::memory::PAGE_SIZE)
            || self.section_bounds.first == 0
        {
            return Err(ActivationError32::InvalidParameter);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivationError32 {
    InvalidHandle,
    InvalidParameter,
    NotEnoughMemory,
    NoAccess,
    FileNotFound,
    ManifestFormat,
    ManifestParse,
    AssemblyNotFound,
    IdentityMismatch,
    DuplicateClass,
    DuplicateDll,
    DuplicateWindowClass,
    DuplicateInterface,
    DuplicateTypeLibrary,
    DuplicateProgId,
    KeyNotFound,
    SectionNotFound,
    EarlyDeactivation,
    InvalidDeactivation,
    Stale,
    Busy,
    Disabled,
    DefaultAlreadySet,
    Win32(u32),
}
impl ActivationError32 {
    pub fn win32(self) -> u32 {
        match self {
            Self::Win32(error) => error,
            Self::FileNotFound => 2,
            Self::InvalidHandle => 6,
            Self::NotEnoughMemory => 8,
            Self::InvalidParameter | Self::Stale => 87,
            Self::Busy => 170,
            Self::NoAccess => 998,
            Self::SectionNotFound => 14000,
            Self::Disabled => 14006,
            Self::DefaultAlreadySet => 14011,
            Self::AssemblyNotFound => 14003,
            Self::ManifestFormat => 14004,
            Self::ManifestParse => 14005,
            Self::KeyNotFound => 14007,
            Self::IdentityMismatch => 14017,
            Self::DuplicateDll => 14021,
            Self::DuplicateWindowClass => 14022,
            Self::DuplicateClass => 14023,
            Self::DuplicateInterface => 14024,
            Self::DuplicateTypeLibrary => 14025,
            Self::DuplicateProgId => 14026,
            Self::EarlyDeactivation | Self::InvalidDeactivation => 87,
        }
    }
    pub fn exception(self) -> Option<u32> {
        match self {
            Self::EarlyDeactivation => Some(0xc015_000f),
            Self::InvalidDeactivation => Some(0xc015_0010),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComRedirection32 {
    pub context: u32,
    pub assembly_roster: u32,
    pub clsid: [u8; 16],
    pub server: Vec<u16>,
    pub threading: crate::windows::com::ServerThreadingModel32,
    pub progid: Option<Vec<u16>>,
    pub typelib: Option<[u8; 16]>,
}
