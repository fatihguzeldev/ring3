use crate::memory::MemoryError;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceIdentifier32 {
    Id(u32),
    Name(Vec<u16>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceLocation32 {
    pub entry_rva: u32,
    pub data_rva: u32,
    pub size: u32,
    pub code_page: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceString32 {
    pub data_rva: u32,
    pub units: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceError32 {
    Malformed,
    Capacity,
    Allocation,
    TypeNotFound,
    NameNotFound,
    LanguageNotFound,
    Memory(MemoryError),
}
impl ResourceError32 {
    pub fn win32(self) -> u32 {
        match self {
            Self::Malformed => 13,
            Self::Capacity | Self::Allocation => 8,
            Self::TypeNotFound => 1813,
            Self::NameNotFound => 1814,
            Self::LanguageNotFound => 1815,
            Self::Memory(_) => 998,
        }
    }
}
impl From<MemoryError> for ResourceError32 {
    fn from(error: MemoryError) -> Self {
        Self::Memory(error)
    }
}
