use crate::windows::text::CodePage32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NlsProfile32 {
    pub locale: u32,
    pub ansi: CodePage32,
    pub oem: CodePage32,
    pub time_zone_bias_minutes: i32,
    pub max_text_units: usize,
}
impl Default for NlsProfile32 {
    fn default() -> Self {
        Self {
            locale: 0x409,
            ansi: CodePage32::Windows1252,
            oem: CodePage32::Oem437,
            time_zone_bias_minutes: 0,
            max_text_units: 1024 * 1024,
        }
    }
}
impl NlsProfile32 {
    pub fn validate(self) -> Result<(), NlsError32> {
        if super::locales::locale_identity(self.locale).is_err()
            || CodePage32::from_id(self.ansi.id()) != Some(self.ansi)
            || CodePage32::from_id(self.oem.id()) != Some(self.oem)
            || !(-1440..=1440).contains(&self.time_zone_bias_minutes)
            || !(1..=4 * 1024 * 1024).contains(&self.max_text_units)
        {
            return Err(NlsError32::InvalidParameter);
        }
        Ok(())
    }
    pub fn locale(self, value: u32) -> Result<u32, NlsError32> {
        let locale = match value {
            0 | 0x400 | 0x800 => self.locale,
            _ => value,
        };
        super::locales::locale_identity(locale)?;
        Ok(locale)
    }
    pub fn code_page(self, value: u32) -> Result<CodePage32, NlsError32> {
        match value {
            0 | 3 => Ok(self.ansi),
            1 => Ok(self.oem),
            _ => CodePage32::from_id(value).ok_or(NlsError32::InvalidParameter),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NlsError32 {
    InvalidParameter,
    InvalidFlags,
    InsufficientBuffer,
    NoTranslation,
    Unsupported,
    Capacity,
    Allocation,
    Memory(crate::memory::MemoryError),
}
impl NlsError32 {
    pub fn win32(self) -> u32 {
        match self {
            Self::InvalidFlags => 1004,
            Self::InsufficientBuffer => 122,
            Self::NoTranslation => 1113,
            Self::Unsupported => 50,
            Self::Capacity | Self::Allocation => 8,
            Self::Memory(_) => 998,
            Self::InvalidParameter => 87,
        }
    }
}
impl From<crate::memory::MemoryError> for NlsError32 {
    fn from(value: crate::memory::MemoryError) -> Self {
        Self::Memory(value)
    }
}
